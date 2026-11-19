// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn keys(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn the_stream_a_player_answer_names_is_found_under_its_key() {
    let body = r#"{
        "data": { "title": "A talk", "thumbnail": "https://img.example/t.jpg",
                  "playback": { "hlsUrl": "https:\/\/cdn.example\/v\/master.m3u8?sig=abc",
                                "dash": "//cdn.example/v/manifest.mpd" },
                  "other": "https://cdn.example/not-listed.mp4" } }"#;
    assert_eq!(
        addresses_in(body, &keys(&["hlsUrl", "dash"])),
        vec![
            "https://cdn.example/v/master.m3u8?sig=abc",
            "https://cdn.example/v/manifest.mpd"
        ]
    );
}

#[test]
fn with_no_keys_every_string_is_looked_at_and_only_streams_are_kept() {
    let body = r#"[ { "a": "https://cdn.example/v/one.mp4" },
                    { "b": ["https://cdn.example/v/two.webm", "https://cdn.example/page.html",
                            "see https://cdn.example/x.mp4", "ftp://cdn.example/f.mp4", 7, null] },
                    { "c": "https://cdn.example/v/one.mp4" } ]"#;
    assert_eq!(
        addresses_in(body, &[]),
        vec![
            "https://cdn.example/v/one.mp4",
            "https://cdn.example/v/two.webm"
        ]
    );
}

#[test]
fn a_whole_subtree_under_a_key_is_read() {
    let body = r#"{ "sources": { "hd": [ { "file": "https://cdn.example/hd.mp4" } ],
                                 "sd": "https://cdn.example/sd.mp4" },
                     "ads": { "file": "https://cdn.example/ad.mp4" } }"#;
    assert_eq!(
        addresses_in(body, &keys(&["sources"])),
        vec!["https://cdn.example/hd.mp4", "https://cdn.example/sd.mp4"]
    );
}

#[test]
fn what_is_never_caught_is_not_taken_from_an_answer() {
    let body =
        r#"{ "u": "https://ad.doubleclick.net/promo.mp4", "v": "https://cdn.example/ok.mp4" }"#;
    assert_eq!(addresses_in(body, &[]), vec!["https://cdn.example/ok.mp4"]);
}

#[test]
fn a_body_that_is_not_json_or_is_too_large_gives_nothing() {
    assert!(addresses_in("<html>https://cdn.example/a.mp4</html>", &[]).is_empty());
    assert!(addresses_in("var u = 'https://cdn.example/a.mp4';", &[]).is_empty());
    assert!(addresses_in("", &[]).is_empty());
    let big = format!(
        r#"{{ "u": "https://cdn.example/a.mp4", "pad": "{}" }}"#,
        "x".repeat(MAX_ANSWER_BYTES)
    );
    assert!(addresses_in(&big, &[]).is_empty());
}

