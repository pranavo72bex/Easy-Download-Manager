// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

const URL: &str = "https://cdn.example/v/show/manifest.mpd";

fn mpd(body: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT20S">{body}</MPD>"#
    )
}

const TEMPLATED: &str = r#"<Period>
  <BaseURL>media/</BaseURL>
  <AdaptationSet contentType="video" mimeType="video/mp4">
    <SegmentTemplate timescale="1000" duration="4000" startNumber="1"
        initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/seg-$Number%03d$.m4s?a=1&amp;b=2"/>
    <Representation id="v360" bandwidth="500000" width="640" height="360"/>
    <Representation id="v720" bandwidth="2500000" width="1280" height="720"/>
    <Representation id="v1080" bandwidth="5000000" width="1920" height="1080"/>
  </AdaptationSet>
  <AdaptationSet contentType="audio" mimeType="audio/mp4" lang="en">
    <Role value="main"/>
    <SegmentTemplate timescale="1000" duration="4000" initialization="a/init.mp4" media="a/$Time$.m4s"/>
    <Representation id="a1" bandwidth="128000"/>
  </AdaptationSet>
  <AdaptationSet contentType="audio" mimeType="audio/mp4" lang="en">
    <Role value="commentary"/>
    <SegmentTemplate timescale="1000" duration="4000" initialization="c/init.mp4" media="c/$Number$.m4s"/>
    <Representation id="c1" bandwidth="256000"/>
  </AdaptationSet>
</Period>"#;

#[test]
fn a_template_numbers_its_pieces_and_leads_with_the_init() {
    let plan = plan(&mpd(TEMPLATED), URL, Quality::Best, None).unwrap();
    let video = plan.video.unwrap();
    assert_eq!(video.id, "v1080");
    assert_eq!(video.pieces.len(), 6, "init and 20 s of 4 s pieces");
    assert_eq!(
        video.pieces[0].url,
        "https://cdn.example/v/show/media/v1080/init.mp4"
    );
    assert_eq!(
        video.pieces[1].url,
        "https://cdn.example/v/show/media/v1080/seg-001.m4s?a=1&b=2"
    );
    assert!(video.pieces[5].url.contains("seg-005"));
}

#[test]
fn time_templates_and_the_main_sound_over_a_louder_commentary() {
    let plan = plan(&mpd(TEMPLATED), URL, Quality::Best, None).unwrap();
    let audio = plan.audio.unwrap();
    assert_eq!(audio.id, "a1");
    assert!(audio.pieces[1].url.ends_with("/a/0.m4s"));
    assert!(audio.pieces[2].url.ends_with("/a/4000.m4s"));
}

#[test]
fn a_height_picks_the_tallest_at_or_below_and_the_smallest_when_none_fits() {
    let p = |q| {
        plan(&mpd(TEMPLATED), URL, q, None)
            .unwrap()
            .video
            .unwrap()
            .id
    };
    assert_eq!(p(Quality::UpTo(720)), "v720");
    assert_eq!(p(Quality::UpTo(500)), "v360");
    assert_eq!(p(Quality::UpTo(100)), "v360");
}

#[test]
fn audio_asks_for_no_picture() {
    let plan = plan(&mpd(TEMPLATED), URL, Quality::Audio, None).unwrap();
    assert!(plan.video.is_none());
    assert_eq!(plan.audio.unwrap().id, "a1");
}

