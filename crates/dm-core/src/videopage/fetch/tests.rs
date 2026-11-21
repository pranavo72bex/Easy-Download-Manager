// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

/// A reading's day is written as tags write one (gaps row Y11).
#[test]
fn a_readings_day_is_written_as_tags_write_one() {
    assert_eq!(super::iso_day("20050423"), "2005-04-23");
    assert_eq!(super::iso_day("2005"), "2005");
    assert_eq!(super::iso_day("2005042x"), "2005042x");
}
use super::*;

fn read(json: &str) -> Read {
    Read::new(serde_json::from_str(json).unwrap())
}

#[test]
fn a_reading_says_when_its_streams_are_served() {
    let none = read(r#"{"url": "https://e.com/v"}"#);
    assert_eq!(none.available_at(), None);
    let one = read(r#"{"url": "https://e.com/v", "available_at": 1700000004}"#);
    assert_eq!(one.available_at(), Some(1_700_000_004));
    let pair = read(
        r#"{"requested_formats": [
            {"url": "https://e.com/a", "available_at": 1700000002},
            {"url": "https://e.com/b", "available_at": 1700000005}
        ]}"#,
    );
    assert_eq!(pair.available_at(), Some(1_700_000_005));
}

#[test]
fn a_picture_and_sound_over_https_are_fetched_by_dm_beside_the_joined_name() {
    let r = read(
        r#"{"filename": "/s/Talk [abc].mp4", "ext": "mp4",
            "requested_formats": [
              {"format_id": "137", "ext": "mp4", "protocol": "https",
               "url": "https://cdn.example/v?itag=137",
               "http_headers": {"User-Agent": "Agent/1", "Accept": "*/*"}},
              {"format_id": "251-drc", "ext": "webm", "protocol": "https",
               "url": "https://cdn.example/v?itag=251", "http_headers": {}}
            ]}"#,
    );
    let plan = r.plan(Path::new("/s")).expect("a plan");
    assert_eq!(plan.produced, PathBuf::from("/s/Talk [abc].mp4"));
    assert_eq!(plan.streams.len(), 2);
    assert_eq!(
        plan.streams[0].target,
        PathBuf::from("/s/Talk [abc].f137.mp4")
    );
    assert_eq!(plan.streams[0].agent.as_deref(), Some("Agent/1"));
    assert_eq!(
        plan.streams[1].target,
        PathBuf::from("/s/Talk [abc].f251-drc.webm")
    );
    assert_eq!(plan.streams[1].agent, None);
}

#[test]
fn a_pages_only_video_is_named_without_a_number() {
    let one = read(
        r#"{"filename": "/s/QA clip (1) [watch-1].mp4", "ext": "mp4",
            "extractor_key": "HTML5MediaEmbed", "id": "watch-1",
            "format_id": "0", "protocol": "http", "url": "http://site/clip.mp4"}"#,
    );
    let plan = one.plan(Path::new("/s")).expect("a plan");
    assert_eq!(plan.produced, PathBuf::from("/s/QA clip [watch].mp4"));
    assert_eq!(plan.streams[0].target, plan.produced);
    // Two videos on the page: the numbers tell them apart and stay.
    let two = read(
        r#"{"filename": "/s/QA clip (1) [watch-1].mp4", "ext": "mp4",
            "extractor_key": "HTML5MediaEmbed", "id": "watch-1", "n_entries": 2,
            "format_id": "0", "protocol": "http", "url": "http://site/clip.mp4"}"#,
    );
    assert_eq!(
        two.plan(Path::new("/s")).unwrap().produced,
        PathBuf::from("/s/QA clip (1) [watch-1].mp4")
    );
    // Another site's title that ends in "(1)" is its own.
    let other = read(
        r#"{"filename": "/s/Part (1) [abc-1].mp4", "ext": "mp4",
            "extractor_key": "Vimeo", "id": "abc-1", "n_entries": 1,
            "format_id": "0", "protocol": "https", "url": "https://v/1.mp4"}"#,
    );
    assert_eq!(
        other.plan(Path::new("/s")).unwrap().produced,
        PathBuf::from("/s/Part (1) [abc-1].mp4")
    );
}