#[test]
fn a_deep_or_wide_answer_is_cut_and_not_followed_forever() {
    let mut deep = String::from(r#""https://cdn.example/a.mp4""#);
    for _ in 0..100 {
        deep = format!("[{deep}]");
    }
    // serde_json refuses nesting past its own limit, and the walk stops at its own; either
    // way the address at the bottom is not found.
    assert!(addresses_in(&deep, &[]).is_empty());
    let many: Vec<String> = (0..200)
        .map(|n| format!(r#""https://cdn.example/{n}.mp4""#))
        .collect();
    let found = addresses_in(&format!("[{}]", many.join(",")), &[]);
    assert_eq!(found.len(), MOST_ADDRESSES);
}

#[test]
fn an_address_that_is_not_media_is_not_one() {
    for text in [
        "https://cdn.example/",
        "https://cdn.example/a.mp4x",
        "https://cdn.example/mp4",
        "/relative/a.mp4",
        "javascript:alert(1)//a.mp4",
        "data:video/mp4;base64,AAAA",
    ] {
        assert_eq!(stream_address(text), None, "{text}");
    }
    assert_eq!(
        stream_address(" https://cdn.example/a/b.M3U8 ").as_deref(),
        Some("https://cdn.example/a/b.M3U8")
    );
}

/// The rules dm ships: every site's answer rule is read as the code reads it, and a made-up
/// answer in each site's field names gives its stream (written from the sites' published
/// field names; none has been run against the live site).
#[test]
fn the_shipped_site_rules_find_the_streams_of_their_sites_answers() {
    let rules =
        dm_protocol::Rules::parse(include_str!("../../../../desktop/assets/rules/rules.json"))
            .expect("the shipped rules parse");
    let keys_of = |host: &str, request: &str| -> Vec<String> {
        let site = rules.sites.iter().find(|s| s.host == host).expect(host);
        site.answers
            .iter()
            .find(|a| request.contains(a.url_contains.as_str()))
            .unwrap_or_else(|| panic!("{host} has no rule for {request}"))
            .keys
            .clone()
    };
    let cases = [
        (
            "facebook.com",
            "/api/graphql/",
            r#"{"data":{"browser_native_hd_url":"https://video.fbcdn.net/v/hd.mp4?oh=1"}}"#,
            "https://video.fbcdn.net/v/hd.mp4?oh=1",
        ),
        (
            "instagram.com",
            "/api/v1/media/1/info/",
            r#"{"items":[{"video_versions":[{"url":"https://scontent.cdninstagram.com/v/a.mp4?x=1"}]}]}"#,
            "https://scontent.cdninstagram.com/v/a.mp4?x=1",
        ),
        (
            "vimeo.com",
            "/video/1/config",
            r#"{"request":{"files":{"progressive":[{"url":"https://vod.vimeocdn.com/p/720.mp4"}]}}}"#,
            "https://vod.vimeocdn.com/p/720.mp4",
        ),
        (
            "tiktok.com",
            "/api/item/detail/",
            r#"{"itemInfo":{"itemStruct":{"video":{"playAddr":"https://v16.tiktokcdn.com/v/a.mp4"}}}}"#,
            "https://v16.tiktokcdn.com/v/a.mp4",
        ),
        (
            "x.com",
            "/graphql/abc/TweetDetail".to_lowercase().leak(),
            r#"{"video_info":{"variants":[{"url":"https://video.twimg.com/v/1280.mp4"}]}}"#,
            "https://video.twimg.com/v/1280.mp4",
        ),
        (
            "reddit.com",
            "/r/a/comments/1/x/",
            r#"{"reddit_video":{"fallback_url":"https://v.redd.it/id/DASH_1080.mp4","hls_url":"https://v.redd.it/id/HLSPlaylist.m3u8"}}"#,
            "https://v.redd.it/id/DASH_1080.mp4",
        ),
    ];
    for (host, request, body, expected) in cases {
        let found = addresses_in(body, &keys_of(host, request));
        assert!(found.iter().any(|a| a == expected), "{host}: {found:?}");
    }
}

/// A player config shaped as Vimeo's: the stream from two CDNs, five files, its own video's id.
fn vimeo_config(id: u64) -> String {
    format!(
        r#"{{ "video": {{ "id": {id}, "title": "A film" }},
              "request": {{ "files": {{
                "hls": {{ "default_cdn": "a", "cdns": {{
                  "a": {{ "url": "https://a.example/{id}/playlist.m3u8", "origin": "gcs" }},
                  "b": {{ "url": "https://b.example/{id}/playlist.m3u8", "origin": "gcs" }} }} }},
                "progressive": [
                  {{ "url": "https://f.example/{id}/1080.mp4", "quality": "1080p" }},
                  {{ "url": "https://f.example/{id}/720.mp4", "quality": "720p" }} ] }} }} }}"#
    )
}

fn vimeo_rule() -> dm_protocol::AnswerRule {
    dm_protocol::AnswerRule {
        url_contains: "/config".into(),
        keys: keys(&["url"]),
        video_id: "video.id".into(),
        first_of: keys(&["cdns"]),
    }
}

#[test]
fn a_config_for_the_pages_video_lists_its_stream_once_and_every_file() {
    // gaps row 214: one HLS line, not one per CDN; the files all.
    let page = "https://vimeo.com/513177164";
    assert_eq!(
        addresses_by(&vimeo_config(513177164), &vimeo_rule(), page),
        vec![
            "https://a.example/513177164/playlist.m3u8",
            "https://f.example/513177164/1080.mp4",
            "https://f.example/513177164/720.mp4",
        ]
    );
    // The player in its own frame names the video the same way.
    assert_eq!(
        addresses_by(
            &vimeo_config(513177164),
            &vimeo_rule(),
            "https://player.vimeo.com/video/513177164?h=1"
        )
        .len(),
        3
    );
}

#[test]
fn a_config_for_another_video_than_the_pages_gives_nothing() {
    // The page's background clip has a player config of its own (gaps row 214).
    let page = "https://vimeo.com/513177164";
    assert!(addresses_by(&vimeo_config(42424242), &vimeo_rule(), page).is_empty());
}

#[test]
fn where_the_page_or_the_answer_names_no_video_the_answer_is_read_as_before() {
    // A page whose address names no video by number: a channel, a showcase, the home page.
    for page in [
        "https://vimeo.com/channels/staffpicks",
        "https://vimeo.com/",
    ] {
        assert_eq!(
            addresses_by(&vimeo_config(42424242), &vimeo_rule(), page).len(),
            3,
            "{page}"
        );
    }
    // An answer without the id's path; a rule with no id path or no first_of.
    let without_id = r#"{ "cdns": { "a": { "url": "https://a.example/p.m3u8" },
                                    "b": { "url": "https://b.example/p.m3u8" } } }"#;
    assert_eq!(
        addresses_by(without_id, &vimeo_rule(), "https://vimeo.com/513177164").len(),
        1
    );
    let plain = dm_protocol::AnswerRule {
        keys: keys(&["url"]),
        ..Default::default()
    };
    assert_eq!(
        addresses_by(
            &vimeo_config(42424242),
            &plain,
            "https://vimeo.com/513177164"
        )
        .len(),
        4
    );
    // first_of on an array: its first item only.
    let listed = r#"{ "cdns": [ { "url": "https://a.example/p.m3u8" }, { "url": "https://b.example/p.m3u8" } ] }"#;
    assert_eq!(
        addresses_by(listed, &vimeo_rule(), ""),
        vec!["https://a.example/p.m3u8"]
    );
}
