// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn page(player: &str) -> String {
    format!("<html><script>var ytInitialPlayerResponse = {player};var meta = 1;</script>")
}

const PLAYABLE: &str = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"isLive":false},
  "streamingData":{"adaptiveFormats":[
    {"itag":137,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1920,"height":1080,"contentLength":"8000"},
    {"itag":248,"mimeType":"video/webm; codecs=\"vp9\"","width":1920,"height":1080,"contentLength":"3000"},
    {"itag":136,"mimeType":"video/mp4; codecs=\"avc1.4d401f\"","width":1280,"height":720,"contentLength":"2000"},
    {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500"},
    {"itag":251,"mimeType":"audio/webm; codecs=\"opus\"","averageBitrate":140000,"contentLength":"400"}]}}"#;

/// Each caption track is a line after the sound, as reference A lists them (gaps row 72c).
#[test]
fn each_caption_track_is_a_line_after_the_sound() {
    let player = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"isLive":false},
      "streamingData":{"adaptiveFormats":[
        {"itag":136,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"contentLength":"2000"},
        {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500"}]},
      "captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[
        {"baseUrl":"https://www.youtube.com/api/timedtext?v=x&lang=en","languageCode":"en","name":{"simpleText":"English"}},
        {"baseUrl":"https://www.youtube.com/api/timedtext?v=x&lang=hi&kind=asr","languageCode":"hi","kind":"asr"},
        {"baseUrl":"https://e.com/t","languageCode":"../x"}]}}}"#;
    let list = choices_of_page(&page(player));
    let lines: Vec<(&str, &str)> = list
        .iter()
        .map(|c| (c.label.as_str(), c.quality.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            ("720p MP4", "720/mp4/f136"),
            ("M4A audio", "audio/m4a"),
            ("TTML file, EN subtitles", "caption/en"),
            ("TTML file, HI subtitles (ASR)", "caption/hi/asr"),
        ]
    );
}

/// A height at two frame rates is two lines, the faster first (gaps row 72d).
#[test]
fn a_height_at_two_frame_rates_is_two_lines_from_the_pages_own_data() {
    let player = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"isLive":false},
      "streamingData":{"adaptiveFormats":[
        {"itag":136,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"fps":30,"contentLength":"2000"},
        {"itag":298,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"fps":60,"contentLength":"3000"},
        {"itag":135,"mimeType":"video/mp4; codecs=\"avc1\"","width":854,"height":480,"fps":30,"contentLength":"900"},
        {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500"}]}}"#;
    let list = choices_of_page(&page(player));
    let lines: Vec<(&str, &str)> = list
        .iter()
        .map(|c| (c.label.as_str(), c.quality.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            ("720p MP4 60 fps", "720/mp4/60/f298"),
            ("720p MP4", "720/mp4/30/f136"),
            ("480p MP4", "480/mp4/f135"),
            ("M4A audio", "audio/m4a"),
        ]
    );
}

#[test]
fn a_pages_own_data_gives_the_list() {
    let list = choices_of_page(&page(PLAYABLE));
    let said: Vec<(&str, &str, Option<u64>)> = list
        .iter()
        .map(|c| (c.label.as_str(), c.quality.as_str(), c.size))
        .collect();
    assert_eq!(
        said,
        [
            ("1080p MP4", "1080/mp4/f137", Some(8500)),
            ("720p MP4", "720/mp4/f136", Some(2500)),
            ("M4A audio", "audio/m4a", Some(500)),
        ]
    );
}

#[test]
fn a_portrait_videos_qualities_are_named_by_the_shorter_side() {
    // A Short: 1080x1920 and 720x1280 are 1080 and 720, as the chooser
    // reads them, so a click on the second fetches the 720-wide stream.
    let short = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"isLive":false},
      "streamingData":{"adaptiveFormats":[
        {"itag":1,"mimeType":"video/mp4; codecs=\"avc1\"","width":1080,"height":1920,"contentLength":"9000"},
        {"itag":2,"mimeType":"video/mp4; codecs=\"avc1\"","width":720,"height":1280,"contentLength":"4000"},
        {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500"}]}}"#;
    let words: Vec<String> = choices_of_page(&page(short))
        .into_iter()
        .map(|c| c.quality)
        .collect();
    assert_eq!(words, ["1080/mp4/f1", "720/mp4/f2", "audio/m4a"]);
}

#[test]
fn a_page_that_does_not_play_or_is_live_or_has_no_data_gives_nothing() {
    let blocked = PLAYABLE.replace(r#""status":"OK""#, r#""status":"LOGIN_REQUIRED""#);
    assert!(choices_of_page(&page(&blocked)).is_empty());
    let live = PLAYABLE.replace(r#""isLive":false"#, r#""isLive":true"#);
    assert!(choices_of_page(&page(&live)).is_empty());
    assert!(choices_of_page("<html>consent</html>").is_empty());
    assert!(choices_of_page("var ytInitialPlayerResponse = {broken").is_empty());
}

#[test]
fn only_watch_pages_are_read_this_way() {
    for yes in [
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=RD",
        "https://m.youtube.com/watch?v=abc",
        "https://youtu.be/dQw4w9WgXcQ",
        "https://www.youtube.com/shorts/abc",
        "https://www.youtube.com/embed/dQw4w9WgXcQ?start=3",
        "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
        "https://www.youtube.com/live/dQw4w9WgXcQ",
        "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
    ] {
        assert!(is_watch_page(yes), "{yes}");
    }
    // An embedded player's qualities come from its video's watch page.
    assert_eq!(
        watch_address("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ").as_deref(),
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );
    for no in [
        "https://www.youtube.com/playlist?list=PL1",
        "https://www.youtube.com/@channel",
        "https://www.youtube.com/watch",
        "https://evil.example/watch?v=abc",
        "https://youtu.be/",
        "https://notyoutube.com/watch?v=a",
    ] {
        assert!(!is_watch_page(no), "{no}");
    }
}
