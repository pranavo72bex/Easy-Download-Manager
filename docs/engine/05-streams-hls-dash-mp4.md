# 05. Streams: HLS, DASH, MP4 and the live protocols

Goal: from a manifest or a media address, produce one playable file, with
picture and sound together, optionally a part of it, with no helper program.

Standards: RFC 8216 (HLS), ISO/IEC 23009-1 (DASH), ISO/IEC 14496-12 (ISO base
media file format), 14496-14 (MP4), 14496-15 (AVC in ISO BMFF), ISO/IEC
13818-1 (MPEG-2 transport stream), ISO/IEC 23001-7 (common encryption, for
refusal), Matroska/WebM, RFC 7826 (RTSP), the RTMP specification, WebVTT.

## Pipeline

```
address -> manifest read -> pick tracks -> fetch pieces (ranged engine rules)
        -> join per track -> mux picture+sound -> optional cut -> final file
```

Pieces are fetched by the same retry, limit and cancel rules as files
(file 03, 04). Each piece is staged as its own part file; the join writes the
output once and the parts are deleted.

## HLS

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| H1 | Detect a manifest by address or by the start of the body (`#EXTM3U`). | BUILT | `stream.rs` (`Kind`) | Address without `.m3u8` whose body starts with the tag. |
| H2 | Master playlist: list variants (`EXT-X-STREAM-INF`: bandwidth, resolution, codecs) and renditions (`EXT-X-MEDIA`: audio, subtitles). Pick by the quality asked; audio renditions are fetched and joined (checked live on two sites). | BUILT | `stream.rs`, `download/hls.rs`, `tests/hls.rs` | Master with three variants and an audio group: ask each quality; the file has that height and sound. |
| H3 | Media playlist: segment URIs relative to the playlist; durations; `EXT-X-MAP` initialisation section; `EXT-X-BYTERANGE` sub-ranges of one file (with the implied offset rule); `EXT-X-DISCONTINUITY`. | BUILT (byte ranges, map); PARTIAL (discontinuity handling not confirmed) | `stream.rs` | Playlist cutting one file by byte ranges; playlist with a discontinuity between two encodings. |
| H4 | Encryption: `EXT-X-KEY` METHOD=AES-128 with the key fetched (with the page's cookies and referer), IV from the attribute or the media sequence number; AES-128-CBC with PKCS#7. Segment-level key changes. `SAMPLE-AES` and any other method (and DRM key formats) are refused in words. | BUILT (AES-128); refusal BUILT | `stream.rs` (`decrypt`) | Playlist encrypted with a test key; wrong key gives a refusal; `SAMPLE-AES` gives the refusal message. |
| H5 | Segment kinds: MPEG-TS (join then put into MP4), fragmented MP4 (init plus fragments, join as is), packed audio (ADTS AAC, MP3) and WebVTT subtitle segments. | BUILT for TS and fMP4; PARTIAL for packed audio and subtitles | `tsmux/`, `fmp4.rs`, `subtitles.rs` | One fixture each; output plays and has the expected track list (read back by dm's own MP4 reader). |
| H6 | Segment fetching: all segments in parallel up to the connections setting, written in order; a failed segment is retried with the file rules; a segment of the wrong length is refetched. Segments are named by their bytes, not by guessed names (decisions 2026-10-17). | BUILT | `download/hls.rs` | Server that cuts and throttles any one response (capped request tests, `download/capped_tests.rs`). |
| H7 | Live playlists (no `EXT-X-ENDLIST`): record by polling the playlist at about half the target duration, taking new segments, until the person stops or an end tag appears; the recording is written as it arrives so a stop leaves a playable file. A recording time limit is one setting (decisions 2026-10-23). | BUILT | `stream.rs` (`is_live`), `fmp4.rs`, `docs/decisions.md` | Local server that adds a segment every 2 s; stop after 20 s; file plays. |
| H8 | Low-latency HLS parts, `EXT-X-DEFINE`, I-frame playlists, `EXT-X-DATERANGE` ads. | TO BUILD (low priority) | none | Fixture with parts: the segment is taken whole, parts ignored. |
| H9 | Variants on different hosts with different cookies or referer: the page's headers are kept for every request the stream makes (gaps row 64: a playlist read in the browser sends no referer). | PARTIAL | `videopage/fetch.rs`, `browser/src/web/streams.rs` | Server that requires the referer on segments. |

## DASH

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| D1 | Read `MPD` static: periods, adaptation sets, representations; `BaseURL` chains (relative resolution per RFC 3986). | BUILT | `dash.rs`, `dash/tests.rs` | Fixtures with nested `BaseURL`. |
| D2 | Addressing: `SegmentTemplate` with `$Number$`, `$Time$`, `$Bandwidth$`, `$RepresentationID$` and printf widths; `SegmentTimeline` with `r` repeats; `SegmentList`; `SegmentBase` with `indexRange` (a single file with a `sidx` index). | BUILT | `dash.rs` | One fixture each; the segment list equals a hand-made list. |
| D3 | Pick one picture and one sound by the quality asked (height, bandwidth, codecs), preferring codecs the muxer can write; WebM pairs go to the WebM joiner. | BUILT | `dash.rs`, `download/dash.rs` | Manifest with four pictures and two sounds. |
| D4 | Initialisation piece first, then media pieces in order; pieces fetched in parallel. | BUILT | `download/dash.rs` | Log shows init first. |
| D5 | Several periods (an ad, then the content) joined in order with timestamps rebased. | TO BUILD (refused in words today: "changes period part-way") | `dash.rs` | Two-period fixture. |
| D6 | Live (`type="dynamic"`): refresh the manifest by `minimumUpdatePeriod`, take new segments. | TO BUILD (refused in words today) | `dash.rs` | Dynamic fixture that grows. |
| D7 | `ContentProtection` / encrypted pieces: refused in words, never saved wrong. | BUILT | `dash.rs` | Fixture with `ContentProtection`. |
| D8 | Subtitles as separate adaptation sets (WebVTT, TTML). | PARTIAL (WebVTT) | `subtitles.rs` | Fixture. |

## Muxing

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| M1 | Join an MP4 picture and an MP4 sound into one MP4 without re-encoding: `moov` with two tracks, sample tables rebuilt, chunk offsets right, handlers and language set, edit lists kept so a cut part keeps its last pictures. | BUILT | `mp4mux.rs`, `mp4mux/{read,write,part,text}.rs`, `tests/mp4mux.rs` | Read the output back with dm's reader: sample counts, durations within one frame, interleave; plays in a stock player (human check). |
| M2 | Fragmented MP4 output so the file plays however far it got (live, long streams); a final non-fragmented rewrite on the person's choice. | BUILT (fragmented) | `fmp4.rs` | Kill a recording; the file plays. |
| M3 | WebM (VP9/AV1 with Opus/Vorbis) pairs joined into one WebM; clusters, cues, block timestamps, seek head. | BUILT | `mkvmux.rs` | Read back; durations equal; plays. |
| M4 | MPEG-TS to MP4: PAT and PMT, PES reassembly, H.264 NAL units to length-prefixed samples with `avcC` from SPS and PPS, AAC ADTS to `esds`, timestamps from PTS and DTS with wrap handling, B-frame composition offsets. More of the transport stream (HEVC, AC-3) is a DECISION. | BUILT (H.264, AAC); PARTIAL (others) | `tsmux/` | TS fixture with B-frames; output decodes with dm's frame reader to the same number of pictures. |
| M5 | Pick the output container by the tracks: MP4 by default; WebM only if the person asked or the codecs demand it (B-rule from decisions 2026-10-15). | BUILT | `download/dash.rs`, `videopage/` | Codec table tests. |
| M6 | Subtitles put in as `tx3g` / WebVTT tracks (MP4) or a text track (WebM), with language codes (ISO 639-2). | BUILT | `subtitles.rs`, `mp4mux/text.rs` | Fixture with two languages. |
| M7 | Metadata: title, creation time, a cover image. | PARTIAL | `mp4mux/write.rs` | Read back. |

## Parts (cutting a time span)

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| C1 | A part from a plain MP4: read `moov` (from the tail with a range request if it sits at the end), pick samples from the keyframe at or before the start to the end, fetch only the byte ranges they occupy, rebuild sample tables, keep an edit list so playback starts exactly at the asked time. | BUILT, "not yet built or tested" per gaps row 45 | `clip.rs`, `mp4mux/part.rs` | Server logging ranges: only the needed spans are read. Output duration within one frame of the ask. |
| C2 | A part from HLS: segments whose summed durations overlap the span; fMP4 trimmed exactly via sample tables; TS joined at the nearest segments. | BUILT per gaps row 45 (untested) | `clip.rs` | Fixtures with known durations. |
| C3 | A part from DASH: template, timeline, list, or single-file index; MP4 trimmed exactly; WebM at the nearest cluster. | BUILT per gaps row 45 (WebM exact trim TO BUILD) | `clip.rs` | Same. |
| C4 | A part whose server gives no ranges or no length. | TO BUILD | none | Fetch whole to a temporary file, cut, delete; say so. |
| C5 | Vertical (9:16) or square (1:1) crop without re-encoding, by the stream's own crop metadata. | BUILT | `shape.rs` | Read back the clean-aperture box. |
| C6 | Part as GIF or audio only (file 07). | see file 07 | | |

## Live protocols

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| L1 | RTMP, RTMPS, RTMPT: handshake, `connect`, `createStream`, `play`, chunk stream reassembly, AMF0, FLV tags to MP4. | BUILT | `rtmp/`, `tests/live.rs` | Local RTMP server fixture. |
| L2 | RTSP over TCP interleaved: DESCRIBE, SETUP, PLAY, RTP depacketising for H.264 and AAC. | BUILT | `rtsp.rs`, `tests/live.rs` | Local RTSP fixture. |
| L3 | MMS over HTTP (`mmsh`): ASF header and packets. | BUILT | `mms.rs` | Fixture. |

## Rules for all streams

* A stream is refused, in words, when it is encrypted by a rights system,
  uses a key method dm does not read, or needs a token dm cannot get.
* The final file is complete only if every piece was fetched; a missing piece
  fails the row. A resume refetches only the pieces not yet on disk
  (pieces are named by their bytes, so a changed playlist keeps good pieces).
* Peak memory is a few segments, not the stream.
