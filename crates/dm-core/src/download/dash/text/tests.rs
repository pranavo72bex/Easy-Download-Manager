// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::download::dash::tests::{cut_into_pieces, options, serve_files};

/// A manifest of one period, served from 127.0.0.1: a picture in two pieces whose fragments
/// start at 10 s (`tfdt`, 900000 ticks), and English WebVTT in two pieces a template names,
/// `en/1.vtt` and `en/2.vtt`, holding `pieces`. Gives back the manifest's address.
async fn served(dir: &Path, pieces: [&str; 2]) -> String {
    let (video, _) = crate::mp4mux::testing::fragmented_from(dir, "v.mp4", 2, 900_000);
    let (init, parts) = cut_into_pieces(&std::fs::read(video).unwrap());
    let mpd = r#"<MPD type="static" mediaPresentationDuration="PT0.2S"><Period>
        <AdaptationSet mimeType="video/mp4">
        <SegmentTemplate timescale="10" duration="1" initialization="v/init.mp4"
            media="v/$Number$.m4s"/>
        <Representation id="v" bandwidth="9" height="360"/></AdaptationSet>
        <AdaptationSet mimeType="text/vtt" lang="en">
        <SegmentTemplate timescale="10" duration="1" media="en/$Number$.vtt"/>
        <Representation id="en" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
    let files = vec![
        ("/show/video.mpd".to_string(), mpd.as_bytes().to_vec()),
        ("/show/v/init.mp4".to_string(), init),
        ("/show/v/1.m4s".to_string(), parts[0].clone()),
        ("/show/v/2.m4s".to_string(), parts[1].clone()),
        ("/show/en/1.vtt".to_string(), pieces[0].as_bytes().to_vec()),
        ("/show/en/2.vtt".to_string(), pieces[1].as_bytes().to_vec()),
    ];
    let base = serve_files(files, Default::default()).await;
    format!("{base}/show/video.mpd")
}

/// The saved file and the lengths of the samples of its text track, where it has one.
async fn saved(pieces: [&str; 2], subtitles: bool) -> (PathBuf, Option<Vec<u32>>) {
    let dir = tempfile::tempdir().unwrap();
    let url = served(dir.path(), pieces).await;
    let options = Options {
        subtitles,
        ..options(dir.path())
    };
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap();
    let text = crate::mp4mux::testing::read_back(&done.path)
        .into_iter()
        .find(|t| &t.handler == b"sbtl")
        .map(|t| t.durations);
    (done.path.file_name().unwrap().into(), text)
}

/// WebVTT in pieces, the second with no header of its own and repeating the first's cue: its
/// cues joined, that one once, on the period's timeline, which the file starts at. "One" at
/// 0.5 s, "Two" at 1 s; with Embed subtitles off, no text track.
#[tokio::test]
async fn webvtt_in_pieces_is_joined_into_one_track_timed_from_the_periods_start() {
    let pieces = [
        "WEBVTT\n\n00:00.500 --> 00:01.000\nOne\n",
        "00:00.500 --> 00:01.000\nOne\n\n00:01.000 --> 00:01.500\nTwo\n",
    ];
    let (name, text) = saved(pieces, true).await;
    assert_eq!(name, PathBuf::from("show.mp4"));
    assert_eq!(text, Some(vec![500, 500, 500, 1]));
    assert_eq!(saved(pieces, false).await.1, None, "Options says none");
}

/// Pieces that map their cues to the media's 90 kHz clock (`X-TIMESTAMP-MAP`, MPEGTS 990000:
/// 11 s) are timed from where the file starts on it (10 s, its first `tfdt`): "Mapped" at 0 of
/// its piece shows at 1 s, once though both pieces write it.
#[tokio::test]
async fn pieces_that_map_their_cues_are_timed_from_the_files_first_time() {
    let piece = "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:990000,LOCAL:00:00:00.000\n\n\
        00:00.000 --> 00:00.500\nMapped\n";
    let (_, text) = saved([piece, piece], true).await;
    assert_eq!(text, Some(vec![1000, 500, 1]));
}
