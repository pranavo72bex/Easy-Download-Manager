// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

const PAGE: &str = "https://site.example/watch/42";

#[test]
fn json_ld_names_the_video_first() {
    let html = r#"<html><head><title>Site</title>
        <script type="application/ld+json">{"@context":"https://schema.org","@type":"VideoObject","name":"A talk","contentUrl":"https://cdn.example/talk.mp4"}</script>
        <meta property="og:video" content="https://cdn.example/other.mp4"></head></html>"#;
    let found = find(html, PAGE).expect("found");
    assert_eq!(found.media, "https://cdn.example/talk.mp4");
    assert_eq!(found.title.as_deref(), Some("A talk"));
    assert_eq!(file_name(&found), "A talk.mp4");
}

#[test]
fn open_graph_names_a_file_but_not_a_players_page() {
    let file = r#"<meta property="og:title" content="Clip &amp; more"><meta property="og:video:secure_url" content="https://cdn.example/c.mp4?sig=1&amp;x=2">"#;
    let found = find(file, PAGE).expect("found");
    assert_eq!(found.media, "https://cdn.example/c.mp4?sig=1&x=2");
    assert_eq!(found.title.as_deref(), Some("Clip & more"));
    let player = r#"<meta property="og:video" content="https://site.example/embed/42">"#;
    assert_eq!(find(player, PAGE), None);
    let typed = r#"<meta property="og:video" content="https://cdn.example/play/42"><meta property="og:video:type" content="video/mp4">"#;
    assert_eq!(
        find(typed, PAGE).map(|f| f.media),
        Some("https://cdn.example/play/42".to_string())
    );
}

#[test]
fn a_video_tag_and_its_sources_are_read_relative_to_the_page() {
    let html = r#"<video controls><source src="/media/clip.webm" type="video/webm"></video>"#;
    assert_eq!(
        find(html, PAGE).map(|f| f.media),
        Some("https://site.example/media/clip.webm".to_string())
    );
    let direct = r#"<video src="https://cdn.example/v.mp4" autoplay></video>"#;
    assert_eq!(
        find(direct, PAGE).map(|f| f.media),
        Some("https://cdn.example/v.mp4".to_string())
    );
    assert_eq!(
        find(r#"<video src="blob:https://site/1"></video>"#, PAGE),
        None
    );
}

#[test]
fn a_players_data_in_a_script_is_read_with_its_caption() {
    // As a short-video site's page holds it: escaped slashes, a caption under "desc".
    let html = r#"<title>Site - Make Your Day</title><script id="data" type="application/json">{"nav":{"title":"Company"},"item":{"id":"77","desc":"A cat  on a skateboard","video":{"playAddr":"https://v16.cdn.example/video/abc?a=1&b=2"}}}</script>"#;
    let found = find(html, PAGE).expect("found");
    assert_eq!(found.media, "https://v16.cdn.example/video/abc?a=1&b=2");
    assert_eq!(found.title.as_deref(), Some("A cat on a skateboard"));
    assert_eq!(file_name(&found), "A cat on a skateboard.mp4");
}

#[test]
fn a_quoted_media_address_is_the_last_resort() {
    let html = r#"<script>var player = setup({file: "https://cdn.example/hls/master.m3u8"});</script><title>News clip</title>"#;
    let found = find(html, PAGE).expect("found");
    assert_eq!(found.media, "https://cdn.example/hls/master.m3u8");
    assert_eq!(file_name(&found), "News clip.mp4");
}

#[test]
fn a_page_with_no_media_names_none() {
    assert_eq!(
        find("<html><title>Just text</title><p>hello</p></html>", PAGE),
        None
    );
    assert_eq!(
        find(r#"<a href="https://site.example/page.html">x</a>"#, PAGE),
        None
    );
    // Not a web address.
    assert_eq!(find(r#"{"playAddr":"javascript:alert(1)"}"#, PAGE), None);
}

#[test]
fn a_long_title_is_cut_and_marks_a_file_system_refuses_are_left_out() {
    let found = Found {
        media: "https://cdn.example/a.mp3".into(),
        title: Some(format!("{}/a:b*c?\nnext", "x".repeat(200))),
    };
    let name = file_name(&found);
    assert!(name.ends_with(".mp3"), "{name}");
    assert!(!name.contains(['/', ':', '*', '?', '\n']), "{name}");
    assert!(name.chars().count() <= 104, "{name}");
}

#[test]
fn the_cookies_set_make_a_cookie_header() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.append(
        reqwest::header::SET_COOKIE,
        "token=abc; Path=/; Secure".parse().unwrap(),
    );
    headers.append(
        reqwest::header::SET_COOKIE,
        "id=1%7Cx; Max-Age=1".parse().unwrap(),
    );
    assert_eq!(cookies_set(&headers), "token=abc; id=1%7Cx");
}
