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
fn live_empty_and_protected_manifests_are_refused_by_name() {
    // A live manifest is recorded as it goes (`live`), not planned whole.
    let live = r#"<MPD type="dynamic"><Period/></MPD>"#;
    assert!(is_live(live));
    assert!(!is_live(&mpd(TEMPLATED)));
    assert!(plan(live, URL, Quality::Best, None)
        .unwrap_err()
        .to_string()
        .contains("live"));
    // Periods with nothing in them are nothing to fetch.
    let two = mpd("<Period/><Period/>");
    assert!(plan(&two, URL, Quality::Best, None)
        .unwrap_err()
        .to_string()
        .contains("no picture or sound"));
    let drm = mpd(
        r#"<Period><AdaptationSet mimeType="video/mp4"><ContentProtection schemeIdUri="urn:x"/>
        <SegmentTemplate duration="1" media="m"/><Representation id="v" bandwidth="1"/></AdaptationSet></Period>"#,
    );
    // In the same words as a protected HLS stream (gaps row Y19).
    assert!(plan(&drm, URL, Quality::Best, None)
        .unwrap_err()
        .to_string()
        .contains("copy-protected (DRM)"));
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
        protected_above: false,
        left_out: 0,
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

/// A programme's two halves with an advert between, each period with its own pieces of two
/// seconds; `advert` is the advert's picture codec.
fn three_periods(advert: &str) -> String {
    let period = |id: &str, start: u32, seconds: u32, codec: &str| {
        format!(
            r#"<Period id="{id}" start="PT{start}S" duration="PT{seconds}S">
          <AdaptationSet mimeType="video/mp4" codecs="{codec}" width="1280" height="720">
            <SegmentTemplate timescale="1000" duration="2000" initialization="{id}/v-init.mp4" media="{id}/v-$Number$.m4s"/>
            <Representation id="v" bandwidth="1000"/></AdaptationSet>
          <AdaptationSet mimeType="audio/mp4" codecs="mp4a.40.2" audioSamplingRate="48000">
            <SegmentTemplate timescale="1000" duration="2000" initialization="{id}/a-init.mp4" media="{id}/a-$Number$.m4s"/>
            <Representation id="a" bandwidth="128"/></AdaptationSet></Period>"#
        )
    };
    mpd(&[
        period("one", 0, 6, "avc1.64001f"),
        period("ad", 6, 2, advert),
        period("two", 8, 4, "avc1.64001f"),
    ]
    .concat())
}

/// The folder and file of each piece's address: `one/v-1.m4s`.
fn tails(track: &Track) -> Vec<String> {
    track
        .pieces
        .iter()
        .map(|p| {
            let mut parts = p.url.rsplit('/');
            let file = parts.next().unwrap();
            let folder = parts.next().unwrap();
            format!("{folder}/{file}")
        })
        .collect()
}

/// The file of each piece's address.
fn files(pieces: &[Piece]) -> Vec<&str> {
    pieces
        .iter()
        .map(|p| p.url.rsplit('/').next().unwrap())
        .collect()
}

#[test]
fn alike_periods_are_joined_in_order_each_later_ones_first_boxes_kept_at_its_seam() {
    let plan = plan(&three_periods("avc1.64001f"), URL, Quality::Best, None).unwrap();
    assert_eq!(plan.left_out, 0);
    let video = plan.video.unwrap();
    assert_eq!(
        tails(&video),
        [
            "one/v-init.mp4",
            "one/v-1.m4s",
            "one/v-2.m4s",
            "one/v-3.m4s",
            "ad/v-1.m4s",
            "two/v-1.m4s",
            "two/v-2.m4s"
        ]
    );
    let seams: Vec<usize> = video.seams.iter().map(|s| s.at).collect();
    assert_eq!(seams, [4, 5]);
    assert!(video.seams[0].init.url.ends_with("/ad/v-init.mp4"));
    assert!(video.seams[1].init.url.ends_with("/two/v-init.mp4"));
    // Each piece still plays its two seconds: the periods follow one another.
    assert_eq!(video.pieces.iter().map(|p| p.millis).sum::<u64>(), 12_000);
    let audio = plan.audio.unwrap();
    assert_eq!(audio.pieces.len(), 7);
    assert_eq!(audio.seams.len(), 2);
}

#[test]
fn an_advert_in_another_codec_is_left_out_and_the_halves_either_side_joined() {
    let plan = plan(&three_periods("hev1.1.6.L93.B0"), URL, Quality::Best, None).unwrap();
    assert_eq!(plan.left_out, 1, "the advert");
    let video = plan.video.unwrap();
    assert_eq!(
        tails(&video),
        [
            "one/v-init.mp4",
            "one/v-1.m4s",
            "one/v-2.m4s",
            "one/v-3.m4s",
            "two/v-1.m4s",
            "two/v-2.m4s"
        ]
    );
    assert_eq!(video.seams.len(), 1);
    assert_eq!(video.seams[0].at, 4);
    assert!(video.seams[0].init.url.ends_with("/two/v-init.mp4"));
    let audio = plan.audio.unwrap();
    assert!(audio.pieces.iter().all(|p| !p.url.contains("/ad/")));
}

#[test]
fn a_joined_track_is_cut_to_the_periods_kept() {
    let all = plan(&three_periods("avc1.64001f"), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    let without_advert = plan(&three_periods("hev1.1.6.L93.B0"), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    assert_eq!(keep_periods(&all, &[0, 2]).unwrap(), without_advert);
    let advert = keep_periods(&all, &[1]).unwrap();
    assert_eq!(tails(&advert), ["ad/v-init.mp4", "ad/v-1.m4s"]);
    assert!(advert.seams.is_empty());
    assert_eq!(keep_periods(&all, &[0, 1, 2]).unwrap(), all);
    assert!(keep_periods(&all, &[]).is_none());
    assert!(keep_periods(&all, &[3]).is_none());
}

#[test]
fn the_heaviest_group_of_alike_items_wins_the_earliest_among_equals() {
    let keys = [
        Some(Some(1)),
        Some(None),
        Some(Some(1)),
        None,
        Some(Some(2)),
    ];
    assert_eq!(heaviest(&keys, &[1.0, 5.0, 1.0, 9.0, 1.0]), [1]);
    assert_eq!(heaviest(&keys, &[3.0, 5.0, 3.0, 9.0, 1.0]), [0, 2]);
    assert_eq!(heaviest(&keys, &[1.0, 1.0, 0.0, 9.0, 1.0]), [0, 2]);
    assert!(heaviest::<u8>(&[None, None], &[1.0, 1.0]).is_empty());
}

#[test]
fn a_period_with_no_start_or_length_follows_the_one_before_to_the_streams_end() {
    let period = |media: &str| {
        format!(
            r#"<AdaptationSet mimeType="video/mp4"><SegmentTemplate timescale="1" duration="2"
            initialization="init.mp4" media="{media}-$Number$.m4s"/><Representation id="v" bandwidth="1" height="360"/></AdaptationSet>"#
        )
    };
    let body = format!(
        "<Period duration=\"PT4S\">{}</Period><Period>{}</Period>",
        period("a"),
        period("b")
    );
    let video = plan(&mpd(&body), URL, Quality::Best, None)
        .unwrap()
        .video
        .unwrap();
    // Two pieces of the first four seconds, eight of the sixteen left of twenty.
    let first = video
        .pieces
        .iter()
        .filter(|p| p.url.contains("/a-"))
        .count();
    let second = video
        .pieces
        .iter()
        .filter(|p| p.url.contains("/b-"))
        .count();
    assert_eq!((first, second), (2, 8));
}

#[test]
fn a_dates_seconds_since_1970_are_read_with_or_without_a_zone() {
    assert_eq!(periods::unix_seconds("1970-01-01T00:01:00Z"), Some(60.0));
    assert_eq!(periods::unix_seconds("1970-01-01T00:01:00.5"), Some(60.5));
    assert_eq!(
        periods::unix_seconds("1970-01-01T01:01:00+01:00"),
        Some(60.0)
    );
    assert_eq!(periods::unix_seconds("yesterday"), None);
}

#[test]
fn a_live_window_holds_the_whole_pieces_the_server_keeps() {
    // Ten and a half seconds into two-second pieces: five are whole.
    assert_eq!(periods::live_window(10.5, 2.0, Some(4.0)), (3, 2));
    assert_eq!(periods::live_window(10.5, 2.0, None), (2, 3));
    assert_eq!(periods::live_window(10.5, 2.0, Some(60.0)), (0, 5));
    assert_eq!(periods::live_window(1.0, 2.0, None), (0, 0));
    assert_eq!(periods::live_window(-1.0, 2.0, None), (0, 0));
    assert_eq!(periods::live_window(f64::NAN, 2.0, None), (0, 0));
}

/// A live manifest that started at 1970's first minute.
fn live_manifest(extra: &str, period: &str) -> String {
    format!(
        r#"<MPD type="dynamic" availabilityStartTime="1970-01-01T00:01:00Z" {extra}>{period}</MPD>"#
    )
}

const LIVE_NUMBERED: &str = r#"<Period id="p" start="PT0S"><AdaptationSet mimeType="video/mp4">
  <SegmentTemplate timescale="1000" duration="2000" startNumber="1" initialization="init.mp4" media="seg-$Number$.m4s"/>
  <Representation id="v" bandwidth="1" height="360"/></AdaptationSet></Period>"#;

#[test]
fn a_live_manifest_lists_the_numbered_pieces_the_clock_says_are_made() {
    let text = live_manifest(
        r#"timeShiftBufferDepth="PT4S" minimumUpdatePeriod="PT0.1S""#,
        LIVE_NUMBERED,
    );
    let read = live(&text, URL, Quality::Best, None, 70.5).unwrap();
    assert!(read.live);
    assert_eq!(read.every, 0.5, "kept to half a second at least");
    let track = read.periods[0].video.as_ref().unwrap();
    assert!(track.init.as_ref().unwrap().url.ends_with("/init.mp4"));
    assert_eq!(files(&track.pieces), ["seg-4.m4s", "seg-5.m4s"]);
    // With no window said, the last three; later on, the newer pieces.
    let text = live_manifest("", LIVE_NUMBERED);
    let read = live(&text, URL, Quality::Best, None, 74.5).unwrap();
    assert_eq!(read.every, 2.0);
    let track = read.periods[0].video.as_ref().unwrap();
    assert_eq!(
        files(&track.pieces),
        ["seg-5.m4s", "seg-6.m4s", "seg-7.m4s"]
    );
    // A long wait is kept to half a minute.
    let text = live_manifest(r#"minimumUpdatePeriod="PT5M""#, LIVE_NUMBERED);
    assert_eq!(
        live(&text, URL, Quality::Best, None, 70.5).unwrap().every,
        30.0
    );
    // Numbered pieces with no start to count from cannot be listed.
    let text = format!(r#"<MPD type="dynamic">{LIVE_NUMBERED}</MPD>"#);
    assert!(live(&text, URL, Quality::Best, None, 70.5).is_err());
}

#[test]
fn a_live_timeline_runs_to_its_last_whole_piece_and_a_period_not_begun_is_not_listed() {
    let periods = r#"<Period id="p" start="PT0S"><AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" initialization="init.mp4" media="t-$Time$.m4s">
        <SegmentTimeline><S t="0" d="2000" r="-1"/></SegmentTimeline></SegmentTemplate>
      <Representation id="v" bandwidth="1" height="360"/></AdaptationSet></Period>
      <Period id="later" start="PT100S"><AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" duration="2000" initialization="init2.mp4" media="l-$Number$.m4s"/>
      <Representation id="v" bandwidth="1" height="360"/></AdaptationSet></Period>"#;
    let read = live(&live_manifest("", periods), URL, Quality::Best, None, 69.0).unwrap();
    assert_eq!(read.periods.len(), 1, "the later period has not begun");
    let track = read.periods[0].video.as_ref().unwrap();
    assert_eq!(
        files(&track.pieces),
        ["t-0.m4s", "t-2000.m4s", "t-4000.m4s", "t-6000.m4s"]
    );
    // Once static, the stream has ended and is read as a whole.
    let ended = mpd(&TEMPLATED.replace("<Period>", "<Period duration=\"PT8S\">"));
    let read = live(&ended, URL, Quality::Best, None, 69.0).unwrap();
    assert!(!read.live);
    assert_eq!(read.periods[0].video.as_ref().unwrap().pieces.len(), 2);
    assert!(read.periods[0].audio.is_some());
}
