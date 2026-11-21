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
        stretch: 0,
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
        stretch: 0,
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

/// A segment after its playlist's discontinuity is moved as the picture's second stretch was
/// (its clock started at 5,000,000 ticks, the remux put it at 1 s); where the two count their
/// stretches apart, or a part was asked for, it is not, as before.
#[test]
fn a_segment_after_a_discontinuity_is_moved_as_the_pictures_stretch_was() {
    let second = VttSegment {
        stretch: 1,
        ..mapped(5_000_000, 0, vec![cue(0, 500, "two")])
    };
    let texts = HlsTexts {
        renditions: vec![Rendition {
            language: "en".into(),
            segments: vec![mapped(0, 0, vec![cue(100, 200, "one")]), second],
        }],
        skipped: 0,
    };
    let clock = |moved: Vec<i64>| crate::tsmux::Clock { zero: 0, moved };
    let moved = texts.timed_on(clock(vec![0, 90_000 - 5_000_000]));
    assert_eq!(
        moved[0].cues,
        [cue(100, 200, "one"), cue(1000, 1500, "two")]
    );
    let as_before = [cue(100, 200, "one"), cue(55_555, 56_055, "two")];
    assert_eq!(texts.timed_on(clock(vec![0, 1, 2]))[0].cues, as_before);
    let part = texts.clone().skipping(1.0);
    assert_eq!(
        part.timed_on(clock(vec![0, 90_000 - 5_000_000]))[0].cues,
        as_before
    );
    assert_eq!(
        texts.timed(0)[0].cues,
        as_before,
        "an MP4 of fragments is told none"
    );
}

#[test]
fn each_segment_is_on_the_stretch_its_playlists_discontinuities_say() {
    let playlist = "#EXTM3U\n#EXT-X-DISCONTINUITY-SEQUENCE:4\n#EXTINF:1,\na.vtt\n\
        #EXT-X-DISCONTINUITY\n#EXTINF:1,\nb.vtt\n#EXTINF:1,\nc.vtt\r\n#EXT-X-DISCONTINUITY\r\n\
        #EXTINF:1,\nd.vtt\n#EXT-X-ENDLIST\n";
    assert_eq!(stretches_of(playlist), [0, 1, 1, 2]);
}

/// `ts` with every PES timestamp (PTS, DTS) moved on by `by` ticks of the 90 kHz clock: a
/// segment whose clock starts afresh, as one after an `EXT-X-DISCONTINUITY` may.
fn moved_on(ts: &[u8], by: i64) -> Vec<u8> {
    let mut out = ts.to_vec();
    for packet in out.chunks_mut(188) {
        let control = (packet[3] >> 4) & 3;
        if packet[1] & 0x40 == 0 || control & 1 == 0 {
            continue;
        }
        let at = if control & 2 != 0 {
            5 + usize::from(packet[4])
        } else {
            4
        };
        if packet.len() < at + 19 || packet[at..at + 3] != [0u8, 0, 1] {
            continue;
        }
        let flags = packet[at + 7] >> 6;
        for (stamped, from) in [(flags & 2 != 0, at + 9), (flags == 3, at + 14)] {
            if stamped {
                restamp(&mut packet[from..from + 5], by);
            }
        }
    }
    out
}

/// A PES timestamp's five bytes moved on by `by` ticks, past the 33-bit wrap where it goes;
/// its prefix and marker bits kept.
fn restamp(b: &mut [u8], by: i64) {
    let old = (i64::from(b[0] >> 1) & 7) << 30
        | i64::from(b[1]) << 22
        | i64::from(b[2] >> 1) << 15
        | i64::from(b[3]) << 7
        | i64::from(b[4] >> 1);
    let new = (old + by).rem_euclid(1 << 33);
    b[0] = (b[0] & 0xf1) | ((new >> 29) & 0x0e) as u8;
    b[1] = (new >> 22) as u8;
    b[2] = ((new >> 14) & 0xfe) as u8 | 1;
    b[3] = (new >> 7) as u8;
    b[4] = ((new << 1) & 0xfe) as u8 | 1;
}

/// A master playlist whose variant plays `picture.m3u8` with an English subtitle group, and
/// with the sound of `sound.m3u8` where `sound`.
fn master(sound: bool) -> Vec<u8> {
    let (group, said) = if sound {
        (
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n",
            ",AUDIO=\"a\"",
        )
    } else {
        ("", "")
    };
    format!(
        "#EXTM3U\n{group}\
        #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English\",LANGUAGE=\"en\",\
        URI=\"subs/en.m3u8\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,SUBTITLES=\"subs\"{said}\n\
        picture.m3u8\n"
    )
    .into_bytes()
}

