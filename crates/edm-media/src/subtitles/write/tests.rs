// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::subtitles::parse_vtt;

fn cue(start: u64, end: u64, text: &str) -> Cue {
    Cue {
        start,
        end,
        text: text.into(),
    }
}

#[test]
fn times_are_written_as_each_format_writes_them() {
    let cues = [
        cue(1000, 3500, "one"),
        cue(3_723_045, 3_724_000, "an hour in"),
        cue(360_000_000, 360_000_001, "a hundred hours in"),
    ];
    let srt = to_srt(&cues);
    assert!(
        srt.starts_with("1\n00:00:01,000 --> 00:00:03,500\none\n\n"),
        "{srt}"
    );
    assert!(srt.contains("2\n01:02:03,045 --> 01:02:04,000\nan hour in\n\n"));
    assert!(srt.contains("3\n100:00:00,000 --> 100:00:00,001\n"));
    let vtt = to_vtt(&cues);
    assert!(
        vtt.starts_with("WEBVTT\n\n00:00:01.000 --> 00:00:03.500\none\n\n"),
        "{vtt}"
    );
    assert!(vtt.contains("01:02:03.045 --> 01:02:04.000\nan hour in\n"));
}

#[test]
fn srt_keeps_the_simple_styles_and_reads_the_entities() {
    let cues = [cue(
        0,
        1000,
        "<v Bob>Hello <i>there</i></v> <c.yellow><b>A &amp; B</b></c> &lt;3<00:00:00.500> <u>u</u>",
    )];
    assert_eq!(
        to_srt(&cues),
        "1\n00:00:00,000 --> 00:00:01,000\nHello <i>there</i> <b>A & B</b> <3 <u>u</u>\n\n"
    );
    // WebVTT keeps the cue's text as it was written.
    assert!(to_vtt(&cues).contains("<v Bob>Hello <i>there</i></v>"));
}

#[test]
fn a_timing_arrow_or_an_empty_line_in_the_text_cannot_split_a_cue() {
    let cues = [cue(0, 1000, "first\n\n   \nnext --> after")];
    assert_eq!(
        to_srt(&cues),
        "1\n00:00:00,000 --> 00:00:01,000\nfirst\nnext -> after\n\n"
    );
    let vtt = to_vtt(&cues);
    assert!(vtt.ends_with("first\nnext --&gt; after\n\n"), "{vtt}");
    let back = parse_vtt(&vtt);
    assert_eq!(back.len(), 1, "{back:?}");
}

#[test]
fn multi_line_cues_keep_their_lines() {
    let cues = parse_vtt("WEBVTT\n\n00:01.000 --> 00:02.000\nline one\nline two\n");
    assert!(to_srt(&cues).contains("\nline one\nline two\n\n"));
    assert_eq!(parse_vtt(&to_vtt(&cues)), cues);
}

#[test]
fn overlapping_cues_are_each_written_in_start_order_with_their_own_times() {
    let cues = [
        cue(2000, 3000, "later"),
        cue(1000, 4000, "long"),
        cue(1000, 2500, "short"),
    ];
    let srt = to_srt(&cues);
    let expected = "1\n00:00:01,000 --> 00:00:02,500\nshort\n\n\
                    2\n00:00:01,000 --> 00:00:04,000\nlong\n\n\
                    3\n00:00:02,000 --> 00:00:03,000\nlater\n\n";
    assert_eq!(srt, expected);
    let back = parse_vtt(&to_vtt(&cues));
    let times: Vec<(u64, u64)> = back.iter().map(|c| (c.start, c.end)).collect();
    assert_eq!(times, [(1000, 2500), (1000, 4000), (2000, 3000)]);
}

#[test]
fn an_empty_track_is_an_empty_srt_and_a_bare_webvtt() {
    let none: [Cue; 0] = [];
    assert_eq!(to_srt(&none), "");
    assert_eq!(to_vtt(&none), "WEBVTT\n\n");
    // Cues with nothing to show, or backwards, are left out and the numbers stay in a row.
    let odd = [
        cue(0, 1000, "  "),
        cue(5000, 4000, "backwards"),
        cue(0, 1000, "<i></i>"),
        cue(1000, 2000, "kept"),
    ];
    assert_eq!(to_srt(&odd), "1\n00:00:01,000 --> 00:00:02,000\nkept\n\n");
    assert!(parse_vtt(&to_vtt(&none)).is_empty());
}

#[test]
fn files_are_written_beside_the_video_by_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("talk [id].mp4");
    let subtitle = Subtitle {
        language: "pt-BR".into(),
        cues: vec![cue(0, 1000, "olá")],
    };
    let srt = write_beside(&video, &subtitle, Format::Srt).unwrap();
    assert_eq!(srt, dir.path().join("talk [id].pt-BR.srt"));
    assert_eq!(
        std::fs::read_to_string(&srt).unwrap(),
        "1\n00:00:00,000 --> 00:00:01,000\nolá\n\n"
    );
    let vtt = write_beside(&video, &subtitle, Format::Vtt).unwrap();
    assert_eq!(vtt, dir.path().join("talk [id].pt-BR.vtt"));
    // What EDM writes it reads back beside the video.
    let found = crate::subtitles::beside(&video);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].1, subtitle);
    for bad in ["", "../x", "en.vtt", "a b"] {
        let named = Subtitle {
            language: bad.into(),
            cues: Vec::new(),
        };
        assert!(write_beside(&video, &named, Format::Srt).is_err(), "{bad}");
    }
}

proptest::proptest! {
    #[test]
    fn what_is_written_as_webvtt_reads_back(texts in proptest::collection::vec(".{0,60}", 0..6)) {
        let cues: Vec<Cue> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| cue(i as u64 * 1000, i as u64 * 1000 + 900, t))
            .collect();
        let back = parse_vtt(&to_vtt(&cues));
        let written = kept(&cues).into_iter().filter(|c| !lines(&c.text, "").is_empty()).count();
        proptest::prop_assert_eq!(back.len(), written);
        let _ = to_srt(&cues);
    }
}
