// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The codec filter over sample manifests (gaps row Y20).

use super::*;
use crate::dash::plan;
use crate::stream::Quality;

const URL: &str = "https://cdn.example/v/show/manifest.mpd";

fn mpd(body: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT20S"><Period>{body}</Period></MPD>"#
    )
}

/// A set of `mime` whose representations are `reps`, its pieces named from a template.
fn set(mime: &str, reps: &str) -> String {
    format!(
        r#"<AdaptationSet mimeType="{mime}"><SegmentTemplate timescale="1000" duration="4000" initialization="$RepresentationID$/init" media="$RepresentationID$/$Number$"/>{reps}</AdaptationSet>"#
    )
}

/// The ids of the picture and the sound taken from `body` at `quality` for `container`.
fn taken(body: &str, quality: Quality, container: Option<&str>) -> (String, String) {
    let plan = plan(&mpd(body), URL, quality, container).unwrap();
    let id = |t: Option<crate::dash::Track>| t.map(|t| t.id).unwrap_or_default();
    (id(plan.video), id(plan.audio))
}

#[test]
fn what_each_codec_fits_is_read_from_its_name() {
    let both = Fits {
        mp4: true,
        matroska: true,
    };
    assert_eq!(fits("avc1.64001f,mp4a.40.2", false), both);
    assert_eq!(fits("", false), both, "none named: taken to fit, as before");
    assert_eq!(fits("hev1.1.6.L93.B0", false), both);
    assert_eq!(fits("mp4a.6B", false), both, "MPEG audio");
    assert!(!fits("mp4a.a5", false).matroska, "AC-3 as an object type");
    assert!(!fits("dvh1.05.06", false).matroska);
    assert!(!fits("ac-4.02.01.01", false).matroska);
    assert!(
        fits("ac-4.02.01.01", false).mp4,
        "an MP4's samples go into an MP4"
    );
    assert_eq!(fits("opus", true), both);
    assert_eq!(fits("VP9", true), both);
    assert!(!fits("vp8", true).mp4 && fits("vp8", true).matroska);
    assert!(!fits("vorbis", true).mp4);
}

#[test]
fn matroska_asked_passes_over_a_picture_and_sound_it_cannot_hold() {
    let pictures = set(
        "video/mp4",
        r#"<Representation id="dv" bandwidth="16000000" height="2160" codecs="dvh1.05.06"/>
           <Representation id="hevc" bandwidth="6000000" height="1080" codecs="hvc1.2.4.L123.90"/>"#,
    );
    let sounds = set(
        "audio/mp4",
        r#"<Representation id="ac4" bandwidth="640000" codecs="ac-4.02.01.01"/>
           <Representation id="eac3" bandwidth="448000" codecs="ec-3"/>"#,
    );
    let body = format!("{pictures}{sounds}");
    let pair = |container| taken(&body, Quality::Best, container);
    assert_eq!(pair(None), ("dv".into(), "ac4".into()), "an MP4 holds both");
    assert_eq!(pair(Some("mkv")), ("hevc".into(), "eac3".into()));
    // Nothing that fits listed: the best, as before, for the join to say why.
    let only = body.replace("hvc1.2.4.L123.90", "dvh1.05.06");
    let only = only.replace(r#"codecs="ec-3""#, r#"codecs="ac-4.02.01.01""#);
    assert_eq!(
        taken(&only, Quality::Best, Some("mkv")),
        ("dv".into(), "ac4".into())
    );
}

#[test]
fn a_webm_picture_no_mp4_holds_is_taken_only_beside_a_sound_matroska_holds() {
    let vp8 = set(
        "video/webm",
        r#"<Representation id="vp8" bandwidth="3000000" height="1080" codecs="vp8"/>"#,
    );
    let avc = set(
        "video/mp4",
        r#"<Representation id="avc" bandwidth="2000000" height="720" codecs="avc1.64001f"/>"#,
    );
    let pictures = format!("{vp8}{avc}");
    let ac4 = set(
        "audio/mp4",
        r#"<Representation id="ac4" bandwidth="640000" codecs="ac-4.02.01.01"/>"#,
    );
    let aac = set(
        "audio/mp4",
        r#"<Representation id="aac" bandwidth="128000" codecs="mp4a.40.2"/>"#,
    );
    let alone = format!("{pictures}{ac4}");
    assert_eq!(
        taken(&alone, Quality::Best, None),
        ("avc".into(), "ac4".into()),
        "VP8 and AC-4 go into neither: the shorter picture an MP4 takes"
    );
    let with_aac = format!("{pictures}{ac4}{aac}");
    assert_eq!(
        taken(&with_aac, Quality::Best, None),
        ("vp8".into(), "aac".into()),
        "the taller picture, joined into Matroska beside the sound it holds"
    );
}

#[test]
fn a_sound_alone_is_one_an_m4a_holds() {
    let body = set(
        "audio/webm",
        r#"<Representation id="vorbis" bandwidth="256000" codecs="vorbis"/>
           <Representation id="opus" bandwidth="160000" codecs="opus"/>"#,
    );
    assert_eq!(
        taken(&body, Quality::Audio, None),
        (String::new(), "opus".into())
    );
    assert_eq!(
        taken(&body, Quality::Best, None),
        (String::new(), "opus".into())
    );
}