#[test]
fn a_timeline_repeats_and_restarts_at_its_stated_time() {
    let body = r#"<Period><AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="90000" media="s-$Time$-$Number$.m4s" initialization="i.mp4" startNumber="7">
        <SegmentTimeline><S t="0" d="180000" r="2"/><S t="900000" d="90000"/></SegmentTimeline>
      </SegmentTemplate>
      <Representation id="x" bandwidth="1" height="480"/></AdaptationSet></Period>"#;
    let track = plan(&mpd(body), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    let urls: Vec<&str> = track
        .pieces
        .iter()
        .map(|p| p.url.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        urls,
        [
            "i.mp4",
            "s-0-7.m4s",
            "s-180000-8.m4s",
            "s-360000-9.m4s",
            "s-900000-10.m4s"
        ]
    );
}

#[test]
fn a_segment_list_carries_byte_ranges() {
    let body = r#"<Period><AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="1">
      <BaseURL>audio.mp4</BaseURL>
      <SegmentList><Initialization range="0-99"/>
        <SegmentURL mediaRange="100-499"/><SegmentURL mediaRange="500-999"/></SegmentList>
      </Representation></AdaptationSet></Period>"#;
    let track = plan(&mpd(body), URL, Quality::Best, None)
        .unwrap()
        .audio
        .unwrap();
    assert_eq!(track.pieces.len(), 3);
    assert_eq!(track.pieces[0].range, Some((0, 99)));
    assert_eq!(track.pieces[2].range, Some((500, 999)));
    assert_eq!(track.pieces[2].url, "https://cdn.example/v/show/audio.mp4");
}

#[test]
fn a_single_file_with_an_index_is_fetched_whole() {
    let body = r#"<Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1" height="240">
      <BaseURL>https://other.example/v.mp4</BaseURL><SegmentBase indexRange="800-1200"/>
      </Representation></AdaptationSet></Period>"#;
    let track = plan(&mpd(body), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    assert_eq!(
        track.pieces,
        vec![Piece {
            url: "https://other.example/v.mp4".into(),
            range: None,
            lock: None,
            millis: 0
        }]
    );
}

#[test]
fn live_multi_period_and_protected_manifests_are_refused_by_name() {
    let live = r#"<MPD type="dynamic"><Period/></MPD>"#;
    assert!(plan(live, URL, Quality::Best, None)
        .unwrap_err()
        .to_string()
        .contains("live"));
    let two = mpd("<Period/><Period/>");
    assert!(plan(&two, URL, Quality::Best, None)
        .unwrap_err()
        .to_string()
        .contains("period"));
    let drm = mpd(
        r#"<Period><AdaptationSet mimeType="video/mp4"><ContentProtection schemeIdUri="urn:x"/>
        <SegmentTemplate duration="1" media="m"/><Representation id="v" bandwidth="1"/></AdaptationSet></Period>"#,
    );
    assert!(plan(&drm, URL, Quality::Best, None)
        .unwrap_err()
        .to_string()
        .contains("rights system"));
}

#[test]
fn webm_is_preferred_among_equals_only_when_asked() {
    let body = r#"<Period><AdaptationSet mimeType="video/mp4"><SegmentTemplate duration="10000" timescale="1000" media="a$Number$"/>
        <Representation id="avc" bandwidth="1" height="720"/></AdaptationSet>
      <AdaptationSet mimeType="video/webm"><SegmentTemplate duration="10000" timescale="1000" media="b$Number$"/>
        <Representation id="vp9" bandwidth="1" height="720"/></AdaptationSet></Period>"#;
    let id = |c| {
        plan(&mpd(body), URL, Quality::Best, c)
            .unwrap()
            .video
            .unwrap()
            .id
    };
    assert_eq!(id(None), "avc");
    assert_eq!(id(Some("webm")), "vp9");
}

#[test]
fn garbage_and_absurd_templates_fail_without_panicking() {
    assert!(plan("not xml <<", URL, Quality::Best, None).is_err());
    assert!(plan("<html/>", URL, Quality::Best, None).is_err());
    let huge = mpd(
        r#"<Period><AdaptationSet mimeType="video/mp4"><SegmentTemplate duration="1" timescale="100000" media="$Number$"/>
        <Representation id="v" bandwidth="1"/></AdaptationSet></Period>"#,
    );
    assert!(plan(&huge, URL, Quality::Best, None).is_err());
    assert!(fill("$Bogus$", "", 0, 0, 0).is_none());
    assert_eq!(fill("a$$b$Number%05d$", "", 0, 42, 0).unwrap(), "a$b00042");
}

#[test]
fn iso_durations_read() {
    assert_eq!(iso_seconds("PT1H2M3.5S"), Some(3723.5));
    assert_eq!(iso_seconds("P1DT1S"), Some(86401.0));
    assert_eq!(iso_seconds("nonsense"), None);
}

#[test]
fn a_portrait_manifest_is_picked_by_the_shorter_side() {
    // 720p of a vertical video is 720 wide and 1280 tall.
    let portrait = TEMPLATED
        .replace(r#"width="640" height="360""#, r#"width="405" height="720""#)
        .replace(
            r#"width="1280" height="720""#,
            r#"width="720" height="1280""#,
        )
        .replace(
            r#"width="1920" height="1080""#,
            r#"width="1080" height="1920""#,
        );
    let p = |q| {
        plan(&mpd(&portrait), URL, q, None)
            .unwrap()
            .video
            .unwrap()
            .id
    };
    assert_eq!(p(Quality::UpTo(720)), "v720");
    assert_eq!(p(Quality::UpTo(1080)), "v1080");
    assert_eq!(p(Quality::UpTo(480)), "v360");
}

#[test]
fn a_part_of_a_templated_track_keeps_its_first_boxes_and_the_pieces_that_overlap() {
    // 4-second pieces: 5s..9s lies in the second (4..8s) and third (8..12s).
    let track = plan(&mpd(TEMPLATED), URL, Quality::UpTo(360), None)
        .unwrap()
        .video
        .unwrap();
    assert!(track.pieces.iter().skip(1).all(|p| p.millis == 4000));
    let (cut, starts, ends) = super::cut(&track, 5.0, 9.0).unwrap();
    assert_eq!(cut.pieces.len(), 3);
    assert_eq!(cut.pieces[0].millis, 0);
    assert!(cut.pieces[0].url.ends_with("init.mp4"));
    assert_eq!((starts, ends), (4.0, 12.0));
    assert!(super::cut(&track, 500.0, 600.0).is_none());
}

#[test]
fn a_one_file_track_is_marked_to_be_cut_by_its_own_index() {
    let body = r#"<Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1" height="240">
      <BaseURL>https://other.example/v.mp4</BaseURL><SegmentBase indexRange="800-1200"><Initialization range="0-799"/></SegmentBase>
      </Representation></AdaptationSet></Period>"#;
    let track = plan(&mpd(body), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    assert_eq!(
        track.indexed,
        Some(Indexed {
            init: Some((0, 799)),
            index: Some((800, 1200))
        })
    );
}

#[test]
fn a_negative_repeat_runs_to_the_end_of_the_period() {
    // Twenty seconds of four-second pieces after the first, which has no time of its own.
    let body = r#"<Period><AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" media="s-$Time$.m4s">
        <SegmentTimeline><S d="4000" r="-1"/></SegmentTimeline>
      </SegmentTemplate>
      <Representation id="x" bandwidth="1" height="480"/></AdaptationSet></Period>"#;
    let track = plan(&mpd(body), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    let urls: Vec<&str> = track
        .pieces
        .iter()
        .map(|p| p.url.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        urls,
        [
            "s-0.m4s",
            "s-4000.m4s",
            "s-8000.m4s",
            "s-12000.m4s",
            "s-16000.m4s"
        ]
    );
    // With no length of the period to run to, the timeline cannot be read.
    let endless = format!(r#"<?xml version="1.0"?><MPD type="static">{body}</MPD>"#);
    assert!(plan(&endless, URL, Quality::Best, None).is_err());
}

#[test]
fn a_segment_list_names_its_files_and_how_long_each_plays() {
    let body = r#"<Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1" height="240">
      <SegmentList timescale="1000" duration="2000"><Initialization sourceURL="init.mp4"/>
        <SegmentURL media="one.m4s"/><SegmentURL media="two.m4s"/></SegmentList>
      </Representation></AdaptationSet></Period>"#;
    let track = plan(&mpd(body), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    let got: Vec<(&str, u64)> = track
        .pieces
        .iter()
        .map(|p| (p.url.rsplit('/').next().unwrap(), p.millis))
        .collect();
    assert_eq!(got, [("init.mp4", 0), ("one.m4s", 2000), ("two.m4s", 2000)]);
    // A list with no timescale counts its duration in seconds.
    let seconds = body.replace(r#"timescale="1000" duration="2000""#, r#"duration="3""#);
    let track = plan(&mpd(&seconds), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    assert_eq!(track.pieces[1].millis, 3000);
}

#[test]
fn a_track_is_known_by_its_height_or_codec_and_subtitles_are_left_out() {
    let body = r#"<Period>
      <AdaptationSet><Representation id="v" bandwidth="1" height="360"><BaseURL>v.mp4</BaseURL></Representation></AdaptationSet>
      <AdaptationSet><Representation id="a" bandwidth="1" codecs="opus"><BaseURL>a.webm</BaseURL></Representation></AdaptationSet>
      <AdaptationSet mimeType="text/vtt" contentType="video"><Representation id="t" bandwidth="1"><BaseURL>t.vtt</BaseURL></Representation></AdaptationSet>
      <AdaptationSet><Representation id="x" bandwidth="1"><BaseURL>x.bin</BaseURL></Representation></AdaptationSet>
      </Period>"#;
    let plan = plan(&mpd(body), URL, Quality::Best, None).unwrap();
    assert_eq!(plan.video.as_ref().unwrap().id, "v");
    assert_eq!(plan.audio.as_ref().unwrap().id, "a");
    assert!(plan.signature().starts_with("dash v:1:1 a:1:1"));
    let sound_only = super::Plan {
        video: None,
        audio: plan.audio.clone(),
    };
    assert!(sound_only.signature().starts_with("dash - a:"));
}

#[test]
fn a_manifest_of_nothing_fetchable_says_so() {
    let only_subtitles = r#"<Period><AdaptationSet mimeType="text/vtt" contentType="audio">
      <Representation id="t" bandwidth="1"><BaseURL>t.vtt</BaseURL></Representation></AdaptationSet></Period>"#;
    let why = plan(&mpd(only_subtitles), URL, Quality::Best, None)
        .unwrap_err()
        .to_string();
    assert!(why.contains("no picture or sound dm can fetch"), "{why}");
    // A picture alone, asked for its sound.
    let picture = r#"<Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1" height="360">
      <BaseURL>v.mp4</BaseURL></Representation></AdaptationSet></Period>"#;
    let why = plan(&mpd(picture), URL, Quality::Audio, None)
        .unwrap_err()
        .to_string();
    assert!(why.contains("no picture or sound dm can fetch"), "{why}");
}

#[test]
fn a_manifest_nested_past_reason_is_refused_and_odd_text_kept_as_it_is() {
    let deep = format!("<MPD>{}{}</MPD>", "<a>".repeat(70), "</a>".repeat(70));
    let why = plan(&deep, URL, Quality::Best, None)
        .unwrap_err()
        .to_string();
    assert!(why.contains("nested too deeply"), "{why}");
    // An address with an escaped `&` in its text, as a CDN's signed one has.
    let body = r#"<Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1" height="360">
      <BaseURL>v.mp4?a=1&amp;b=&lt;2&gt;&#38;c=&#x33;</BaseURL></Representation></AdaptationSet></Period>"#;
    let track = plan(&mpd(body), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    assert_eq!(
        track.pieces[0].url,
        "https://cdn.example/v/show/v.mp4?a=1&b=%3C2%3E&c=3"
    );
    // One dm does not know is kept as it was written.
    let odd = body.replace("v.mp4?a=1&amp;b=&lt;2&gt;&#38;c=&#x33;", "v&odd;.mp4");
    let track = plan(&mpd(&odd), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    assert_eq!(track.pieces[0].url, "https://cdn.example/v/show/v&odd;.mp4");
}

#[test]
fn iso_durations_in_weeks_and_with_unknown_parts() {
    assert_eq!(iso_seconds("P1W"), Some(604_800.0));
    assert_eq!(iso_seconds("P1Y"), None);
    assert_eq!(iso_seconds("P1X"), None);
    assert_eq!(iso_seconds("PT1X"), None);
}
