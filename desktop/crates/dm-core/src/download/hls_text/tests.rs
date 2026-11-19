// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::download::dash::tests::{cut_into_pieces, options, serve_files};

fn cue(start: u64, end: u64, text: &str) -> Cue {
    Cue {
        start,
        end,
        text: text.into(),
    }
}

fn mapped(mpegts: i64, local: u64, cues: Vec<Cue>) -> VttSegment {
    VttSegment {
        map: Some(TimestampMap { mpegts, local }),
        cues,
    }
}

/// A stream whose first time is 10 s (900000 ticks): a segment mapping its cue time 0 to that
/// clock's 10 s puts a cue it times at 0.5 s at 0.5 s of the file, and one mapping its cue time
/// 10 s there (cues timed as the stream is) puts its 10.5 s at 0.5 s too.
#[test]
fn a_cue_is_timed_from_the_files_first_time_by_its_segments_map() {
    let zero = 900_000;
    let from_zero = mapped(900_000, 0, vec![cue(500, 1500, "a")]);
    assert_eq!(timed(&[from_zero], zero, zero), [cue(500, 1500, "a")]);
    let as_stream = mapped(900_000, 10_000, vec![cue(10_500, 11_500, "a")]);
    assert_eq!(timed(&[as_stream], zero, zero), [cue(500, 1500, "a")]);
    // A file that starts 1 s later on the clock: the cue half a second before it is dropped,
    // one running across its start starts with it.
    let early = mapped(
        900_000,
        0,
        vec![cue(500, 900, "gone"), cue(800, 1500, "cut")],
    );
    assert_eq!(timed(&[early], zero, zero + 90_000), [cue(0, 500, "cut")]);
}

/// A segment with no map is timed from the stream's start, which a part asked for is
/// `skipped` past; a map across the 33-bit wrap is read as the short way round.
#[test]
fn a_segment_with_no_map_is_timed_from_the_streams_start_and_a_wrap_is_crossed() {
    let unmapped = VttSegment {
        map: None,
        cues: vec![cue(4500, 5000, "four and a half")],
    };
    let texts = HlsTexts {
        renditions: vec![Rendition {
            language: "en".into(),
            segments: vec![unmapped],
        }],
        skipped: 0,
    }
    .skipping(4.0);
    // The stream starts at 10 s of its clock; the file, 4 s into it.
    let read = texts.timed(900_000 + 4 * 90_000);
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].cues, [cue(500, 1000, "four and a half")]);
    // The file starts a second before the clock wraps; the segment, a second after.
    let wrapped = mapped(90_000, 0, vec![cue(0, 1000, "a")]);
    assert_eq!(timed(&[wrapped], 0, WRAP - 90_000), [cue(2000, 3000, "a")]);
}

/// A cue running across two segments, written in both, is kept once; the cues come out in
/// time order.
#[test]
fn a_cue_written_in_two_segments_is_kept_once() {
    let first = mapped(0, 0, vec![cue(1000, 3000, "both"), cue(500, 900, "first")]);
    let second = mapped(
        0,
        0,
        vec![cue(1000, 3000, "both"), cue(3000, 3500, "second")],
    );
    assert_eq!(
        timed(&[first, second], 0, 0),
        [
            cue(500, 900, "first"),
            cue(1000, 3000, "both"),
            cue(3000, 3500, "second")
        ]
    );
}

#[test]
fn a_segments_timestamp_map_is_read_from_its_header_alone() {
    let read = |vtt: &str| timestamp_map(vtt).map(|m| (m.mpegts, m.local));
    let plain = "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:900000,LOCAL:00:00:00.000\n\n\
        00:01.000 --> 00:02.000\nA\n";
    assert_eq!(read(plain), Some((900_000, 0)));
    let turned = "\u{feff}WEBVTT\r\nX-TIMESTAMP-MAP=LOCAL:00:01:00.500, MPEGTS:181\r\n\r\n";
    assert_eq!(read(turned), Some((181, 60_500)));
    assert_eq!(read("WEBVTT\n\n00:01.000 --> 00:02.000\nA\n"), None);
    let in_a_cue = "WEBVTT\n\n00:01.000 --> 00:02.000\n\
        X-TIMESTAMP-MAP=MPEGTS:1,LOCAL:00:00.000\n";
    assert_eq!(read(in_a_cue), None, "a cue's text is not the header");
    assert_eq!(
        read("WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:lots,LOCAL:00:00.000\n"),
        None
    );
}