#[test]
fn a_numbered_name_is_matched_as_it_is_written() {
    let r = read(
        r#"{"filename": "/s/QA clip (1) [watch\uff1fv=ok-1].mp4", "ext": "mp4",
            "extractor_key": "HTML5MediaEmbed", "id": "watch?v=ok-1",
            "format_id": "0", "protocol": "http", "url": "http://site/clip.mp4"}"#,
    );
    assert_eq!(
        r.plan(Path::new("/s")).unwrap().produced,
        PathBuf::from("/s/QA clip [watch\u{ff1f}v=ok].mp4")
    );
}

#[test]
fn a_chunk_size_is_read_where_given_and_ignored_where_odd() {
    let r = read(
        r#"{"filename": "/s/a.mp4", "requested_formats": [
            {"format_id": "1", "protocol": "https", "url": "https://x/1",
             "downloader_options": {"http_chunk_size": 10485760}},
            {"format_id": "2", "protocol": "https", "url": "https://x/2",
             "downloader_options": {"http_chunk_size": "ten"}},
            {"format_id": "3", "protocol": "https", "url": "https://x/3",
             "downloader_options": {"http_chunk_size": 0}},
            {"format_id": "4", "protocol": "https", "url": "https://x/4"}]}"#,
    );
    let plan = r.plan(Path::new("/s")).expect("a plan");
    let chunks: Vec<_> = plan.streams.iter().map(|s| s.chunk).collect();
    assert_eq!(chunks, [Some(10_485_760), None, None, None]);
}

#[test]
fn a_type_dm_cannot_name_is_still_fetched_by_dm() {
    let r = read(
        r#"{"filename": "/s/file [file].unknown_video", "format_id": "0",
            "ext": "unknown_video", "protocol": "http", "url": "http://h/file.bin"}"#,
    );
    assert!(r.plan(Path::new("/s")).is_some());
}

#[test]
fn one_plain_format_is_fetched_under_the_name_itself() {
    let r = read(
        r#"{"filename": "/s/clip [clip].mp4", "format_id": "mp4", "ext": "mp4",
            "protocol": "http", "url": "http://127.0.0.1:9/clip.mp4"}"#,
    );
    let plan = r.plan(Path::new("/s")).expect("a plan");
    assert_eq!(plan.streams.len(), 1);
    assert_eq!(plan.streams[0].target, plan.produced);
}

#[test]
fn streams_in_pieces_live_ones_and_names_elsewhere_are_not_planned() {
    let pieces = read(
        r#"{"filename": "/s/a.mp4", "requested_formats": [
            {"format_id": "1", "protocol": "https", "url": "https://x/1"},
            {"format_id": "2", "protocol": "m3u8_native", "url": "https://x/2.m3u8"}]}"#,
    );
    assert_eq!(pieces.plan(Path::new("/s")), None);
    let live = read(
        r#"{"filename": "/s/a.mp4", "is_live": true, "protocol": "https", "url": "https://x/1"}"#,
    );
    assert_eq!(live.plan(Path::new("/s")), None);
    let elsewhere =
        read(r#"{"filename": "/etc/a.mp4", "protocol": "https", "url": "https://x/1"}"#);
    assert_eq!(elsewhere.plan(Path::new("/s")), None);
    let unnamed = read(r#"{"protocol": "https", "url": "https://x/1"}"#);
    assert_eq!(unnamed.plan(Path::new("/s")), None);
    let sabr = read(r#"{"filename": "/s/a.mp4", "protocol": "sabr", "url": "https://x/1"}"#);
    assert_eq!(sabr.plan(Path::new("/s")), None);
}

#[test]
fn progress_adds_up_and_has_a_total_once_every_stream_has_one() {
    let p = |downloaded, total, speed| crate::Progress {
        downloaded,
        total,
        speed,
        eta: None,
        estimated: false,
    };
    let one = sum(&[Some(p(100, Some(1000), 10.0)), None]);
    assert_eq!((one.downloaded, one.total), (100, None));
    let both = sum(&[Some(p(100, Some(1000), 10.0)), Some(p(50, Some(200), 40.0))]);
    assert_eq!(
        (both.downloaded, both.total, both.speed),
        (150, Some(1200), 50.0)
    );
    assert_eq!(both.eta, Some(std::time::Duration::from_secs(21)));
}

#[test]
fn an_agent_is_kept_once() {
    let a = interned("Agent/7 (test)");
    let b = interned(&String::from("Agent/7 (test)"));
    assert!(std::ptr::eq(a, b));
}
