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
            range: None
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