/// A master playlist whose variant plays with an English subtitle group, its picture
/// `picture` (a media playlist) and its subtitles two WebVTT segments, the second repeating the
/// first's cue, both mapped to the clock at `mpegts`.
fn served(picture: &str, mpegts: i64) -> Vec<(String, Vec<u8>)> {
    let master = "#EXTM3U\n\
        #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English\",LANGUAGE=\"en\",\
        URI=\"subs/en.m3u8\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,SUBTITLES=\"subs\"\n\
        picture.m3u8\n";
    let subs = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nen0.vtt\n#EXTINF:1,\nen1.vtt\n\
        #EXT-X-ENDLIST\n";
    let head = format!("WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:{mpegts},LOCAL:00:00:00.000\n\n");
    let first = format!("{head}00:00.500 --> 00:01.500\nHello\n");
    let second = format!("{first}\n00:01.500 --> 00:02.000\nAgain\n");
    vec![
        ("/hls/master.m3u8".into(), master.as_bytes().to_vec()),
        ("/hls/picture.m3u8".into(), picture.as_bytes().to_vec()),
        ("/hls/subs/en.m3u8".into(), subs.as_bytes().to_vec()),
        ("/hls/subs/en0.vtt".into(), first.into_bytes()),
        ("/hls/subs/en1.vtt".into(), second.into_bytes()),
    ]
}

/// The lengths of the samples of the saved file's text track, where it has one.
fn text_track_of(path: &Path) -> Option<Vec<u32>> {
    crate::mp4mux::testing::read_back(path)
        .into_iter()
        .find(|t| &t.handler == b"sbtl")
        .map(|t| t.durations)
}

/// A transport stream remuxed into an MP4 with Embed subtitles on carries its English as a
/// text track, timed from the MP4's first time: the stand-in's sound starts at 126000 ticks
/// (1.4 s), its subtitles map cue time 0 to 216000 (2.4 s), so "Hello" at 0.5 s shows at 1.5 s,
/// "Again" after it, and the cue both segments write is there once. With it off, no text.
#[tokio::test]
async fn an_hls_transport_streams_subtitles_are_carried_inside_its_mp4() {
    let ts = include_bytes!("../../../tests/fixtures/media/hls/h264/seg0.ts").to_vec();
    let picture = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\none.ts\n#EXT-X-ENDLIST\n";
    let mut files = served(picture, 216_000);
    files.push(("/hls/one.ts".into(), ts));
    let addr = serve_files(files, Default::default()).await;
    let url = format!("{addr}/hls/master.m3u8");
    for subtitles in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let options = Options {
            subtitles,
            remux: true,
            ..options(dir.path())
        };
        let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
            .await
            .unwrap();
        assert_eq!(done.path.extension().unwrap(), "mp4");
        let data = std::fs::read(&done.path).unwrap();
        assert_eq!(data.windows(4).any(|w| w == b"tx3g"), subtitles);
        if subtitles {
            assert_eq!(text_track_of(&done.path), Some(vec![1500, 1000, 500, 1]));
        }
    }
}

/// A stream of fragmented MP4 whose picture starts at 10 s (`tfdt`) and whose subtitles map
/// cue time 0 there: made one MP4 with its English, "Hello" at 0.5 s of the file.
#[tokio::test]
async fn a_fragmented_hls_streams_subtitles_are_carried_from_its_first_time() {
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = crate::mp4mux::testing::fragmented_from(dir.path(), "v.mp4", 2, 900_000);
    let (init, pieces) = cut_into_pieces(&std::fs::read(video).unwrap());
    let picture = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MAP:URI=\"init.mp4\"\n\
        #EXTINF:0.1,\none.m4s\n#EXTINF:0.1,\ntwo.m4s\n#EXT-X-ENDLIST\n";
    let mut files = served(picture, 900_000);
    files.push(("/hls/init.mp4".into(), init));
    files.push(("/hls/one.m4s".into(), pieces[0].clone()));
    files.push(("/hls/two.m4s".into(), pieces[1].clone()));
    let addr = serve_files(files, Default::default()).await;
    let url = format!("{addr}/hls/master.m3u8");
    let options = Options {
        subtitles: true,
        ..options(dir.path())
    };
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mp4");
    assert_eq!(text_track_of(&done.path), Some(vec![500, 1000, 500, 1]));
}
