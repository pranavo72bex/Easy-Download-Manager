# 07. Post-processing and formats

Goal: after the bytes are in, dm can name, sort, mark, open into a list, cut,
convert and tidy the result with its own code. Decoders the operating system
already provides are called as libraries; none is fetched.

Standards: ISO/IEC 11172-3 and 13818-3 (MPEG audio, Layer III), ISO/IEC
14496-3 (AAC-LC), ISO/IEC 14496-10 (H.264, for decoding frames), CompuServe
GIF 89a, LZW, PKWARE ZIP appnote (central directory), WebVTT, ISO 639-2.

## Requirements

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| F1 | **Sound saved as MP3.** dm's own MPEG-1 Layer III encoder (constant bit rate): polyphase filterbank, MDCT, psychoacoustic-lite quantisation, Huffman tables, bit reservoir, headers and Xing/Info frame. The AAC sound of a video or part is decoded to PCM first. | BUILT (encoder); decoder per platform below | `mp3.rs`, `mp3/tables.rs`, `mp3/decode/` | Encode a generated sine and a chirp; decode with dm's own MP3 decoder in the tests (`mp3/decode.rs`) and compare the spectrum peak to the input within a tolerance; frame headers valid for every frame; the file plays in a stock player (human check). |
| F2 | **AAC-LC decode.** macOS: the system decoder (AudioToolbox). Windows: Media Foundation, only where the system has an AAC decoder (a Windows "N" edition lacks one until the Media Feature Pack is added; say so in words). Linux: dm's own AAC-LC decoder (AudioSpecificConfig, frame syntax, sections, scale factors, pulses, mid/side, TNS, IMDCT with window shapes, Huffman codebooks). | macOS BUILT; Windows BUILT (written 2026-11-01); Linux PARTIAL: written, not yet built or run (gaps row 53) | `mp3/decode/`, `mf.rs`, `aac/` | Fixture AAC streams from a reference encoder run at test-build time (not shipped): decode with `aac/`, compare to the reference decoder's PCM within one least-significant bit rounding on a corpus (mono, stereo, 44.1 and 48 kHz, short blocks, TNS, PNS if used). |
| F3 | **Part saved as GIF.** Decode the part's H.264 pictures, scale, reduce to a 256-colour palette (median cut or neuquant-class quantiser), dither optionally, write GIF89a with a loop block, frame delays in centiseconds, and a global or local palette. macOS: VideoToolbox decodes. Windows: Media Foundation H.264 decoder fed with dm's own samples. Linux: no system decoder. | macOS BUILT; Windows BUILT (written 2026-11-01); Linux TO BUILD (gaps row 54) | `gif.rs`, `frames.rs`, `frames/win.rs` | Part of a fixture clip with known frames: the GIF has the expected frame count (within one), size, loop, and each frame's mean colour within a tolerance. Linux needs an H.264 baseline/main decoder: a sized work package (WP-14), tested against reference frames. |
| F4 | **H.264 decode of dm's own (Linux).** Intra and inter frames of Baseline and Main profile: NAL parsing, SPS/PPS, slice header, CAVLC (and CABAC for Main), intra prediction, motion compensation, deblocking, DPB with output order. High profile is a later step. | TO BUILD | none | Conformance streams (public JVT suite) decode to the expected checksums. Large (L). |
| F5 | **Subtitles.** Read WebVTT and put them into the MP4 as a text track (or sidecar `.vtt`/`.srt`) with the right language; plain-text extraction. Read sidecar files beside a video by name. | BUILT | `subtitles.rs`, `mp4mux/text.rs` | `subtitles.rs` tests (three exist, one a property test); add TTML and SRT reads. |
| F6 | **ZIP listing without downloading.** From the end of the file: a range request for the last bytes, find the end-of-central-directory record (also Zip64), then the central directory; list names and uncompressed sizes; a folder has no size. Falls back to nothing when the server gives no ranges. | BUILT | `zipinfo.rs`, `tests/zip_contents.rs` | Logging server: only tail ranges are read; names equal `unzip -l` of the fixture. A truncated central directory fails in words. |
| F7 | **ZIP extraction.** Into a folder of its own, never replacing; refuse whole archives that are unsafe: path traversal (`..`, absolute paths, drive letters), symlinks pointing outside, too many entries, a declared size far above the compressed size (zip bomb ratio), nested depth. Encrypted archives: ask for the password (ZipCrypto and AES). | BUILT (refusals); PARTIAL (encrypted entries not confirmed) | `extract.rs`, `docs/decisions.md` 2026-10-23 | Archives with traversal, a bomb, a symlink: each refused with a reason and nothing written. |
| F8 | **Categories.** Folder by kind from extension and type with user rules; separate download folder per category; the folder tree is created. | BUILT | `category.rs` | B16. |
| F9 | **File mark.** On Windows a downloaded file carries where it came from (`Zone.Identifier` alternate data stream, zone 3, host and referrer), so the shell asks before running a program; on macOS the quarantine attribute; none on Linux. Never on a file dm was told to open itself. | BUILT | `motw.rs` | On each system read the mark back; a file in an exempt folder has none. |
| F10 | **Antivirus hook.** After completion run a command the person set (with the file path as an argument, not interpolated into a shell string). | PARTIAL (not confirmed in `dm-core`) | `queue` | A script that records its arguments; a path with spaces and quotes arrives intact. |
| F11 | **Names.** Illegal characters, reserved names, trailing dots and spaces, case-insensitive clashes, length limits per file system, and Unicode normalisation to avoid two names that look equal. | BUILT (most) | `probe.rs`, `files.rs` | Table tests. |
| F12 | **Shape.** 9:16 and 1:1 crops without re-encoding. | BUILT | `shape.rs` | See file 05, C5. |
| F13 | **Torrent and Metalink.** BitTorrent: `.torrent` and magnet, trackers over HTTP(S) (behind a proxy), peers wire protocol, piece verification by SHA-1, file selection, seeding setting. Metalink: mirrors, pieces and hashes. | BUILT (torrent), BUILT (metalink) | `torrent.rs`, `metalink.rs`, `tests/torrent*.rs` | Loopback swarm of three peers (`tests/torrent_stress.rs`); one public test torrent. |
| F14 | **Archive formats beyond ZIP** (RAR, 7z, tar.*). Listing and extraction. | TO BUILD (DECISION: list only, or none) | none | Listing of a fixture of each; or a stated "not supported" in words. |
| F15 | **Open, open with, open folder after completion;** double-click behaviour is a setting. | BUILT | `dm-desktop` | Contract rows. |

## Platform table for decoders

| Need | Windows | macOS | Linux |
|---|---|---|---|
| AAC to PCM (for MP3) | Media Foundation, if the system has it | AudioToolbox | dm's own `aac/` (PARTIAL) |
| H.264 to pictures (for GIF) | Media Foundation | VideoToolbox | none (TO BUILD) |
| MP3 encode | dm's own | dm's own | dm's own |
| GIF write | dm's own | dm's own | dm's own |
| Credential store | Credential Manager (written, gaps row 39) | Keychain | Secret Service (written) |

## Notes

* A conversion never overwrites the source. It writes `name (audio).mp3`
  or the like through the same name-claiming as a download.
* A conversion is cancellable and reports progress through the queue row.
* A decoder that is not on the system is not offered in the list, and the
  reason is shown if the person asks.