/// A transport stream whose clock starts afresh at a discontinuity (its second segment stamped
/// 10,000 s on): the remux puts the second segment's first picture (218090 on its own clock) a
/// frame at 30 a second after the first's last (209090), at 212090: 0.956 s from the MP4's first
/// time (the sound's, 126000). The English segment after the playlist's discontinuity, mapped
/// to the second clock, is moved with it: "Two" at 0 of that clock shows at 956 ms, after "One"
/// (0.2 s to 0.6 s of the first).
#[tokio::test]
async fn cues_after_a_discontinuity_follow_the_picture_the_remux_moved_on() {
    const BY: i64 = 900_000_000;
    let first = include_bytes!("../../../tests/fixtures/media/hls/h264/seg0.ts").to_vec();
    let second = include_bytes!("../../../tests/fixtures/media/hls/h264/seg1.ts");
    let picture = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\none.ts\n\
        #EXT-X-DISCONTINUITY\n#EXTINF:1,\ntwo.ts\n#EXT-X-ENDLIST\n";
    let subs = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nen0.vtt\n\
        #EXT-X-DISCONTINUITY\n#EXTINF:1,\nen1.vtt\n#EXT-X-ENDLIST\n";
    let one = "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:126000,LOCAL:00:00:00.000\n\n\
        00:00.200 --> 00:00.600\nOne\n";
    let two = format!(
        "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:{},LOCAL:00:00:00.000\n\n\
        00:00.000 --> 00:00.500\nTwo\n",
        218_090 + BY
    );
    let files = vec![
        ("/hls/master.m3u8".to_string(), master(false)),
        ("/hls/picture.m3u8".into(), picture.as_bytes().to_vec()),
        ("/hls/one.ts".into(), first),
        ("/hls/two.ts".into(), moved_on(second, BY)),
        ("/hls/subs/en.m3u8".into(), subs.as_bytes().to_vec()),
        ("/hls/subs/en0.vtt".into(), one.as_bytes().to_vec()),
        ("/hls/subs/en1.vtt".into(), two.into_bytes()),
    ];
    let addr = serve_files(files, Default::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let options = Options {
        subtitles: true,
        remux: true,
        ..options(dir.path())
    };
    let url = format!("{addr}/hls/master.m3u8");
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mp4");
    let shown = text_track_of(&done.path);
    assert_eq!(shown, Some(vec![200, 400, 356, 500, 1]));
}

/// `file` (an MP4 of fragments) with each fragment's first time (`tfdt`) moved on by `by`
/// ticks of its track.
fn later(mut file: Vec<u8>, by: u64) -> Vec<u8> {
    let at: Vec<usize> = file
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == b"tfdt")
        .map(|(i, _)| i)
        .collect();
    for i in at {
        if file[i + 4] == 1 {
            let time = u64::from_be_bytes(file[i + 8..i + 16].try_into().unwrap());
            file[i + 8..i + 16].copy_from_slice(&(time + by).to_be_bytes());
        } else {
            let time = u32::from_be_bytes(file[i + 8..i + 12].try_into().unwrap());
            let moved = u32::try_from(u64::from(time) + by).unwrap();
            file[i + 8..i + 12].copy_from_slice(&moved.to_be_bytes());
        }
    }
    file
}

/// A picture and its sound kept in playlists of their own, joined into Matroska where Options
/// asks for it, carry their English as an MP4 would: timed from the file's first time, both
/// tracks' fragments saying 10 s (900000 ticks), where the subtitles map cue time 0. "Hello"
/// shows at 0.5 s, "Again" at 1.5 s; with Embed subtitles off, no subtitle track.
#[tokio::test]
async fn an_hls_pair_joined_into_matroska_carries_its_subtitles_from_its_first_time() {
    let fixture = |name: &str| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/mp4mux");
        std::fs::read(path.join(name)).unwrap()
    };
    let picture = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:2,\npicture.m4s\n#EXT-X-ENDLIST\n";
    let sound = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:2,\nsound.m4s\n#EXT-X-ENDLIST\n";
    let mut files = served(picture, 900_000);
    files[0].1 = master(true);
    files.push(("/hls/sound.m3u8".into(), sound.as_bytes().to_vec()));
    let seen = later(fixture("picture.f137.mp4"), 10 * 15_360);
    files.push(("/hls/picture.m4s".into(), seen));
    let heard = later(fixture("sound.f140.m4a"), 10 * 44_100);
    files.push(("/hls/sound.m4s".into(), heard));
    let addr = serve_files(files, Default::default()).await;
    let url = format!("{addr}/hls/master.m3u8");
    for subtitles in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let options = Options {
            subtitles,
            container: Some("mkv".into()),
            ..options(dir.path())
        };
        let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
            .await
            .unwrap();
        assert_eq!(done.path.extension().unwrap(), "mkv");
        let cues = crate::mkvmux::testing::cues_in(&done.path);
        if subtitles {
            let said = [(500, "Hello".to_string()), (1500, "Again".to_string())];
            assert_eq!(cues, said);
        } else {
            assert!(cues.is_empty(), "Options says none: {cues:?}");
        }
    }
}
