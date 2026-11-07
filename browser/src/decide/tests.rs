// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#[test]
fn a_clicked_magnet_is_taken_and_named_by_its_dn() {
    use super::{magnet_name, magnet_url, PanelClick, MAGNET_MESSAGE};
    let click = |src: &str| PanelClick {
        kind: MAGNET_MESSAGE.into(),
        src: src.into(),
        ..PanelClick::default()
    };
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Some+Linux%20ISO";
    assert_eq!(magnet_url(&click(magnet)).as_deref(), Ok(magnet));
    assert!(magnet_url(&click("magnet:?dn=nothing")).is_err());
    assert!(magnet_url(&click("https://e.com/x.torrent")).is_err());
    assert_eq!(magnet_name(magnet), "Some Linux ISO");
    assert_eq!(magnet_name("magnet:?xt=urn:btih:00"), "the torrent");
}

use super::taken_name;

#[test]
fn what_was_taken_is_named_by_its_file() {
    assert_eq!(
        taken_name("/home/a/Downloads/big.zip", "https://e.com/x"),
        "big.zip"
    );
    assert_eq!(taken_name("C:\\Users\\a\\big.zip", ""), "big.zip");
    assert_eq!(
        taken_name("", "https://e.com/files/report.pdf?x=1"),
        "report.pdf"
    );
    assert_eq!(taken_name("", "https://e.com/"), "e.com");
}

use super::*;
use serde_json::json;

#[test]
fn the_plain_page_script_answers_by_the_names_the_background_uses() {
    // Alt-click and the page's links are plain JavaScript, so a page
    // that forbids WebAssembly keeps them; the names must still agree.
    let script = include_str!("../../../extension/content.js");
    assert!(script.contains(&format!("'{SKIP_MESSAGE}'")));
    assert!(script.contains(&format!("'{TAKE_MESSAGE}'")));
    assert!(script.contains(&format!("'{LINKS_MESSAGE}'")));
    // The keys are read from the rules as the protocol names them.
    let rules = serde_json::to_value(dm_protocol::CatchRules::default()).unwrap();
    for field in ["keepKey", "takeKey"] {
        assert!(rules.get(field).is_some(), "{field}");
        assert!(script.contains(&format!("rules.{field}")), "{field}");
    }
    // Registered before the WebAssembly is asked for, not after.
    let load = script.find("'dm.panel-load'").unwrap();
    assert!(script.find("addEventListener('click'").unwrap() < load);
    assert!(script.find("onMessage.addListener").unwrap() < load);
}

#[test]
fn the_panel_is_put_in_by_the_browser_and_fetched_from_no_page() {
    // The background answers the page script's request by injecting the
    // panel's file; the page itself never loads anything of the extension's
    // (Brave refused that on YouTube), so no manifest offers a file to pages.
    let content = include_str!("../../../extension/content.js");
    let background = include_str!("../../../extension/background.js");
    assert!(!content.contains("import("));
    assert!(!content.contains("getURL('pkg/"));
    assert!(background.contains("'dm.panel-load'"));
    assert!(background.contains("files: ['pkg/panel.js']"));
    for manifest in [
        include_str!("../../../extension/manifest.json"),
        include_str!("../../../extension/manifest.firefox.json"),
    ] {
        assert!(!manifest.contains("web_accessible_resources"));
        assert!(manifest.contains("\"scripting\""));
    }
    // The loader's name for the panel's code is not the name the panel
    // keeps to start once: sharing it, the panel never started.
    let package = include_str!("../../../scripts/package.sh");
    assert!(package.contains("var __dmPanelCode;"));
    // Quiet where WebAssembly is forbidden: a rejection nobody handled yet
    // was logged as an error on the page (qa browser check).
    assert!(package.contains("__dmPanelCode.catch(() => {});"));
    assert!(content.contains("globalThis.__dmPanelCode"));
    let start = include_str!("../content/handoff.rs");
    assert!(start.contains("\"__dmPanel\".into()"));
}

#[test]
fn chromes_file_name_question_is_held_until_dm_has_decided() {
    let mut holds: Holds<&str> = Holds::default();
    // Asked first, decided after: held, then given.
    assert_eq!(holds.ask(1, "one"), None);
    assert_eq!(holds.decide(1), Some("one"));
    assert_eq!(holds.decide(1), None, "given once");
    // Decided first (left with the browser): the question that follows
    // is answered at once, and only that once.
    holds.decide(2);
    assert_eq!(holds.ask(2, "two"), Some("two"));
    assert_eq!(holds.ask(2, "again"), None);
    assert_eq!(holds.expire(2), Some("again"));
    // Taken by dm: the answer is dropped, so no Save dialog can follow.
    assert_eq!(holds.ask(3, "three"), None);
    holds.drop_taken(3);
    assert_eq!(holds.decide(3), None);
    assert_eq!(holds.expire(3), None);
    // A hold nobody decides runs out, and is answered then.
    assert_eq!(holds.ask(4, "four"), None);
    assert_eq!(holds.expire(4), Some("four"));
    // Decisions no question follows are not kept for ever.
    for id in 100..300 {
        holds.decide(id);
    }
    assert!(holds.decided.len() <= DECIDED_KEPT);
}

#[test]
fn a_page_open_before_dm_gets_the_links_reader_where_the_browser_allows() {
    assert!(injectable("https://e.com/files/"));
    assert!(injectable("http://127.0.0.1:8000/odd.html"));
    assert!(injectable("file:///home/u/list.html"));
    for closed in [
        "chrome://newtab/",
        "about:blank",
        "edge://settings",
        "chrome-extension://abc/popup.html",
        "moz-extension://abc/popup.html",
        "view-source:https://e.com/",
        "https://chromewebstore.google.com/detail/x",
        "https://chrome.google.com/webstore/detail/x",
        "https://addons.mozilla.org/firefox/addon/x",
        "",
    ] {
        assert!(!injectable(closed), "{closed}");
    }
}

#[test]
fn an_ftp_link_is_handed_over_from_the_menu() {
    assert!(handable("ftp://ftp.gnu.org/gnu/bash/bash-5.2.tar.gz"));
    assert!(!handable("ftps://e.com/f") && !handable("file:///etc/passwd"));
}

#[test]
fn a_sites_cookies_go_with_its_downloads_only_where_switched_on() {
    let sites = with_site(&[], "files.example", true);
    assert!(cookies_wanted(&sites, "https://dl.files.example/a.zip"));
    assert!(!cookies_wanted(&sites, "https://other.example/a.zip"));
    assert!(!cookies_wanted(&[], "https://files.example/a.zip"));
    // A video page's session goes with its video, for that one alone; a
    // listed site's is kept; anything else sends none.
    let video = "https://www.video.example/watch?v=1";
    assert_eq!(cookies_sent(&[], video, true, false), Some(true));
    assert_eq!(
        cookies_sent(&[], "https://other.example/a.zip", false, false),
        None
    );
    assert_eq!(
        cookies_sent(&sites, "https://dl.files.example/a.zip", false, false),
        Some(false)
    );
    assert_eq!(
        cookies_sent(&sites, "https://files.example/watch", true, false),
        Some(false)
    );
    // Every site, where the app says so: for that download alone; a
    // listed site's is still kept.
    assert_eq!(
        cookies_sent(&[], "https://other.example/a.zip", false, true),
        Some(true)
    );
    assert_eq!(
        cookies_sent(&sites, "https://dl.files.example/a.zip", false, true),
        Some(false)
    );
    let header = cookie_header(&[
        BrowserCookie {
            name: "session".into(),
            value: "abc".into(),
        },
        BrowserCookie {
            name: String::new(),
            value: "dropped".into(),
        },
        BrowserCookie {
            name: "theme".into(),
            value: "dark\r\nX-Injected: 1".into(),
        },
    ]);
    assert_eq!(header, "session=abc; theme=darkX-Injected: 1");
    assert_eq!(cookie_header(&[]), "");
}

#[test]
fn a_switch_turned_off_on_part_of_a_listed_site_is_off_afterwards() {
    // "Never on youtube.com" from the panel, then the popup on
    // music.youtube.com: the panel switch reads off; turned on, the
    // entry that kept it off goes, and it is said.
    let off = with_site(&[], "youtube.com", true);
    assert!(panel_off(&off, "https://music.youtube.com/watch"));
    let note = wider_note(&off, "music.youtube.com");
    let back = with_site(&off, "music.youtube.com", false);
    assert!(!panel_off(&back, "https://music.youtube.com/watch"));
    assert!(note.is_some_and(|n| n.contains("youtube.com")));
    // The same for the other two switches, whose lists read "on".
    let all = with_site(&["other.org".into()], "e.com", true);
    let all = with_site(&all, "dl.e.com", true);
    assert!(forced(&all, "https://a.dl.e.com/f"));
    let left = with_site(&all, "a.dl.e.com", false);
    assert!(!forced(&left, "https://a.dl.e.com/f"));
    assert_eq!(left, vec!["other.org"], "only what covered it went");
    let sites = with_site(&[], "e.com", true);
    assert!(!cookies_wanted(
        &with_site(&sites, "m.e.com", false),
        "https://m.e.com/f"
    ));
    // Its own entry alone is not news, and a site beside it is kept.
    assert_eq!(wider_note(&with_site(&[], "e.com", true), "e.com"), None);
    assert_eq!(
        covering(&["notme.com".into()], "me.com"),
        Vec::<String>::new()
    );
}

#[test]
fn a_containers_download_is_asked_its_own_containers_cookies() {
    let item: DownloadItem = serde_json::from_value(json!({
        "id": 4, "url": "https://e.com/f.zip", "state": "in_progress",
        "cookieStoreId": "firefox-container-2",
    }))
    .unwrap();
    let found = first_look(&item, "x").unwrap();
    assert_eq!(found.store.as_deref(), Some("firefox-container-2"));
    assert_eq!(
        cookie_query(&found.url, found.store.as_deref()),
        json!({ "url": "https://e.com/f.zip", "storeId": "firefox-container-2" })
    );
    // Chrome says none: the default store, not a store named "".
    assert_eq!(
        cookie_query("https://e.com/f", Some("")),
        json!({ "url": "https://e.com/f" })
    );
    assert_eq!(
        cookie_query("https://e.com/f", None),
        json!({ "url": "https://e.com/f" })
    );
}

#[test]
fn a_site_whose_every_download_is_taken_covers_its_parts_and_nothing_else() {
    let all = with_site(&[], "files.example", true);
    assert!(forced(&all, "https://files.example/any.weird?x=1"));
    assert!(forced(&all, "http://dl.files.example:8080/a"));
    assert!(!forced(&all, "https://notfiles.example/a"));
    assert!(!forced(&all, "https://example/a"));
    assert!(!forced(&[], "https://files.example/a"));
    // Taken off again.
    assert!(!forced(
        &with_site(&all, "files.example", false),
        "https://files.example/a"
    ));
}

#[test]
fn the_panel_is_kept_off_a_site_and_its_parts_and_let_back() {
    assert_eq!(
        site_of("https://www.Example.com:8443/watch?v=1").as_deref(),
        Some("example.com")
    );
    assert_eq!(
        site_of(&format!(
            "http://user:{}@m.e.com/x",
            super::test_secrets::secret("password")
        ))
        .as_deref(),
        Some("m.e.com")
    );
    assert_eq!(site_of("file:///tmp/a.html"), None);
    let off = with_site(&[], "e.com", true);
    assert!(panel_off(&off, "https://e.com/v"));
    assert!(
        panel_off(&off, "https://m.e.com/v"),
        "a part of the site is the site"
    );
    assert!(
        !panel_off(&off, "https://notme.com/v"),
        "only the site itself"
    );
    assert!(!panel_off(&off, "https://e.com.evil.net/v"));
    let back = with_site(&off, "e.com", false);
    assert!(!panel_off(&back, "https://e.com/v"));
    assert_eq!(
        with_site(&with_site(&off, "e.com", true), "a.com", true),
        vec!["a.com", "e.com"]
    );
}

fn seen(url: &str, kind: &str, headers: &[(&str, &str)]) -> Seen {
    Seen {
        url: url.into(),
        tab_id: 7,
        kind: kind.into(),
        status_code: 200,
        response_headers: headers
            .iter()
            .map(|(n, v)| Header {
                name: (*n).into(),
                value: Some((*v).into()),
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn a_pages_manifests_and_whole_media_files_are_streams() {
    let hls = stream_seen(&seen(
        "https://cdn.e.com/live/master.m3u8?token=1",
        "xmlhttprequest",
        &[("Content-Type", "application/vnd.apple.mpegurl")],
    ))
    .unwrap();
    assert_eq!(
        (hls.kind, hls.label.as_str()),
        (StreamKind::Hls, "HLS stream")
    );
    // Named by its address alone, whatever the server calls it.
    assert_eq!(
        stream_seen(&seen(
            "https://e.com/v/index.m3u8",
            "other",
            &[("Content-Type", "text/plain")]
        ))
        .map(|s| s.kind),
        Some(StreamKind::Hls)
    );
    let dash = stream_seen(&seen(
        "https://e.com/manifest",
        "xmlhttprequest",
        &[("content-type", "application/dash+xml")],
    ))
    .unwrap();
    assert_eq!(dash.kind, StreamKind::Dash);

    let file = stream_seen(&seen(
        "https://e.com/media/talk.mp4",
        "media",
        &[
            ("Content-Type", "video/mp4"),
            ("Content-Range", "bytes 0-1048575/52428800"),
        ],
    ))
    .unwrap();
    assert_eq!(file.kind, StreamKind::File);
    assert_eq!(file.size, Some(52_428_800));
    assert_eq!(file.label, "MP4 video file");
    let audio = stream_seen(&seen(
        "https://e.com/ep1",
        "media",
        &[
            ("Content-Type", "audio/mpeg"),
            ("Content-Length", "9000000"),
        ],
    ))
    .unwrap();
    assert_eq!(audio.label, "MPEG audio file");
}

#[test]
fn pieces_of_something_else_are_not_streams() {
    let none =
        |url: &str, headers: &[(&str, &str)]| stream_seen(&seen(url, "media", headers)).is_none();
    // Segments of a stream.
    assert!(none(
        "https://e.com/seg-12.ts",
        &[("Content-Type", "video/mp2t")]
    ));
    assert!(none(
        "https://e.com/chunk-3.m4s",
        &[("Content-Type", "video/mp4")]
    ));
    // A range further into a file, and a player's numbered pieces.
    assert!(none(
        "https://e.com/v.mp4",
        &[
            ("Content-Type", "video/mp4"),
            ("Content-Range", "bytes 1048576-2097151/52428800")
        ]
    ));
    assert!(none(
        "https://rr1.googlevideo.com/videoplayback?itag=18&range=0-100000",
        &[("Content-Type", "video/mp4")]
    ));
    // Too small to be what a person came for.
    assert!(none(
        "https://e.com/click.mp3",
        &[("Content-Type", "audio/mpeg"), ("Content-Length", "20000")]
    ));
    // Not media, not a web address, not a success.
    assert!(none(
        "https://e.com/app.js",
        &[("Content-Type", "text/javascript")]
    ));
    assert!(none(
        "blob:https://e.com/1",
        &[("Content-Type", "video/mp4")]
    ));
    let mut refused = seen(
        "https://e.com/v.mp4",
        "media",
        &[("Content-Type", "video/mp4")],
    );
    refused.status_code = 403;
    assert!(stream_seen(&refused).is_none());
}

/// A response from a social site's CDN: a piece, as the player asked for it.
fn cdn_piece(host: &str, id: &str, efg: &str, from: u64, to: u64, mime: &str) -> Seen {
    seen(
        &format!(
            "https://{host}/o1/v/t2/f2/{id}.mp4?_nc_cat=1&efg={efg}&oh=sig&bytestart={from}&byteend={to}"
        ),
        "media",
        &[("Content-Type", mime), ("Content-Length", &(to - from + 1).to_string())],
    )
}

#[test]
fn a_social_sites_pieces_are_one_picture_and_one_sound_each_whole() {
    const VIDEO: &str =
        "eyJ2ZW5jb2RlX3RhZyI6ICJpZy14cHZkcy5jbGlwcy5pZ3d3dy1DMy5kYXNoX2Jhc2VsaW5lXzFfdjEifQ";
    const AUDIO: &str = "eyJ2ZW5jb2RlX3RhZyI6ICJpZy14cHZkcy5jbGlwcy5pZ3d3dy1DMy5kYXNoX2xuX2hlYWFjX3ZicjNfYXVkaW8ifQ";
    let mut streams = Streams::default();
    // The player asks for each file in pieces, picture and sound apart, each
    // piece far under the size of anything worth offering.
    for (from, to) in [(0, 891), (892, 1031), (1032, 296_383), (296_384, 1_002_715)] {
        streams.absorb(&cdn_piece(
            "instagram.fktm18-1.fna.fbcdn.net",
            "AQvid",
            VIDEO,
            from,
            to,
            "video/mp4",
        ));
    }
    for (from, to) in [(0, 823), (824, 1107), (1108, 20_467)] {
        streams.absorb(&cdn_piece(
            "instagram.fktm18-1.fna.fbcdn.net",
            "AQaud",
            AUDIO,
            from,
            to,
            "video/mp4",
        ));
    }
    let list = streams.of(7);
    assert_eq!(list.len(), 1, "{list:?}");
    let line = &list[0];
    assert_eq!(line.label, "video + sound");
    // The files are asked for whole: the piece's two parameters are gone,
    // the rest (which carry the signature) is kept.
    assert_eq!(
        line.url,
        format!("https://instagram.fktm18-1.fna.fbcdn.net/o1/v/t2/f2/AQvid.mp4?_nc_cat=1&efg={VIDEO}&oh=sig")
    );
    assert_eq!(
        line.sound.as_deref(),
        Some(format!("https://instagram.fktm18-1.fna.fbcdn.net/o1/v/t2/f2/AQaud.mp4?_nc_cat=1&efg={AUDIO}&oh=sig").as_str())
    );
}

#[test]
fn a_social_sites_picture_says_its_height_and_a_missing_tag_falls_back_to_the_type() {
    const FB: &str = "eyJ2ZW5jb2RlX3RhZyI6ICJkYXNoX2gyNjQtYmFzaWMtZ2VuMl83MjBwIn0";
    let picture = stream_seen(&cdn_piece(
        "video-sjc.xx.fbcdn.net",
        "v",
        FB,
        0,
        99,
        "video/mp4",
    ))
    .unwrap();
    assert_eq!(
        (picture.kind, picture.label.as_str()),
        (StreamKind::Picture, "720p video")
    );
    // No tag: what the server called the piece decides.
    let no_tag = |mime: &str| {
        stream_seen(&seen(
            "https://scontent.cdninstagram.com/v/x.mp4?bytestart=0&byteend=99",
            "media",
            &[("Content-Type", mime)],
        ))
        .unwrap()
        .kind
    };
    assert_eq!(no_tag("audio/mp4"), StreamKind::Sound);
    assert_eq!(no_tag("video/mp4"), StreamKind::Picture);
    // Only the social CDNs name their pieces this way.
    assert!(pieces_of(
        "https://notfbcdn.net/v.mp4?bytestart=0&byteend=9",
        "video/mp4"
    )
    .is_none());
    assert!(pieces_of("https://a.fbcdn.net/v.mp4?x=1", "video/mp4").is_none());
}

#[test]
fn a_clip_the_browser_plays_is_offered_under_the_floor_a_script_fetch_is_not() {
    // X's GIF-style clips are ~100 KB: the post's own media.
    let clip = |kind: &str| {
        stream_seen(&seen(
            "https://video.twimg.com/tweet_video/Gi.mp4",
            kind,
            &[
                ("Content-Type", "video/mp4"),
                ("Content-Range", "bytes 0-80763/103807"),
            ],
        ))
    };
    assert_eq!(clip("media").map(|s| s.size), Some(Some(103_807)));
    assert!(clip("xmlhttprequest").is_none());
}

#[test]
fn a_playlists_address_names_its_height_and_sound_alone_is_not_offered_beside_a_picture() {
    let playlist = |path: &str| {
        seen(
            &format!("https://video.e.com{path}?tag=1"),
            "xmlhttprequest",
            &[("Content-Type", "application/x-mpegURL")],
        )
    };
    let mut streams = Streams::default();
    for path in [
        "/pl/Ab12.m3u8",
        "/pl/avc1/1280x720/Ab12.m3u8",
        "/pl/avc1/640x360/Ab12.m3u8",
        "/pl/mp4a/128000/Ab12.m3u8",
    ] {
        streams.absorb(&playlist(path));
    }
    let labels: Vec<String> = streams.of(7).into_iter().map(|s| s.label).collect();
    assert_eq!(
        labels,
        ["HLS stream", "720p HLS stream", "360p HLS stream"],
        "the master first; no line that would save a sound alone"
    );
    // A tab with only a sound playlist still offers it.
    let mut only = Streams::default();
    only.absorb(&playlist("/pl/mp4a/128000/Ab12.m3u8"));
    assert_eq!(only.of(7).len(), 1);
}

#[test]
fn a_tab_keeps_its_streams_until_it_loads_another_page() {
    let mut streams = Streams::default();
    let manifest = seen("https://e.com/a/master.m3u8", "xmlhttprequest", &[]);
    streams.absorb(&manifest);
    streams.absorb(&manifest);
    streams.absorb(&seen("https://e.com/b/other.m3u8", "xmlhttprequest", &[]));
    streams.absorb(&seen(
        "https://e.com/v.mp4",
        "media",
        &[("Content-Type", "video/mp4"), ("Content-Length", "9000000")],
    ));
    let listed = streams.of(7);
    assert_eq!(listed.len(), 3, "the same address twice is one stream");
    assert_eq!(
        listed[0].label, "HLS stream · master.m3u8",
        "two of a kind are told apart"
    );
    assert_eq!(listed[2].kind, StreamKind::File, "manifests first");
    assert!(streams.of(8).is_empty(), "another tab's are its own");

    streams.absorb(&seen("https://e.com/next", "main_frame", &[]));
    assert!(streams.of(7).is_empty(), "a new page forgets the old one's");

    for n in 0..20 {
        streams.absorb(&seen(
            &format!("https://e.com/{n}.m3u8"),
            "xmlhttprequest",
            &[],
        ));
    }
    let kept = streams.of(7);
    assert_eq!(kept.len(), STREAMS_PER_TAB);
    assert!(
        kept.iter().any(|s| s.url.ends_with("/0.m3u8")),
        "the first is kept"
    );
    assert!(
        kept.iter().any(|s| s.url.ends_with("/19.m3u8")),
        "and the latest"
    );
}

/// A master playlist shaped like Vimeo's: renditions and a sound track on
/// addresses relative to the playlist's own.
const VIMEO_MASTER: &str = r#"#EXTM3U
#EXT-X-INDEPENDENT-SEGMENTS
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio-high",NAME="Original",LANGUAGE="en",DEFAULT=YES,URI="../../a/audio-high/playlist.m3u8?pathsig=8c"
#EXT-X-STREAM-INF:BANDWIDTH=3500000,AVERAGE-BANDWIDTH=3200000,RESOLUTION=1920x1080,FRAME-RATE=29.970,CODECS="avc1.640028,mp4a.40.2",AUDIO="audio-high"
../../v/1080p/playlist.m3u8?pathsig=8c
#EXT-X-STREAM-INF:BANDWIDTH=1800000,RESOLUTION=1280x720,FRAME-RATE=29.970,CODECS="avc1.64001f,mp4a.40.2",AUDIO="audio-high"
../../v/720p/playlist.m3u8?pathsig=8c
#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,CODECS="avc1.4d401e,mp4a.40.2",AUDIO="audio-high"
../../v/360p/playlist.m3u8?pathsig=8c
"#;

const VIMEO_URL: &str = "https://vod.example.net/p/x/y/playlist.m3u8?pathsig=9";

/// One shaped like TikTok's: portrait pictures on absolute addresses, and a
/// rendition that is sound alone.
const TIKTOK_MASTER: &str = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1400000,RESOLUTION=576x1024,CODECS="avc1.4d401f,mp4a.40.2"
https://v16.example-cdn.com/a/576.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2600000,RESOLUTION=720x1280,CODECS="avc1.64001f,mp4a.40.2"
https://v16.example-cdn.com/a/720.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS="mp4a.40.2"
https://v16.example-cdn.com/a/audio.m3u8
"#;

#[test]
fn a_master_playlist_is_named_by_the_best_picture_it_lists() {
    let renditions = hls_renditions(VIMEO_URL, VIMEO_MASTER);
    assert_eq!(renditions.len(), 4, "three pictures and a sound track");
    assert_eq!(
        renditions[1].url, "https://vod.example.net/p/v/1080p/playlist.m3u8?pathsig=8c",
        "relative addresses are made absolute against the playlist's own"
    );
    assert_eq!(
        (
            renditions[1].height,
            renditions[1].frame_rate,
            renditions[1].bandwidth
        ),
        (Some(1080), Some(30), Some(3_200_000)),
        "the average bandwidth where it is said"
    );
    assert!(renditions[0].sound_only, "the EXT-X-MEDIA sound track");
    assert_eq!(
        hls_master_label(&renditions).as_deref(),
        Some("1080p HLS stream")
    );

    // A portrait clip is named by its smaller side, as the address rule does.
    let tiktok = hls_renditions("https://v16.example-cdn.com/a/master.m3u8", TIKTOK_MASTER);
    assert_eq!(tiktok.len(), 3);
    assert!(tiktok[2].sound_only, "no picture and only an AAC codec");
    assert_eq!(
        hls_master_label(&tiktok).as_deref(),
        Some("720p HLS stream")
    );
}

#[test]
fn a_playlist_says_its_frame_rate_bandwidth_or_that_it_is_sound() {
    let master = |inf: &str| format!("#EXTM3U\n{inf}\nhttps://e.com/x.m3u8\n");
    let label = |text: &str| hls_master_label(&hls_renditions("https://e.com/m.m3u8", text));
    // 50 frames a second or more is part of the name; 30 is not.
    let both = format!(
        "{}{}",
        master("#EXT-X-STREAM-INF:BANDWIDTH=6000000,RESOLUTION=1920x1080,FRAME-RATE=60.000"),
        master("#EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1920x1080,FRAME-RATE=30.000"),
    );
    assert_eq!(label(&both).as_deref(), Some("1080p60 HLS stream"));
    // No resolution: the bandwidth names it.
    assert_eq!(
        label(&master(
            "#EXT-X-STREAM-INF:BANDWIDTH=2500000,CODECS=\"avc1.4d401e,mp4a.40.2\""
        ))
        .as_deref(),
        Some("2.5 Mbps HLS stream")
    );
    // Nothing but sound.
    assert_eq!(
        label(&master(
            "#EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS=\"mp4a.40.2\""
        ))
        .as_deref(),
        Some("HLS sound")
    );
    // A media playlist lists no renditions: nothing to name it by.
    let media = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts\n#EXTINF:6.0,\nseg1.ts\n#EXT-X-ENDLIST\n";
    assert!(hls_renditions("https://e.com/media.m3u8", media).is_empty());
    assert_eq!(label(media), None);
    // Tags in any case, and a quoted comma, do not confuse the reading.
    assert_eq!(
        label("#ext-x-stream-inf:CODECS=\"avc1.4d401e,mp4a.40.2\",RESOLUTION=1280x720,BANDWIDTH=1\nv.m3u8\n")
            .as_deref(),
        Some("720p HLS stream")
    );
}

#[test]
fn a_dash_manifest_is_named_by_its_tallest_picture() {
    let mpd = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static"><Period>
<AdaptationSet contentType="video" mimeType="video/mp4">
<Representation id="v1" bandwidth="800000" width="640" height="360" codecs="avc1.4d401e"/>
<Representation id="v2" bandwidth="3000000" width="1920" height="1080" codecs="avc1.640028"/>
</AdaptationSet>
<AdaptationSet contentType="audio" mimeType="audio/mp4">
<Representation id="a1" bandwidth="128000" codecs="mp4a.40.2"/>
</AdaptationSet></Period></MPD>"#;
    assert_eq!(dash_label(mpd).as_deref(), Some("1080p DASH stream"));
    let sound = r#"<MPD><Period><AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="96000" codecs="mp4a.40.2"/></AdaptationSet></Period></MPD>"#;
    assert_eq!(dash_label(sound).as_deref(), Some("DASH sound"));
    assert_eq!(dash_label("<MPD><Period/></MPD>"), None);
    assert_eq!(dash_label("not a manifest"), None);
}

fn playlist_seen(url: &str) -> Seen {
    seen(
        url,
        "xmlhttprequest",
        &[("Content-Type", "application/vnd.apple.mpegurl")],
    )
}

#[test]
fn a_read_master_playlist_is_the_line_and_its_renditions_are_not() {
    let variant = "https://vod.example.net/p/v/720p/playlist.m3u8?pathsig=8c";
    let mut streams = Streams::default();
    // The player fetched the master, then the rendition it chose.
    streams.absorb(&playlist_seen(VIMEO_URL));
    streams.absorb(&playlist_seen(variant));
    assert_eq!(streams.of(7).len(), 2, "before it is read: a line each");

    assert!(streams.wants_description(7, VIMEO_URL));
    assert!(
        !streams.wants_description(7, VIMEO_URL),
        "once: a player that fetches it again does not have it read again"
    );
    assert!(streams.describe(7, VIMEO_URL, VIMEO_MASTER));
    let listed = streams.of(7);
    assert_eq!(listed.len(), 1, "the rendition is no line of its own");
    assert_eq!(listed[0].label, "1080p HLS stream");
    assert_eq!(listed[0].url, VIMEO_URL);

    // A rendition heard after the master is not a line either.
    streams.absorb(&playlist_seen(
        "https://vod.example.net/p/v/360p/playlist.m3u8?pathsig=8c",
    ));
    streams.absorb(&playlist_seen(
        "https://vod.example.net/p/a/audio-high/playlist.m3u8?pathsig=8c",
    ));
    assert_eq!(streams.of(7).len(), 1);

    // What is kept for a sleeping worker keeps it so, in either order of
    // hearing the two.
    let kept = streams.raw(7);
    let mut woken = Streams::default();
    woken.absorb(&playlist_seen(variant));
    woken.restore(7, kept);
    let listed = woken.of(7);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].label, "1080p HLS stream");

    // A line already named by its address, or a media playlist, is not read
    // or changed.
    let mut other = Streams::default();
    other.absorb(&playlist_seen("https://e.com/pl/avc1/1280x720/a.m3u8"));
    assert!(!other.wants_description(7, "https://e.com/pl/avc1/1280x720/a.m3u8"));
    other.absorb(&playlist_seen("https://e.com/media.m3u8"));
    assert!(other.wants_description(7, "https://e.com/media.m3u8"));
    assert!(!other.describe(7, "https://e.com/media.m3u8", "#EXTM3U\n#EXTINF:6,\ns.ts\n"));
    assert!(other.of(7).iter().any(|s| s.label == "HLS stream"));

    // A new page forgets what was asked, with the streams.
    streams.forget(7);
    streams.absorb(&playlist_seen(VIMEO_URL));
    assert!(streams.wants_description(7, VIMEO_URL));
}

#[test]
fn a_dash_manifest_line_takes_its_name_from_the_manifest() {
    let mut streams = Streams::default();
    let manifest = seen(
        "https://cdn.e.com/v/manifest.mpd",
        "xmlhttprequest",
        &[("Content-Type", "application/dash+xml")],
    );
    streams.absorb(&manifest);
    assert!(streams.wants_description(7, "https://cdn.e.com/v/manifest.mpd"));
    let mpd = r#"<MPD><Period><AdaptationSet contentType="video"><Representation height="720" bandwidth="2000000"/></AdaptationSet></Period></MPD>"#;
    assert!(streams.describe(7, "https://cdn.e.com/v/manifest.mpd", mpd));
    assert_eq!(streams.of(7)[0].label, "720p DASH stream");
}

#[test]
fn the_list_is_best_first_and_a_small_clip_beside_a_video_is_not_in_it() {
    let mut streams = Streams::default();
    streams.absorb(&playlist_seen("https://e.com/pl/avc1/640x360/a.m3u8"));
    streams.absorb(&playlist_seen("https://e.com/pl/avc1/1920x1080/a.m3u8"));
    streams.absorb(&playlist_seen("https://e.com/pl/avc1/1280x720/a.m3u8"));
    let labels: Vec<String> = streams.of(7).into_iter().map(|s| s.label).collect();
    assert_eq!(
        labels,
        ["1080p HLS stream", "720p HLS stream", "360p HLS stream"],
        "the tallest picture first"
    );

    // TikTok's own sign-in clip (193 KB) beside the video (4 MB).
    let file = |url: &str, bytes: &str| {
        seen(
            url,
            "media",
            &[("Content-Type", "video/mp4"), ("Content-Length", bytes)],
        )
    };
    let mut tab = Streams::default();
    tab.absorb(&file("https://v.example.com/playback1.mp4", "197632"));
    assert_eq!(tab.of(7).len(), 1, "alone, a small clip is the post");
    tab.absorb(&file("https://v.example.com/video.mp4", "4194304"));
    let listed = tab.of(7);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert!(listed[0].url.ends_with("video.mp4"));
}

#[test]
fn a_new_page_forgets_the_kept_copy_and_a_woken_worker_merges_rather_than_overwrites() {
    // The worker slept; the tab navigated. Its memory is empty, but the
    // kept copy must go, or the next page offers the old one's video.
    let page = seen(
        "https://other.example/",
        "main_frame",
        &[("Content-Type", "text/html; charset=utf-8")],
    );
    assert_eq!(keep(&page), Keep::Forget);
    // A download opened in the tab is not a new page.
    let file = seen(
        "https://e.com/f.zip",
        "main_frame",
        &[("Content-Type", "application/zip")],
    );
    assert_eq!(keep(&file), Keep::Nothing);
    let attached = seen(
        "https://e.com/report",
        "main_frame",
        &[
            ("Content-Type", "text/html"),
            ("Content-Disposition", "attachment; filename=r.html"),
        ],
    );
    assert_eq!(keep(&attached), Keep::Nothing);
    let variant = seen("https://e.com/v/720.m3u8", "xmlhttprequest", &[]);
    assert_eq!(keep(&variant), Keep::Add);
    assert_eq!(
        keep(&seen("https://e.com/x.js", "xmlhttprequest", &[])),
        Keep::Nothing
    );

    // Woken mid-page: the master was kept before the worker slept, and
    // the first thing this worker hears is a variant.
    let master = stream_seen(&seen("https://e.com/master.m3u8", "xmlhttprequest", &[])).unwrap();
    let mut streams = Streams::default();
    streams.absorb(&variant);
    streams.restore(7, vec![master.clone()]);
    let raw = streams.raw(7);
    assert_eq!(raw.len(), 2, "{raw:?}");
    assert_eq!(raw[0], master, "the master is kept, and first");
    // Read twice (two responses racing the storage read): still once each.
    streams.restore(7, vec![master.clone()]);
    assert_eq!(streams.raw(7).len(), 2);
}

#[test]
fn a_stream_whose_read_outlives_its_page_is_dropped() {
    let mut pages = Pages::default();
    // An Add begins on the old page and awaits the kept copy...
    let then = pages.now(7);
    // ...while the new page's Forget runs.
    pages.next(7);
    assert!(
        !pages.still(7, then),
        "the old page's Add must not write back"
    );
    assert!(pages.still(7, pages.now(7)));
    assert!(pages.still(8, 0), "another tab is untouched");
}

#[test]
fn the_panels_list_says_each_quality_and_its_size_as_a_download_list_does() {
    assert_eq!(
        choice_line("MP4 1280×720", Some(47_400_000)),
        "MP4 1280×720 — 45 MB"
    );
    assert_eq!(
        choice_line("M4A audio", Some(3_300_000)),
        "M4A audio — 3.1 MB"
    );
    assert_eq!(choice_line("WEBM 3840×2160", None), "WEBM 3840×2160");
    assert_eq!(human_size(900), "900 B");
    assert_eq!(human_size(1536), "1.5 KB");
    assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
    let player = offer(&Media {
        tag: "VIDEO".into(),
        src: "blob:https://www.youtube.com/abc".into(),
        width: 800.0,
        height: 450.0,
    })
    .unwrap();
    assert!(player.page, "a player page has qualities to list");
    let file = offer(&Media {
        tag: "VIDEO".into(),
        src: "https://cdn.example/clip.mp4".into(),
        width: 800.0,
        height: 450.0,
    })
    .unwrap();
    assert!(!file.page, "a file is one quality");
}

#[test]
fn a_pages_links_are_listed_once_each_and_only_the_web_ones() {
    let page = "https://e.com/list.html";
    let hrefs: Vec<String> = [
        "https://e.com/a.zip",
        "https://e.com/a.zip#mirror",
        "mailto:me@e.com",
        "javascript:void(0)",
        "https://e.com/list.html#top",
        "http://cdn.e.com/b.iso",
        " https://e.com/c.pdf ",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        links_from(&hrefs, page),
        [
            "https://e.com/a.zip",
            "http://cdn.e.com/b.iso",
            "https://e.com/c.pdf"
        ]
    );
    let many: Vec<String> = (0..5000)
        .map(|n| format!("https://e.com/{n}.bin"))
        .collect();
    assert_eq!(links_from(&many, page).len(), MOST_LINKS);
    let ids: Vec<&str> = MENUS.iter().map(|(id, _, _)| *id).collect();
    assert!(ids.contains(&MENU_ALL_LINKS) && ids.contains(&MENU_SELECTED_LINKS));
}

#[test]
fn an_alt_click_leaves_a_web_address_with_the_browser_and_nothing_else() {
    let click = |kind: &str, src: &str| PanelClick {
        kind: kind.into(),
        src: src.into(),
        ..PanelClick::default()
    };
    assert_eq!(
        skip_url(&click(SKIP_MESSAGE, " https://e.com/f.zip ")),
        Ok("https://e.com/f.zip".to_string())
    );
    assert!(skip_url(&click(SKIP_MESSAGE, "javascript:void(0)")).is_err());
    assert_eq!(
        take_url(&click(TAKE_MESSAGE, "https://e.com/f.zip")).as_deref(),
        Ok("https://e.com/f.zip")
    );
    assert!(take_url(&click(SKIP_MESSAGE, "https://e.com/f.zip")).is_err());
    assert!(take_url(&click(TAKE_MESSAGE, "file:///etc/passwd")).is_err());
    assert_eq!(
        inline_url(&click(INLINE_MESSAGE, "https://e.com/a.pdf")).as_deref(),
        Ok("https://e.com/a.pdf")
    );
    assert!(inline_url(&click(INLINE_MESSAGE, "javascript:void(0)")).is_err());
    assert!(inline_url(&click(TAKE_MESSAGE, "https://e.com/a.pdf")).is_err());
    assert!(skip_url(&click(PANEL_MESSAGE, "https://e.com/f.zip")).is_err());
    let mut released = Released::default();
    released.remember("https://e.com/f.zip", 0.0);
    assert!(
        released.take("https://e.com/f.zip", 10.0),
        "let through once"
    );
    assert!(!released.take("https://e.com/f.zip", 20.0), "and only once");
}

const OWN: &str = "ownextensionid";

fn item(value: Value) -> DownloadItem {
    serde_json::from_value(value).unwrap()
}

#[test]
fn a_chrome_download_item_is_read_as_chrome_sends_it() {
    let found = first_look(
        &item(json!({
            "id": 7, "url": "https://e.com/get?id=1", "finalUrl": "https://cdn.e.com/f.zip",
            "referrer": "https://e.com/page", "filename": "", "mime": "application/zip",
            "fileSize": 2048, "totalBytes": 2048, "state": "in_progress", "incognito": false,
            "danger": "safe", "exists": true,
        })),
        OWN,
    )
    .unwrap();
    assert_eq!(found.id, 7);
    assert_eq!(
        found.url, "https://cdn.e.com/f.zip",
        "where the redirects ended"
    );
    assert_eq!(found.referrer, "https://e.com/page");
    assert_eq!(found.size, Some(2048));
}

#[test]
fn a_firefox_download_item_with_nulls_is_read_too() {
    // Firefox has no finalUrl, sends null for an absent referrer, and -1
    // for a size it does not know.
    let found = first_look(
        &item(json!({
            "id": 3, "url": "https://e.com/f.iso", "referrer": null,
            "filename": "/home/x/Downloads/f.iso", "mime": null,
            "fileSize": -1, "totalBytes": -1, "state": "in_progress",
        })),
        OWN,
    )
    .unwrap();
    assert_eq!(found.url, "https://e.com/f.iso");
    assert_eq!(found.referrer, "");
    assert_eq!(found.size, None);
}

#[test]
fn what_dm_restarted_itself_is_never_caught_again() {
    let ours = item(json!({ "id": 1, "url": "https://e.com/f.zip", "byExtensionId": OWN }));
    assert!(first_look(&ours, OWN).is_err());
    let theirs = item(json!({ "id": 1, "url": "https://e.com/f.zip", "byExtensionId": "x" }));
    assert!(first_look(&theirs, OWN).is_ok());
}

/// A PDF Chromium 155 (Debian) finished before the extension could hold
/// it, as `chrome-check.py` logged it: 300 KB served at full speed with
/// `Content-Disposition: attachment`. It used to be left with the
/// browser as "already finished, usually from the cache".
#[test]
fn a_pdf_chromium_finished_before_it_could_be_held_is_still_handed_over() {
    let pdf = item(json!({
        "id": 3, "url": "http://127.0.0.1:41857/files/small.pdf",
        "finalUrl": "http://127.0.0.1:41857/files/small.pdf",
        "referrer": "http://127.0.0.1:41857/page.html",
        "filename": "/tmp/browser-downloads/small (1).pdf", "mime": "application/pdf",
        "state": "complete", "bytesReceived": 300000, "totalBytes": 300000,
        "fileSize": 300000, "danger": "safe", "exists": true, "paused": false,
        "canResume": false, "incognito": false,
    }));
    let found = first_look(&pdf, OWN).unwrap();
    assert_eq!(found.mime, "application/pdf");
    assert_eq!(found.size, Some(300000));
    assert!(
        dm_protocol::CatchRules::default()
            .takes(&found.candidate(), false)
            .is_ok(),
        "a PDF is a type caught by default"
    );
    // Its pause fails (or holds nothing); it was finished, so it goes,
    // and once dm has it the browser's whole file is removed rather
    // than cancelled, which would leave it.
    assert_eq!(unheld(Some("complete")), Ok(()));
    assert_eq!(after_taken(Some("complete")), BrowserCopy::Remove);
    // One the person stopped, or erased, is not handed.
    assert!(unheld(Some("interrupted")).is_err());
    assert!(unheld(None).is_err());
    // The usual case: held, then cancelled.
    assert_eq!(unheld(Some("in_progress")), Ok(()));
    assert_eq!(after_taken(Some("in_progress")), BrowserCopy::Cancel);
    // Still never one dm restarted itself, finished or not.
    let ours = item(json!({
        "id": 1, "url": "https://e.com/f.pdf", "state": "complete", "byExtensionId": OWN,
    }));
    assert!(first_look(&ours, OWN).is_err());
}

#[test]
fn stopped_private_and_non_web_downloads_stay_with_the_browser() {
    for left in [
        json!({ "id": 1, "url": "https://e.com/f.zip", "state": "interrupted" }),
        json!({ "id": 1, "url": "https://e.com/f.zip", "incognito": true }),
        json!({ "id": 1, "url": "blob:https://e.com/1234" }),
        json!({ "id": 1, "url": "data:application/zip;base64,AAAA" }),
        json!({ "id": 1, "url": "file:///etc/passwd" }),
        json!({ "url": "https://e.com/f.zip" }),
    ] {
        assert!(first_look(&item(left.clone()), OWN).is_err(), "{left}");
    }
}

#[test]
fn a_link_is_handed_with_the_page_it_was_on() {
    let click = MenuClick {
        menu_item_id: json!(MENU_LINK),
        link_url: Some("https://e.com/f.zip".into()),
        page_url: Some("https://e.com/list".into()),
        ..Default::default()
    };
    assert_eq!(
        menu_target(&click, false),
        Ok(Target {
            url: "https://e.com/f.zip".into(),
            referrer: "https://e.com/list".into(),
            page: false,
        })
    );
}

#[test]
fn a_magnet_link_is_handed_over_and_a_javascript_one_is_not() {
    let mut click = MenuClick {
        menu_item_id: json!(MENU_LINK),
        link_url: Some("magnet:?xt=urn:btih:abc".into()),
        ..Default::default()
    };
    assert!(menu_target(&click, false).is_ok());
    click.link_url = Some("javascript:void(0)".into());
    let refused = menu_target(&click, false).unwrap_err();
    assert!(refused.contains("javascript:"), "{refused}");
}

#[test]
fn media_with_a_real_source_is_a_file_and_a_blob_source_is_its_page() {
    let mut click = MenuClick {
        menu_item_id: json!(MENU_MEDIA),
        src_url: Some("https://cdn.e.com/clip.mp4".into()),
        page_url: Some("https://e.com/watch?v=1".into()),
        ..Default::default()
    };
    let file = menu_target(&click, false).unwrap();
    assert_eq!(file.url, "https://cdn.e.com/clip.mp4");
    assert!(!file.page);

    click.src_url = Some("blob:https://e.com/5c1d".into());
    let page = menu_target(&click, false).unwrap();
    assert_eq!(page.url, "https://e.com/watch?v=1");
    assert!(page.page, "a player page goes to the app");

    // In a frame, the frame is the player's page.
    click.frame_url = Some("https://player.e.com/embed/1".into());
    assert_eq!(
        menu_target(&click, false).unwrap().url,
        "https://player.e.com/embed/1"
    );
}

#[test]
fn no_menu_item_hands_over_a_whole_page() {
    // Links, media and selections, but no
    // item that downloads whatever page is open.
    assert!(MENUS.iter().all(|(id, _, _)| *id != "dm-page"));
    assert!(MENUS
        .iter()
        .all(|(_, _, contexts)| !contexts.contains(&"page") || contexts.len() == 1));
}

#[test]
fn a_click_on_something_else_is_refused_with_its_name() {
    let click = MenuClick {
        menu_item_id: json!("someone-elses"),
        ..Default::default()
    };
    assert!(menu_target(&click, false)
        .unwrap_err()
        .contains("someone-elses"));
}

#[test]
fn a_hand_off_is_filed_under_a_plain_name() {
    let id = hand_id("b", 42, 1_700_000_000_123.0);
    assert_eq!(id, "b42-1700000000123");
    assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
}

#[test]
fn the_fix_named_is_the_one_for_this_browser() {
    // The unpacked build's id is the one --register assumes; any other
    // has to be named.
    assert!(absent_text("x", CHROME_EXTENSION_ID, false).ends_with("dm-desktop --register"));
    assert!(absent_text("x", "storeid", false).ends_with("dm-desktop --register storeid"));
    assert!(absent_text("x", "dm@localhost", true).ends_with("dm-desktop --register"));
    assert!(absent_text("timed out", "a", false).contains("(timed out)"));
}

/// With no host the extension cannot know whether dm runs, so
/// it says what works either way — open dm, again if it is open — and
/// no longer says to install an app the person has.
#[test]
fn no_host_says_open_dm_and_not_install_it() {
    for (reason, firefox) in [
        ("Specified native messaging host not found.", false),
        (
            "Access to the specified native messaging host is forbidden.",
            false,
        ),
        ("No such native application com.dm.host", true),
        (
            "This extension does not have permission to use native application com.dm.host",
            true,
        ),
    ] {
        assert!(not_connected(reason), "{reason}");
        let popup = absent_popup_text(reason, CHROME_EXTENSION_ID, firefox);
        assert!(
            popup.starts_with("dm isn't connected to this browser yet."),
            "{popup}"
        );
        assert!(
            popup.contains("if it's already open, open it again"),
            "{popup}"
        );
        assert!(popup.contains("then reopen this popup."), "{popup}");
        assert!(
            popup.ends_with("Advanced: dm-desktop --register"),
            "{popup}"
        );
        assert!(!popup.contains("Install"), "{popup}");
        let notice = absent_text(reason, CHROME_EXTENSION_ID, firefox);
        assert!(notice.contains("then try again."), "{notice}");
    }
    // A host that is there and did not answer is not "not connected":
    // Open dm can still start it.
    for reason in [
        "the dm app did not answer in time",
        "Native host has exited.",
    ] {
        assert!(!not_connected(reason), "{reason}");
        let said = absent_popup_text(reason, CHROME_EXTENSION_ID, false);
        assert!(said.starts_with("dm did not answer ("), "{said}");
    }
}

#[test]
fn an_answer_about_the_app_lasts_as_long_as_it_should() {
    let present = Known {
        app: App::Present {
            hello: Hello::default(),
            rules: CatchRules::default(),
        },
        at_ms: 1000.0,
    };
    assert!(present.fresh(1000.0 + PRESENT_FOR_MS - 1.0));
    assert!(!present.fresh(1000.0 + PRESENT_FOR_MS));
    assert!(!present.fresh(999.0), "a clock that went backwards");

    let absent = Known {
        app: App::Absent { reason: "x".into() },
        at_ms: 0.0,
    };
    assert!(absent.fresh(ABSENT_FOR_MS - 1.0));
    assert!(
        !absent.fresh(ABSENT_FOR_MS),
        "an app installed since is found"
    );
}

fn hello() -> Value {
    json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "0.1.0" })
}

#[test]
fn a_handshake_is_whole_with_a_hello_and_the_rules() {
    let mut probe = Probe::default();
    probe.absorb(&hello());
    assert!(probe.has_hello());
    assert_eq!(probe.finished(), None, "still waiting for the rules");
    probe.absorb(&json!({
        "protocolVersion": 1, "type": "settings.get",
        "catch": { "enabled": true, "extensions": ["bin"], "minSizeKb": 0, "skipHosts": [] },
    }));
    let Some(App::Present { hello, rules }) = probe.finished() else {
        panic!("not present")
    };
    assert_eq!(hello.engine_version, "0.1.0");
    assert_eq!(rules.extensions, vec!["bin".to_string()]);
}

#[test]
fn a_host_too_old_to_send_rules_gets_the_defaults_and_no_host_is_absent() {
    let mut probe = Probe::default();
    probe.absorb(&hello());
    assert_eq!(
        probe.give_up("timed out"),
        App::Present {
            hello: Hello {
                engine_version: "0.1.0".into(),
                protocol: None,
                rules: None,
                renews: false,
            },
            rules: CatchRules::default(),
        }
    );
    assert_eq!(
        Probe::default().give_up("Specified native messaging host not found."),
        App::Absent {
            reason: "Specified native messaging host not found.".into()
        }
    );
}

#[test]
fn a_host_of_another_protocol_is_treated_as_no_host_and_names_the_side_to_update() {
    let mut probe = Probe::default();
    probe.absorb(&json!({ "protocolVersion": 3, "type": "engine.hello" }));
    let Some(App::Absent { reason }) = probe.finished() else {
        panic!("a host of another version was used")
    };
    let said = absent_text(&reason, CHROME_EXTENSION_ID, false);
    assert!(said.contains("update the dm extension"), "{said}");
    assert!(
        !said.contains("--register") && !said.contains("Install"),
        "{said}"
    );

    let mut probe = Probe::default();
    probe.absorb(&json!({ "protocolVersion": 0, "type": "engine.hello" }));
    let Some(App::Absent { reason }) = probe.finished() else {
        panic!("a host of another version was used")
    };
    let said = absent_text(&reason, CHROME_EXTENSION_ID, false);
    assert!(said.contains("update the dm app"), "{said}");
    assert!(!said.contains("--register"), "{said}");
    // No host at all is still told how to install one.
    assert!(absent_text("not found", CHROME_EXTENSION_ID, false).contains("--register"));
}

#[test]
fn a_download_the_browser_holds_goes_only_to_an_app_that_judges_pages() {
    // An app of protocol 1 says nothing of its protocol in its hello.
    let mut probe = Probe::default();
    probe.absorb(&hello());
    let Some(App::Present { hello: old, .. }) = Some(probe.give_up("no rules")) else {
        unreachable!()
    };
    assert_eq!(takes_caught(&old), Err(OLD_APP));
    assert!(OLD_APP.contains("update the dm app"));
    let (line, bad) = status_line(&old);
    assert!(bad && line.contains("update the dm app"), "{line}");

    let mut probe = Probe::default();
    probe.absorb(&json!({
        "protocolVersion": 1, "type": "engine.hello", "engineVersion": "3.0.1",
        "protocol": 2,
    }));
    let App::Present { hello: new, .. } = probe.give_up("no rules") else {
        panic!("not present")
    };
    assert_eq!(takes_caught(&new), Ok(()));
    let (line, bad) = status_line(&new);
    assert!(!bad && !line.contains("update the dm app"), "{line}");
}

#[test]
fn only_the_answer_about_this_hand_off_counts() {
    let started = json!({ "protocolVersion": 1, "type": "download.started", "id": "b1-2" });
    assert_eq!(taken(&started, "b1-2"), Some(Ok(())));
    assert_eq!(taken(&started, "b9-9"), None);
    let failed = json!({
        "protocolVersion": 1, "type": "download.failed", "id": "b1-2",
        "failure": { "message": "dm could not take it: disk full" },
    });
    assert_eq!(
        taken(&failed, "b1-2"),
        Some(Err("dm could not take it: disk full".into()))
    );
    let bare = json!({ "protocolVersion": 1, "type": "download.failed", "id": "b1-2" });
    assert!(matches!(taken(&bare, "b1-2"), Some(Err(m)) if !m.is_empty()));
    assert_eq!(taken(&hello(), "b1-2"), None);
}

#[test]
fn a_url_given_back_is_let_through_once_and_not_for_ever() {
    let mut released = Released::default();
    released.remember("https://e.com/f.zip", 0.0);
    assert!(released.take("https://e.com/f.zip", 10.0));
    assert!(!released.take("https://e.com/f.zip", 20.0), "only once");

    released.remember("https://e.com/g.zip", 0.0);
    assert!(!released.take("https://e.com/g.zip", RELEASED_FOR_MS + 1.0));
}

#[test]
fn the_menus_cover_links_media_and_the_page() {
    let contexts: Vec<&str> = MENUS
        .iter()
        .flat_map(|(_, _, c)| c.iter().copied())
        .collect();
    for needed in ["link", "video", "audio", "page"] {
        assert!(contexts.contains(&needed), "{needed}");
    }
}

#[test]
fn the_panel_is_for_real_players_only() {
    let video = |w: f64, h: f64, src: &str| Media {
        tag: "VIDEO".into(),
        src: src.into(),
        width: w,
        height: h,
    };
    assert_eq!(
        offer(&video(640.0, 360.0, "https://cdn.e.com/v/clip.mp4")),
        Some(Offer {
            title: "Download video",
            sub: "cdn.e.com".into(),
            page: false,
        })
    );
    assert!(
        offer(&video(120.0, 90.0, "https://e.com/a.mp4")).is_none(),
        "an avatar got a panel"
    );
    assert!(offer(&video(f64::NAN, 400.0, "https://e.com/a.mp4")).is_none());
    let player = offer(&video(800.0, 450.0, "blob:https://www.e.com/1f0a")).unwrap();
    assert!(player.sub.contains("through the dm app"));
    let audio = Media {
        tag: "audio".into(),
        src: "https://e.com/song.mp3".into(),
        width: 300.0,
        height: 40.0,
    };
    assert_eq!(offer(&audio).map(|o| o.title), Some("Download this audio"));
    assert!(offer(&Media {
        tag: "img".into(),
        ..audio
    })
    .is_none());
}

#[test]
fn a_dragged_panel_goes_where_it_is_put_and_stays_on_screen() {
    let view = (1200.0, 800.0);
    let size = (240.0, 30.0);
    assert_eq!(
        dragged((500.0, 60.0), (-200.0, 300.0), size, view),
        (300.0, 360.0)
    );
    // Not past an edge, however far it is pulled.
    assert_eq!(
        dragged((500.0, 60.0), (5000.0, -5000.0), size, view),
        (952.0, 8.0)
    );
    assert_eq!(
        dragged((500.0, 60.0), (f64::NAN, 0.0), size, view),
        (8.0, 60.0)
    );
}

#[test]
fn the_panel_sits_in_the_players_corner_and_stays_on_screen() {
    // Inside the top right of a player in the middle of the page.
    assert_eq!(
        panel_at((100.0, 50.0, 640.0, 360.0), (220.0, 44.0), (1200.0, 800.0)),
        (512.0, 58.0)
    );
    // A player wider than the window: kept inside the window.
    let (x, _) = panel_at((0.0, 0.0, 3000.0, 400.0), (220.0, 44.0), (1200.0, 800.0));
    assert_eq!(x, 1200.0 - 220.0 - 8.0);
    // A player scrolled above the top: the panel stays visible.
    let (_, y) = panel_at((0.0, -300.0, 640.0, 360.0), (220.0, 44.0), (1200.0, 800.0));
    assert_eq!(y, 8.0);
    assert_eq!(
        panel_at((f64::NAN, 0.0, 10.0, 10.0), (1.0, 1.0), (10.0, 10.0)).0,
        8.0
    );
}

/// The bar sits on the part of the video a person sees: below a page's fixed
/// header over the video's top, and inside the window; on a video narrower than
/// 400 px it is the compact bar (gaps row 76).
#[test]
fn the_bar_sits_on_the_seen_part_of_the_video_and_is_compact_on_a_narrow_one() {
    // A header 60 px tall over a video starting at 20: the bar goes under it.
    let seen = visible_box((100.0, 20.0, 640.0, 360.0), (1200.0, 800.0), Some(60.0));
    assert_eq!(seen, (100.0, 60.0, 640.0, 320.0));
    let size = panel_size(640.0);
    assert_eq!(size, (240.0, 30.0));
    assert_eq!(panel_at(seen, size, (1200.0, 800.0)), (492.0, 68.0));
    // A video running past the window's right edge: clipped to the window.
    let seen = visible_box((900.0, 100.0, 640.0, 360.0), (1200.0, 800.0), None);
    assert_eq!(seen, (900.0, 100.0, 300.0, 360.0));
    // Nothing of it seen: its own box, not an empty one.
    let gone = (100.0, -900.0, 640.0, 360.0);
    assert_eq!(visible_box(gone, (1200.0, 800.0), None), gone);
    // A 292 px reel: the compact bar, in its top right.
    let size = panel_size(292.0);
    assert_eq!(size, (COMPACT_WIDTH, 30.0));
    let (x, _) = panel_at((10.0, 10.0, 292.0, 520.0), size, (1200.0, 800.0));
    assert_eq!(x, 10.0 + 292.0 - COMPACT_WIDTH - 8.0);
}

#[test]
fn a_panel_click_hands_the_file_or_the_page() {
    let click = |src: &str, frame: &str| PanelClick {
        kind: PANEL_MESSAGE.into(),
        src: src.into(),
        frame: frame.into(),
        ..PanelClick::default()
    };
    let page = "https://www.e.com/watch?v=1";
    let file = panel_target(&click("https://cdn.e.com/clip.mp4", page), page, false).unwrap();
    assert_eq!(
        (file.url.as_str(), file.referrer.as_str(), file.page),
        ("https://cdn.e.com/clip.mp4", page, false)
    );
    let player = panel_target(&click("blob:https://www.e.com/x", page), page, false).unwrap();
    assert_eq!((player.url.as_str(), player.page), (page, true));
    // An embedded player hands its own frame's page, which is the one
    // that knows the video.
    let embedded = panel_target(
        &click(
            "blob:https://player.v.com/x",
            "https://player.v.com/embed/9",
        ),
        page,
        false,
    )
    .unwrap();
    assert_eq!(embedded.url, "https://player.v.com/embed/9");
    assert!(panel_target(
        &PanelClick {
            kind: "other".into(),
            ..click("https://e.com/a.mp4", "")
        },
        page,
        false
    )
    .is_err());
}

#[test]
fn nothing_from_a_private_window_is_handed_over_from_a_menu_or_the_panel() {
    // As a private window's downloads are left with the browser.
    let link = MenuClick {
        menu_item_id: json!(MENU_LINK),
        link_url: Some("https://e.com/f.zip".into()),
        page_url: Some("https://e.com/list".into()),
        ..Default::default()
    };
    let said = menu_target(&link, true).unwrap_err();
    assert!(said.contains("private window"), "{said}");
    assert!(menu_target(&link, false).is_ok());
    let media = MenuClick {
        menu_item_id: json!(MENU_MEDIA),
        src_url: Some("blob:https://e.com/1".into()),
        page_url: Some("https://e.com/watch?v=1".into()),
        ..Default::default()
    };
    assert!(menu_target(&media, true).is_err());
    let click = PanelClick {
        kind: PANEL_MESSAGE.into(),
        src: "https://cdn.e.com/clip.mp4".into(),
        ..PanelClick::default()
    };
    assert!(panel_target(&click, "https://e.com/watch", true).is_err());
    assert!(panel_target(&click, "https://e.com/watch", false).is_ok());
    let formats = PanelClick {
        kind: FORMATS_MESSAGE.into(),
        ..click
    };
    assert!(panel_target(&formats, "https://e.com/watch", true).is_err());
    assert!(not_private(true).is_err() && not_private(false).is_ok());
}

#[test]
fn a_site_whose_every_download_is_taken_covers_its_redirects_and_its_pages() {
    let all = with_site(&[], "github.com", true);
    // Asked for on the site, redirected (Chrome's finalUrl) to its file host.
    let redirected = first_look(
        &item(json!({
            "id": 1,
            "url": "https://github.com/o/r/releases/download/v1/f.weird",
            "finalUrl": "https://objects.githubusercontent.com/123?sig=x",
        })),
        OWN,
    )
    .unwrap();
    assert!(forced_download(&all, &redirected));
    // Started from one of its pages, served from elsewhere.
    let from_page = first_look(
        &item(json!({
            "id": 2, "url": "https://cdn.example/f.weird",
            "referrer": "https://github.com/o/r",
        })),
        OWN,
    )
    .unwrap();
    assert!(forced_download(&all, &from_page));
    let elsewhere = first_look(
        &item(json!({ "id": 3, "url": "https://cdn.example/f.weird" })),
        OWN,
    )
    .unwrap();
    assert!(!forced_download(&all, &elsewhere));
}

#[test]
fn a_forced_site_still_answers_to_the_apps_switch_and_skip_list() {
    let found = first_look(
        &item(json!({
            "id": 1, "url": "https://github.com/o/r/notes.txt",
            "referrer": "https://github.com/o/r",
        })),
        OWN,
    )
    .unwrap();
    let rules = CatchRules {
        extensions: vec!["zip".into()],
        ..CatchRules::default()
    };
    assert!(
        rules.catches(&found.candidate()).is_err(),
        "not a type caught"
    );
    assert!(rules.takes(&found.candidate(), true).is_ok(), "but forced");
    let off = CatchRules {
        enabled: false,
        ..CatchRules::default()
    };
    assert!(off.takes(&found.candidate(), true).is_err());
    let skipping = CatchRules {
        skip_hosts: vec!["github.com".into()],
        ..CatchRules::default()
    };
    assert!(skipping.takes(&found.candidate(), true).is_err());
}

#[test]
fn the_host_answers_well_before_the_extension_stops_waiting() {
    // The host's probe and its margin for answering fit inside the wait,
    // with room left for the host to start: a late "taken" after the
    // browser was given its download back is a file downloaded twice.
    let host_ms = (dm_protocol::PROBE_WITHIN_MS + dm_protocol::ANSWER_MARGIN_MS) as f64;
    assert!(host_ms + 5_000.0 <= TAKEN_WITHIN_MS, "{host_ms}");
}

fn player(itag: u32, mime: &str, clen: u64, extra: &str) -> String {
    format!(
        "https://rr1.sn-x.googlevideo.com/videoplayback?expire=9&itag={itag}&mime={mime}&clen={clen}&sig=abc{extra}"
    )
}

#[test]
fn a_picture_and_a_sound_a_player_fetched_apart_are_one_line_with_both_addresses() {
    let mut streams = Streams::default();
    let picture = player(137, "video%2Fmp4", 900, "&range=0-99&rn=3");
    let sound = player(140, "audio%2Fmp4", 100, "&rn=4&rbuf=1");
    for url in [&picture, &sound] {
        streams.absorb(&seen(url, "xmlhttprequest", &[]));
    }
    let list = streams.of(7);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].label, "1080p video + sound");
    assert_eq!(list[0].size, Some(1000));
    // The pieces' own parameters are not part of the file's address.
    assert!(!list[0].url.contains("range=") && !list[0].url.contains("rn="));
    let sound = list[0].sound.as_deref().unwrap();
    assert!(sound.contains("itag=140") && !sound.contains("rbuf"));
}

#[test]
fn a_picture_with_no_sound_is_not_offered_alone() {
    let mut streams = Streams::default();
    let mut one = seen(&player(137, "video%2Fmp4", 900, ""), "xmlhttprequest", &[]);
    one.tab_id = 7;
    streams.absorb(&one);
    assert!(streams.of(7).is_empty());
}

#[test]
fn only_a_video_servers_playback_addresses_are_read_as_tracks() {
    assert!(track_of("https://evil.example/videoplayback?itag=137&mime=video%2Fmp4").is_none());
    assert!(track_of("https://notgooglevideo.com/videoplayback?itag=137").is_none());
    assert!(track_of("https://a.googlevideo.com/other?itag=137").is_none());
    // A host another URL parser would read differently is not the suffix it ends with.
    assert!(
        track_of("https://evil.example#.googlevideo.com/videoplayback?itag=22&mime=video/mp4")
            .is_none()
    );
    assert!(track_of(
        "https://evil.example\\@a.googlevideo.com/videoplayback?itag=22&mime=video/mp4"
    )
    .is_none());
    // Built here, so no tracked file holds a login in an address.
    let userinfo = ["a.googlevideo.com", "8080"].join(":");
    assert!(track_of(&format!(
        "https://{userinfo}@evil.example.googlevideo.com/videoplayback?itag=22&mime=video/mp4"
    ))
    .is_none());
    assert!(
        track_of("https://rr1---sn-abc.googlevideo.com/videoplayback?itag=22&mime=video/mp4")
            .is_some()
    );
    assert!(track_of("https://a.googlevideo.com/videoplayback?itag=9999").is_none());
}

#[test]
fn a_site_capturing_is_paused_for_keeps_its_downloads_wherever_they_are_served_from() {
    let listed = vec!["example.org".to_string()];
    let own = first_look(
        &item(json!({ "id": 1, "url": "https://files.example.org/a.zip" })),
        OWN,
    )
    .unwrap();
    assert!(paused_download(&listed, &own), "a site it is part of");
    let from_page = first_look(
        &item(json!({
            "id": 2, "url": "https://cdn.other/a.zip",
            "referrer": "https://example.org/get",
        })),
        OWN,
    )
    .unwrap();
    assert!(
        paused_download(&listed, &from_page),
        "started from its page"
    );
    let elsewhere = first_look(
        &item(json!({ "id": 3, "url": "https://cdn.other/a.zip" })),
        OWN,
    )
    .unwrap();
    assert!(!paused_download(&listed, &elsewhere));
    assert!(paused(&listed, "https://www.example.org/x.pdf"));
    assert!(!paused(&[], "https://example.org/x.pdf"));
}

#[test]
fn youtubes_player_answers_for_watch_and_shorts_pages_alone() {
    assert_eq!(
        youtube_video_id("https://www.youtube.com/watch?v=jNQXAC9IVRw&t=3s").as_deref(),
        Some("jNQXAC9IVRw")
    );
    assert_eq!(
        youtube_video_id("https://m.youtube.com/shorts/abc-_123?x=1").as_deref(),
        Some("abc-_123")
    );
    assert_eq!(
        youtube_video_id("https://www.youtube.com/playlist?list=PL1"),
        None
    );
    assert_eq!(
        youtube_video_id("https://www.video.example/watch?v=abc"),
        None
    );
    assert_eq!(
        youtube_video_id("https://www.youtube.com/watch?list=PL1"),
        None
    );
    let levels: Vec<String> = [
        "auto", "hd1080", "hd720", "hd1080", "large", "medium", "tiny", "odd",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(player_heights(&levels), [1080, 720, 480, 360, 144]);
}

#[test]
fn media_on_the_built_in_exclusion_list_is_not_offered_but_an_ordinary_file_is() {
    // An ad server naming itself, an ad network's host, an ad stub in
    // Windows media: all left alone.
    assert!(stream_seen(&seen(
        "https://cdn.e.com/promo.mp4",
        "media",
        &[
            ("Content-Type", "video/mp4"),
            ("Server", "DCLK-AdSvr"),
            ("Content-Length", "9000000")
        ],
    ))
    .is_none());
    assert!(stream_seen(&seen(
        "https://ad.doubleclick.net/spot.mp4",
        "media",
        &[("Content-Type", "video/mp4"), ("Content-Length", "9000000")],
    ))
    .is_none());
    assert!(stream_seen(&seen(
        "https://cdn.e.com/stub.asf",
        "media",
        &[
            ("Content-Type", "video/x-ms-asf"),
            ("Content-Length", "50000")
        ],
    ))
    .is_none());
    // A file a page plays that nothing on the list names is still found.
    let found = stream_seen(&seen(
        "https://cdn.e.com/clip.mp4",
        "media",
        &[("Content-Type", "video/mp4"), ("Content-Length", "9000000")],
    ));
    assert!(found.is_some(), "an ordinary media file was left out");
}

#[test]
fn the_panels_part_line_hands_over_a_time_and_nothing_else() {
    use super::part_text;
    assert_eq!(part_text(" 1:30 "), "1:30");
    assert_eq!(part_text("90"), "90");
    assert_eq!(part_text("1m30s"), "1m30s");
    assert_eq!(part_text("1:02:03.5"), "1:02:03.5");
    assert_eq!(part_text("<b>x</b>"), "");
    assert_eq!(part_text("1:30; rm -rf /"), "1:30m");
    assert_eq!(part_text("   "), "");
    assert_eq!(part_text(&"1".repeat(40)).len(), 16);
    // What the page script sends is read, a Part line or not.
    let click: PanelClick =
        serde_json::from_str(r#"{"type":"dm.panel","src":"u","from":"1:00","to":"2:00"}"#).unwrap();
    assert_eq!(
        (click.from.as_deref(), click.to.as_deref()),
        (Some("1:00"), Some("2:00"))
    );
    let plain: PanelClick = serde_json::from_str(r#"{"type":"dm.panel","src":"u"}"#).unwrap();
    assert!(plain.from.is_none() && plain.to.is_none());
}

#[test]
fn a_picture_whose_tag_is_padded_with_an_escaped_equals_sign_still_says_its_height() {
    // Facebook writes the padding of efg as %3D: "dash_h264-basic-gen2_720p".
    const PADDED: &str = "eyJ2ZW5jb2RlX3RhZyI6ImRhc2hfaDI2NC1iYXNpYy1nZW4yXzcyMHAifQ%3D%3D";
    const AUDIO: &str = "eyJ2ZW5jb2RlX3RhZyI6ImRhc2hfbG5faGVhYWNfdmJyM19hdWRpbyJ9";
    let picture = stream_seen(&cdn_piece(
        "scontent.fktm18-1.fna.fbcdn.net",
        "m366/AQpic",
        PADDED,
        0,
        99,
        "video/mp4",
    ))
    .unwrap();
    assert_eq!(
        (picture.kind, picture.label.as_str()),
        (StreamKind::Picture, "720p video")
    );
    let sound = stream_seen(&cdn_piece(
        "scontent.fktm18-1.fna.fbcdn.net",
        "m412/AQsnd",
        AUDIO,
        0,
        99,
        "audio/mp4",
    ))
    .unwrap();
    assert_eq!(sound.kind, StreamKind::Sound);
}

#[test]
fn a_pages_own_video_files_are_offered_for_the_video_that_is_playing() {
    const ID: &str = "AQOEmnf_c2TGtpRirhIDnZXgky_nEeo1jZFNmFMiNXegAa3MHxF6";
    // The page's data for two videos, far apart; the player fetched the first.
    let page = format!(
        r#"<script>{{"dash_manifests":[{{"manifest_xml":"<MPD><BaseURL>https:\/\/v.fbcdn.net\/o1\/v\/m366\/{ID}.mp4</BaseURL></MPD>"}}],"dash_manifest_urls":[{{"manifest_url":"https:\/\/v.fbcdn.net\/m\/first.mpd?x=1&y=2","failure_reason":null}}],"progressive_urls":[{{"progressive_url":"https:\/\/v.fbcdn.net\/v\/first-sd.mp4?a=1&b=2","failure_reason":null,"metadata":{{"quality":"SD"}}}},{{"progressive_url":"https:\/\/v.fbcdn.net\/v\/first-hd.mp4","failure_reason":null,"metadata":{{"quality":"HD"}}}}]}}</script>{pad}<script>{{"progressive_urls":[{{"progressive_url":"https:\/\/v.fbcdn.net\/v\/other-sd.mp4","failure_reason":null,"metadata":{{"quality":"SD"}}}}]}}</script>"#,
        pad = "x".repeat(400_000)
    );
    let seen = vec![format!(
        "https://v.fbcdn.net/o1/v/m366/{ID}.mp4?_nc_cat=1&efg=e30%3D"
    )];
    let lines = page_lines(&page, &seen);
    let said: Vec<(&str, &str)> = lines
        .iter()
        .map(|l| (l.label.as_str(), l.url.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            (
                "MP4 file, quality SD",
                "https://v.fbcdn.net/v/first-sd.mp4?a=1&b=2"
            ),
            ("MP4 file, quality HD", "https://v.fbcdn.net/v/first-hd.mp4"),
            ("DASH stream", "https://v.fbcdn.net/m/first.mpd?x=1&y=2"),
        ]
    );
    // A video the player has not been seen to fetch is not offered, and a page
    // that names nothing offers nothing.
    assert!(page_lines(
        &page,
        &["https://other.example/zzzzzzzzzzzzzzzzzzzzzzzzzzzz.mp4".into()]
    )
    .is_empty());
    assert!(page_lines("<p>no video here</p>", &seen).is_empty());
    assert!(page_lines(&page, &[]).is_empty());
}

/// The video's caption files in its record are lines after its files, named by their kind as
/// reference A names them ("SRT file"); a picture under a caption key is not one (gaps row 72c).
#[test]
fn a_videos_caption_files_in_the_page_are_lines_after_its_files() {
    const ID: &str = "AQOEmnf_c2TGtpRirhIDnZXgky_nEeo1jZFNmFMiNXegAa3MHxF6";
    let page = format!(
        r#"<script>{{"progressive_urls":[{{"progressive_url":"https:\/\/v.fbcdn.net\/v\/{ID}.mp4","metadata":{{"quality":"HD"}}}}],"video_available_captions_locales":[{{"locale":"en_US","captions_url":"https:\/\/l.fbcdn.net\/captions\/en_US"}}],"tracks":[{{"src":"https:\/\/cdn.example\/subs\/talk.vtt"}}],"caption_thumbnail":"https:\/\/cdn.example\/thumb.jpg"}}</script>"#
    );
    let seen = vec![format!("https://v.fbcdn.net/v/{ID}.mp4?x=1")];
    let said: Vec<(String, String)> = page_lines(&page, &seen)
        .into_iter()
        .map(|l| (l.label, l.url))
        .collect();
    assert!(
        said.contains(&(
            "SRT file".to_string(),
            "https://l.fbcdn.net/captions/en_US".to_string()
        )),
        "{said:?}"
    );
    assert!(
        said.contains(&(
            "VTT file".to_string(),
            "https://cdn.example/subs/talk.vtt".to_string()
        )),
        "{said:?}"
    );
    assert!(
        !said.iter().any(|(_, url)| url.ends_with("thumb.jpg")),
        "{said:?}"
    );
    let first_caption = said
        .iter()
        .position(|(l, _)| l.ends_with(" file") && !l.starts_with("MP4"));
    let last_file = said.iter().rposition(|(l, _)| l.starts_with("MP4"));
    assert!(first_caption > last_file, "{said:?}");
}

#[test]
fn a_pages_dash_manifest_gives_each_rendition_its_height_bitrate_and_length() {
    const ID: &str = "AQOEmnf_c2TGtpRirhIDnZXgky_nEeo1jZFNmFMiNXegAa3MHxF6";
    // The shape of a manifest Facebook's page holds, addresses shortened.
    let xml = format!(
        "<?xml version=\"1.0\"?><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" mediaPresentationDuration=\"PT22.185099S\">         <Period id=\"0\"><AdaptationSet id=\"0\" contentType=\"video\">         <Representation id=\"1v\" bandwidth=\"2075447\" mimeType=\"video/mp4\" width=\"720\" height=\"1280\" FBQualityClass=\"hd\" FBQualityLabel=\"720p\">         <BaseURL>https://v.fbcdn.net/o1/v/m366/{ID}.mp4?a=1&amp;b=2</BaseURL></Representation>         <Representation id=\"2v\" bandwidth=\"353028\" mimeType=\"video/mp4\" width=\"360\" height=\"640\" FBQualityClass=\"sd\" FBQualityLabel=\"360p\">         <BaseURL>https://v.fbcdn.net/o1/v/m84/low.mp4</BaseURL></Representation></AdaptationSet>         <AdaptationSet id=\"1\" contentType=\"audio\"><Representation id=\"3a\" bandwidth=\"59194\" mimeType=\"audio/mp4\">         <BaseURL>https://v.fbcdn.net/o1/v/m412/sound.mp4</BaseURL></Representation></AdaptationSet></Period></MPD>"
    );
    let manifests = serde_json::json!([{ "manifest_xml": xml }]).to_string();
    let page = format!(
        r#"{{"dash_manifests":{manifests},"dash_manifest_urls":[{{"manifest_url":"https://v.fbcdn.net/m/x.mpd","failure_reason":null}}],"progressive_urls":[{{"progressive_url":"https://v.fbcdn.net/v/sd.mp4","failure_reason":null,"metadata":{{"quality":"SD"}}}},{{"progressive_url":"https://v.fbcdn.net/v/hd.mp4","failure_reason":null,"metadata":{{"quality":"HD"}}}}]}}"#
    );
    let seen = vec![format!("https://v.fbcdn.net/o1/v/m366/{ID}.mp4?x=1")];
    let lines = page_lines(&page, &seen);
    let said: Vec<&str> = lines.iter().map(|l| l.label.as_str()).collect();
    // As the other manager lists them: each rendition, then the page's own SD and HD
    // files; the manifest is not a line of its own when its renditions are.
    assert_eq!(
        said,
        [
            "MP4 file, 22 sec, quality 1280p HD, 2075 kbps",
            "MP4 file, 22 sec, quality 640p, 353 kbps",
            "MP4 file, quality SD",
            "MP4 file, quality HD",
        ]
    );
    // A picture goes with the best sound, joined; a whole file needs none.
    assert_eq!(
        lines[0].url,
        format!("https://v.fbcdn.net/o1/v/m366/{ID}.mp4?a=1&b=2")
    );
    assert_eq!(
        lines[0].sound.as_deref(),
        Some("https://v.fbcdn.net/o1/v/m412/sound.mp4")
    );
    assert_eq!(lines[2].sound, None);
}

#[test]
fn instagrams_manifest_and_files_give_the_reels_qualities() {
    const ID: &str = "AQMgO65IBSO4D1Fg1v8qWRLjCR7N-LaX4H_sPQeyoyjFQE149o094RW";
    let xml = format!(
        "<?xml version=\"1.0\"?><MPD mediaPresentationDuration=\"PT41S\"><Period><AdaptationSet contentType=\"video\"><Representation id=\"1\" bandwidth=\"1500000\" width=\"720\" height=\"1280\"><BaseURL>https://i.fbcdn.net/o1/v/{ID}.mp4?a=1&amp;b=2</BaseURL></Representation><Representation id=\"2\" bandwidth=\"500000\" width=\"360\" height=\"640\"><BaseURL>https://i.fbcdn.net/o1/v/low.mp4</BaseURL></Representation></AdaptationSet><AdaptationSet contentType=\"audio\"><Representation id=\"3\" bandwidth=\"60000\"><BaseURL>https://i.fbcdn.net/o1/v/sound.mp4</BaseURL></Representation></AdaptationSet></Period></MPD>"
    );
    let page = format!(
        r#"{{"video_versions":[{{"type":101,"url":"https:\/\/i.fbcdn.net\/o1\/v\/{ID}.mp4?x=1","width":720,"height":1280}}],"video_dash_manifest":{}}}"#,
        serde_json::Value::String(xml)
    );
    let seen = vec![format!(
        "https://i.fbcdn.net/o1/v/{ID}.mp4?x=1&bytestart=0&byteend=9"
    )];
    let lines = page_lines(&page, &seen);
    let said: Vec<&str> = lines.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(
        said,
        [
            "MP4 file, 41 sec, quality 1280p, 1500 kbps",
            "MP4 file, 41 sec, quality 640p, 500 kbps",
            "MP4 file, quality 1280p",
        ]
    );
    assert_eq!(
        lines[0].sound.as_deref(),
        Some("https://i.fbcdn.net/o1/v/sound.mp4")
    );
    assert!(page_lines(
        &page,
        &["https://other.example/zzzzzzzzzzzzzzzzzzzzzzzzzzzz.mp4".into()]
    )
    .is_empty());
}

#[test]
fn tiktoks_bitrate_info_gives_the_clips_qualities() {
    const ID: &str = "o4DPxfQfIIk3KMBHYxVIEGxCCEuyk9deAAzL6j";
    let page = format!(
        r#"{{"bitrateInfo":[{{"Bitrate":811768,"PlayAddr":{{"Height":1280,"UrlList":["https://v16.tiktok.com/video/tos/{ID}/?a=1&sig=2"]}}}},{{"Bitrate":400000,"PlayAddr":{{"Height":960,"UrlList":["https://v16.tiktok.com/video/tos/lowlowlowlowlowlowlowlowlowlow/?a=1"]}}}}]}}"#
    );
    let seen = vec![format!("https://v16.tiktok.com/video/tos/{ID}/?")];
    let said: Vec<String> = page_lines(&page, &seen)
        .into_iter()
        .map(|l| l.label)
        .collect();
    assert_eq!(
        said,
        [
            "MP4 file, quality 1280p, 811 kbps",
            "MP4 file, quality 960p, 400 kbps"
        ]
    );
    assert!(page_lines(
        &page,
        &["https://x.example/zzzzzzzzzzzzzzzzzzzzzzzzzzzz.mp4".into()]
    )
    .is_empty());
}

#[test]
fn a_site_nobody_wrote_a_reader_for_still_lists_its_renditions() {
    const ID: &str = "clip9f3a7c21d4e84b6a9c0d1e2f3a4b5c6d";
    // Another site's shape: sources with a label and a bitrate, and a poster that is not a video.
    let page = format!(
        r#"<html><script>window.__STATE__ = {{"poster":"https://img.example/p.jpg","player":{{"sources":[{{"label":"360p","bitrate":500000,"src":"https://cdn.example/v/{ID}_360.mp4"}},{{"label":"720p","bitrate":1500000,"src":"https://cdn.example/v/{ID}_720.mp4"}},{{"label":"1080p","bitrate":3000000,"src":"https://cdn.example/v/{ID}_1080.mp4"}}]}}}};</script><script>var other = {{"sources":[{{"label":"240p","src":"https://cdn.example/v/another_240.mp4"}}]}};</script></html>"#
    );
    let seen = vec![format!("https://cdn.example/v/{ID}_720.mp4?token=1")];
    let said: Vec<(String, String)> = page_lines(&page, &seen)
        .into_iter()
        .map(|l| (l.label, l.url))
        .collect();
    assert_eq!(
        said,
        [
            (
                "MP4 file, quality 1080p, 3000 kbps".to_string(),
                format!("https://cdn.example/v/{ID}_1080.mp4")
            ),
            (
                "MP4 file, quality 720p, 1500 kbps".to_string(),
                format!("https://cdn.example/v/{ID}_720.mp4")
            ),
            (
                "MP4 file, quality 360p, 500 kbps".to_string(),
                format!("https://cdn.example/v/{ID}_360.mp4")
            ),
        ]
    );
}

#[test]
fn a_caught_download_is_named_by_the_title_of_the_tab_it_came_from() {
    let tabs = vec![
        (
            "https://news.example/a".to_string(),
            "Other page".to_string(),
        ),
        (
            "https://x.example/nasa/status/1#top".to_string(),
            "NASA on X: \"Everybody, wave!\"".to_string(),
        ),
        (
            "https://bare.example/".to_string(),
            "https://bare.example/".to_string(),
        ),
        ("https://blank.example/".to_string(), "  ".to_string()),
    ];
    assert_eq!(
        referrer_title(&tabs, "https://x.example/nasa/status/1"),
        Some("NASA on X: \"Everybody, wave!\"".to_string())
    );
    // A tab whose title is only its address, or empty, names nothing.
    assert_eq!(referrer_title(&tabs, "https://bare.example/"), None);
    assert_eq!(referrer_title(&tabs, "https://blank.example/"), None);
    // No referrer, or no open tab at it.
    assert_eq!(referrer_title(&tabs, ""), None);
    assert_eq!(referrer_title(&tabs, "https://gone.example/"), None);
}

#[test]
fn a_players_id_from_a_site_finder_picks_its_videos_lines_out_of_a_feed() {
    // A feed's data holds two videos; the finder said the player is the second.
    let page = r#"<script>{"videos":[
        {"videoFBID":"1111111111","progressive_urls":[{"progressive_url":"https:\/\/v.example\/v\/one-sd.mp4","metadata":{"quality":"SD"}}]},
        {"videoFBID":"2222222222","progressive_urls":[{"progressive_url":"https:\/\/v.example\/v\/two-sd.mp4","metadata":{"quality":"SD"}},{"progressive_url":"https:\/\/v.example\/v\/two-hd.mp4","metadata":{"quality":"HD"}}]}
    ]}</script>"#;
    let lines = page_lines_for_player(page, "2222222222");
    let urls: Vec<&str> = lines.iter().map(|l| l.url.as_str()).collect();
    assert!(urls.contains(&"https://v.example/v/two-hd.mp4"), "{urls:?}");
    assert!(!urls.iter().any(|u| u.contains("one-sd")), "{urls:?}");
    // An id too short to be told from any number, or one the page lacks, offers nothing.
    assert!(page_lines_for_player(page, "12").is_empty());
    assert!(page_lines_for_player(page, "9999999999").is_empty());
}

#[test]
fn a_players_byte_range_pieces_of_a_picture_and_a_sound_become_one_line_on_any_site() {
    let piece = |url: &str, mime: &str, range: &str| {
        let seen = Seen {
            url: url.into(),
            tab_id: 1,
            kind: "media".into(),
            status_code: 206,
            response_headers: vec![
                Header {
                    name: "Content-Type".into(),
                    value: Some(mime.into()),
                },
                Header {
                    name: "Content-Range".into(),
                    value: Some(range.into()),
                },
            ],
            ..Default::default()
        };
        stream_seen(&seen)
    };
    let low = piece(
        "https://cdn.example/v/a.mp4?id=7&bytestart=0&byteend=999",
        "video/mp4",
        "bytes 0-999/3000000",
    )
    .unwrap();
    assert_eq!(low.url, "https://cdn.example/v/a.mp4?id=7");
    assert_eq!(low.kind, StreamKind::Picture);
    assert_eq!(low.size, Some(3_000_000));
    let high = piece(
        "https://cdn.example/v/b.mp4?id=8&range=0-999",
        "video/mp4",
        "bytes 0-999/9000000",
    )
    .unwrap();
    let sound = piece(
        "https://cdn.example/v/c.m4a?id=9&bytestart=5&byteend=9",
        "audio/mp4",
        "bytes 5-9/700000",
    )
    .unwrap();
    assert_eq!(sound.kind, StreamKind::Sound);
    let mut streams = Streams::default();
    streams.restore(1, vec![low, high, sound]);
    let lines = streams.of(1);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        lines[0].url, "https://cdn.example/v/b.mp4?id=8",
        "the largest picture"
    );
    assert_eq!(
        lines[0].sound.as_deref(),
        Some("https://cdn.example/v/c.m4a?id=9")
    );
    assert_eq!(lines[0].size, Some(9_700_000));
    // Not a piece of a file: a segment of a stream, a type that is neither, or no range at all.
    assert!(piece(
        "https://cdn.example/s/1.m4s?bytestart=0",
        "video/mp4",
        "bytes 0-9/10"
    )
    .is_none());
    assert!(piece(
        "https://cdn.example/s/x?bytestart=0",
        "application/octet-stream",
        "bytes 0-9/10"
    )
    .is_none());
    assert!(piece(
        "https://cdn.example/v/a.mp4?id=7",
        "video/mp4",
        "bytes 0-9/10"
    )
    .is_none());
}

#[test]
fn the_media_types_reference_a_knows_are_read_the_same_way() {
    let kind = |url: &str, mime: &str| {
        stream_seen(&seen(url, "xmlhttprequest", &[("Content-Type", mime)])).map(|s| s.kind)
    };
    // Manifests and playlists by their type alone: the address says nothing.
    assert_eq!(
        kind(
            "https://e.com/playlist?id=3",
            "application/octet-stream-m3u8"
        ),
        Some(StreamKind::Hls)
    );
    assert_eq!(
        kind("https://e.com/playlist?id=3", "audio/x-mpegurl"),
        Some(StreamKind::Hls)
    );
    assert_eq!(
        kind("https://e.com/manifest?id=3", "video/vnd.mpeg.dash.mpd"),
        Some(StreamKind::Dash)
    );
    // Transport-stream pieces, picture or sound, are not files.
    assert_eq!(kind("https://e.com/s?n=1", "audio/mp2t"), None);
    assert_eq!(kind("https://e.com/s?n=1", "video/mp2t"), None);
    // Subtitles are not media, and HDS (an old Flash format) is not read: see docs/decisions.md.
    assert_eq!(kind("https://e.com/c.vtt", "text/vtt"), None);
    assert_eq!(kind("https://e.com/c.xml", "application/ttml+xml"), None);
    assert_eq!(kind("https://e.com/m.f4m", "video/f4m"), None);
    // Older containers a player may still load whole are files.
    let big = |url: &str, mime: &str| {
        stream_seen(&seen(
            url,
            "media",
            &[("Content-Type", mime), ("Content-Length", "9000000")],
        ))
        .map(|s| s.kind)
    };
    for (url, mime) in [
        ("https://e.com/a.flv", "video/x-flv"),
        ("https://e.com/a.3gp", "video/3gpp"),
        ("https://e.com/a.mov", "video/quicktime"),
        ("https://e.com/a.wmv", "video/x-ms-wmv"),
    ] {
        assert_eq!(big(url, mime), Some(StreamKind::File), "{url}");
    }
}

#[test]
fn an_advert_loaded_by_the_player_never_reaches_the_list() {
    let advert = |url: &str, kind: &str, headers: &[(&str, &str)]| {
        stream_seen(&seen(url, kind, headers)).is_none()
    };
    // A pre-roll from an ad server named in the `Server` header, whatever its address.
    assert!(advert(
        "https://cdn.example/preroll.mp4",
        "media",
        &[
            ("Content-Type", "video/mp4"),
            ("Content-Length", "9000000"),
            ("Server", "DCLK-AdSvr")
        ]
    ));
    // The IMA SDK's and DoubleClick's hosts.
    assert!(advert(
        "https://pubads.g.doubleclick.net/ondemand/hls/content/1/master.m3u8",
        "xmlhttprequest",
        &[("Content-Type", "application/vnd.apple.mpegurl")]
    ));
    // A Windows media stub of 100 KB or less.
    assert!(advert(
        "https://cdn.example/ad.wmv",
        "media",
        &[
            ("Content-Type", "video/x-ms-wmv"),
            ("Content-Length", "90000")
        ]
    ));
}

/// Reference A's list lines (gaps row 72a), from its panel on a YouTube
/// video watched on 2026-11-04.
#[test]
fn a_quality_line_is_worded_as_reference_a_words_it() {
    let title = "What Happened To FIRST YOUTUBE Video Guy";
    assert_eq!(
        reference_line(2, title, "720p"),
        "2. What Happened To FIRST YOUTUBE Video Guy… MP4 file, quality 720p HD"
    );
    assert_eq!(
        reference_line(4, title, "480p"),
        "4. What Happened To FIRST YOUTUBE Video Guy… MP4 file, quality 480p"
    );
    // A page's own line is kept; a long title is cut at forty characters.
    let long = "A title that runs on far past the forty characters kept";
    assert_eq!(
        reference_line(1, long, "MP4 file, quality SD"),
        "1. A title that runs on far past the forty… MP4 file, quality SD"
    );
    // With its size, once dm's reading has come.
    assert_eq!(
        reference_line(1, "Clip", "1080p — 50.1 MB"),
        "1. Clip… MP4 file, quality 1080p HD — 50.1 MB"
    );
    // dm's own reading names the container.
    assert_eq!(
        reference_line(1, "Clip", "1080p MP4 — 252 MB"),
        "1. Clip… MP4 file, quality 1080p HD — 252 MB"
    );
    assert_eq!(
        reference_line(2, "Clip", "360p WebM"),
        "2. Clip… WebM file, quality 360p"
    );
    // A line of more words is not a quality to reword.
    assert_eq!(
        reference_line(1, "Clip", "720p video + sound"),
        "1. Clip… 720p video + sound"
    );
    // No title: the number and the line alone.
    assert_eq!(reference_line(3, "  ", "HLS stream"), "3. HLS stream");
    // A height at two frame rates: the faster line says its rate after the
    // quality, as reference A's "720p HD 60 fps" (gaps row 72d).
    assert_eq!(
        reference_line(1, "Clip", "720p MP4 60 fps — 14 MB"),
        "1. Clip… MP4 file, quality 720p HD 60 fps — 14 MB"
    );
    assert_eq!(
        reference_line(2, "Clip", "720p MP4 — 9 MB"),
        "2. Clip… MP4 file, quality 720p HD — 9 MB"
    );
}

/// The words a page gives its links go with the links sent, keyed as they
/// are, spaces closed up; words for an address not sent are dropped.
#[test]
fn a_links_words_are_kept_for_the_link_sent() {
    let urls = vec![
        "https://e.com/a.zip".to_string(),
        "https://e.com/b.iso".to_string(),
    ];
    let texts: std::collections::BTreeMap<String, String> = [
        ("https://e.com/a.zip#mirror", "  The   A\n archive "),
        ("https://e.com/b.iso", ""),
        ("mailto:me@e.com", "Write to us"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let kept = link_texts(&texts, &urls);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept["https://e.com/a.zip"], "The A archive");
    let long: std::collections::BTreeMap<String, String> =
        [("https://e.com/a.zip".to_string(), "x".repeat(500))]
            .into_iter()
            .collect();
    assert_eq!(link_texts(&long, &urls)["https://e.com/a.zip"].len(), 200);
}

/// A link's `download` attribute names its file: the name's last part only
/// (a page does not choose the folder), for a link sent, and nothing for a
/// name that is none.
#[test]
fn a_links_download_attribute_names_its_file_and_never_its_folder() {
    let urls = vec![
        "https://e.com/get?id=7".to_string(),
        "https://e.com/b.iso".to_string(),
        "https://e.com/c.zip".to_string(),
    ];
    let given: std::collections::BTreeMap<String, String> = [
        ("https://e.com/get?id=7#x", "Report 2026.pdf"),
        ("https://e.com/b.iso", "../../Windows\\system32\\evil.dll"),
        ("https://e.com/c.zip", " .. "),
        ("https://e.com/not-sent.zip", "kept.zip"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let names = link_names(&given, &urls);
    assert_eq!(names.len(), 2, "{names:?}");
    assert_eq!(names["https://e.com/get?id=7"], "Report 2026.pdf");
    assert_eq!(names["https://e.com/b.iso"], "evil.dll");
}

/// A form a page sent is kept for the download it starts at the same address,
/// once; not for one sent on to another address, nor after two minutes.
#[test]
fn a_forms_body_goes_with_the_download_that_is_its_answer() {
    let form = |text: &str| dm_protocol::Posted {
        form: text.into(),
        ..dm_protocol::Posted::default()
    };
    let mut forms = Forms::default();
    forms.keep("https://e.com/export#top", form("report=7"), 1_000.0);
    // Sent on to another address: that one is asked for plainly.
    assert!(forms
        .take("https://e.com/export", "https://cdn.e.com/r.csv", 2_000.0)
        .is_none());
    let taken = forms.take("https://e.com/export", "https://e.com/export", 2_000.0);
    assert_eq!(taken.map(|p| p.form), Some("report=7".to_string()));
    assert!(
        forms.take("https://e.com/export", "", 2_500.0).is_none(),
        "taken twice"
    );
    forms.keep("https://e.com/export", form("report=8"), 3_000.0);
    assert!(
        forms
            .take("https://e.com/export", "", 3_000.0 + FORM_KEPT_MS)
            .is_none(),
        "kept too long"
    );
    for n in 0..30 {
        forms.keep(&format!("https://e.com/{n}"), form("x"), 4_000.0);
    }
    assert!(
        forms.take("https://e.com/0", "", 4_000.0).is_none(),
        "more than twenty kept"
    );
    assert!(forms.take("https://e.com/29", "", 4_000.0).is_some());
}

/// The menu items Options lists are the ones the extension makes, by id and
/// words, so leaving one out in Options leaves out that one (gaps row 86).
#[test]
fn the_menu_items_options_lists_are_the_ones_made() {
    let made: Vec<(&str, &str)> = MENUS.iter().map(|(id, title, _)| (*id, *title)).collect();
    assert_eq!(made, dm_protocol::MENU_ITEMS.to_vec());
}

#[test]
fn a_streams_line_says_how_long_it_plays() {
    // The words, as reference A's lines say a file's length.
    assert_eq!(length_words(41.4).as_deref(), Some("41 sec"));
    assert_eq!(length_words(252.0).as_deref(), Some("4 min 12 sec"));
    assert_eq!(length_words(600.0).as_deref(), Some("10 min"));
    assert_eq!(length_words(3725.0).as_deref(), Some("1 h 2 min"));
    assert_eq!(length_words(7200.0).as_deref(), Some("2 h"));
    assert_eq!(length_words(0.2), None);
    assert_eq!(length_words(f64::NAN), None);

    // An HLS media playlist that ends: its segments summed; a live one has no length.
    let vod = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\na.ts\n#EXTINF:5.5,\nb.ts\n#EXT-X-ENDLIST\n";
    assert_eq!(hls_length(vod), Some(11.5));
    assert_eq!(hls_length("#EXTM3U\n#EXTINF:6,\na.ts\n"), None);
    assert_eq!(hls_length("#EXTM3U\n#EXT-X-ENDLIST\n"), None);

    // A DASH manifest's own duration; a live one names none.
    let mpd = r#"<MPD type="static" mediaPresentationDuration="PT4M12.5S"><Period/></MPD>"#;
    assert_eq!(dash_length(mpd), Some(252.5));
    assert_eq!(
        dash_length(r#"<MPD mediaPresentationDuration="P1DT1H"/>"#),
        Some(90_000.0)
    );
    assert_eq!(dash_length(r#"<MPD type="dynamic"><Period/></MPD>"#), None);
    assert_eq!(
        dash_length(r#"<MPD mediaPresentationDuration="P1Y"/>"#),
        None
    );

    // A master playlist's line takes its length from a rendition, once.
    let mut streams = Streams::default();
    streams.absorb(&playlist_seen(VIMEO_URL));
    assert!(streams.describe(7, VIMEO_URL, VIMEO_MASTER));
    let rendition = streams
        .rendition_for_length(7, VIMEO_URL)
        .expect("a rendition to read");
    assert!(rendition.starts_with("https://"), "{rendition}");
    assert!(streams.lengthen(7, VIMEO_URL, 252.0));
    assert_eq!(streams.of(7)[0].label, "1080p HLS stream, 4 min 12 sec");
    assert!(!streams.lengthen(7, VIMEO_URL, 99.0), "once");
    assert_eq!(streams.rendition_for_length(7, VIMEO_URL), None);

    // A DASH manifest says both at once.
    let mut streams = Streams::default();
    streams.absorb(&seen(
        "https://cdn.e.com/v/manifest.mpd",
        "xmlhttprequest",
        &[("Content-Type", "application/dash+xml")],
    ));
    let mpd = r#"<MPD mediaPresentationDuration="PT41S"><Period><AdaptationSet contentType="video"><Representation height="720" bandwidth="2000000"/></AdaptationSet></Period></MPD>"#;
    assert!(streams.describe(7, "https://cdn.e.com/v/manifest.mpd", mpd));
    assert_eq!(streams.of(7)[0].label, "720p DASH stream, 41 sec");

    // A media playlist that ends keeps its name and says its length.
    let mut streams = Streams::default();
    streams.absorb(&playlist_seen("https://e.com/media.m3u8"));
    assert!(streams.describe(7, "https://e.com/media.m3u8", vod));
    assert_eq!(streams.of(7)[0].label, "HLS stream, 12 sec");
}

#[test]
fn a_playlist_is_read_again_from_the_public_web_and_from_a_private_host_only_for_its_own_page() {
    // The public web: whoever asked.
    assert!(may_reread(
        "https://cdn.example/master.m3u8",
        "https://video.example"
    ));
    assert!(may_reread("https://cdn.example/master.m3u8", ""));
    // A page of this computer's (a check's local server) loading its own playlist.
    assert!(may_reread(
        "http://127.0.0.1:8123/master.m3u8",
        "http://127.0.0.1:8123"
    ));
    // A public page aiming the worker at this computer, a router or a link-local address.
    for inside in [
        "http://127.0.0.1:8123/m.m3u8",
        "http://localhost/m.m3u8",
        "http://192.168.1.1/cgi?a=1",
        "http://10.0.0.5/m.mpd",
        "http://172.20.1.1/m.mpd",
        "http://169.254.169.254/latest/meta-data",
        "http://[::1]:80/m.m3u8",
        "http://[fd00::1]/m.m3u8",
        "http://[::ffff:192.168.0.1]/m.m3u8",
        "http://printer.local/m.m3u8",
        "http://100.64.0.1/m.m3u8",
    ] {
        assert!(!may_reread(inside, "https://evil.example"), "{inside}");
        assert!(!may_reread(inside, ""), "{inside}");
    }
    // A rendition a private master names on another private host.
    assert!(!may_reread(
        "http://192.168.1.1/x",
        "http://127.0.0.1:8123/master.m3u8"
    ));
}

/// The first bytes of an MP4 holding one track: `ftyp`, then a `moov` whose `trak` has a
/// track header (with `height` in its last four bytes) and a handler of `handler`.
fn mp4_start(handler: &[u8; 4], height: u32) -> Vec<u8> {
    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }
    let mut tkhd = vec![0u8; 84];
    tkhd[80..84].copy_from_slice(&(height << 16).to_be_bytes());
    let mut hdlr = vec![0u8; 8];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0u8; 13]);
    let mdia = boxed(b"mdia", &boxed(b"hdlr", &hdlr));
    let trak = boxed(b"trak", &[boxed(b"tkhd", &tkhd), mdia].concat());
    let moov = boxed(b"moov", &[boxed(b"mvhd", &[0u8; 100]), trak].concat());
    [boxed(b"ftyp", b"iso5\0\0\x02\0iso5iso6mp41dash"), moov].concat()
}

#[test]
fn an_mp4s_first_bytes_say_whether_it_is_a_picture_or_a_sound() {
    assert_eq!(
        mp4_track(&mp4_start(b"vide", 480)),
        Some((false, Some(480)))
    );
    assert_eq!(mp4_track(&mp4_start(b"soun", 0)), Some((true, None)));
    // Cut before the handler, not an MP4 at all, or a handler of neither kind.
    let whole = mp4_start(b"vide", 480);
    assert_eq!(mp4_track(&whole[..whole.len() - 20]), None);
    assert_eq!(mp4_track(b"<!doctype html><title>x</title>"), None);
    assert_eq!(mp4_track(&mp4_start(b"text", 0)), None);
    assert_eq!(mp4_track(&[]), None);
}

fn ranged_answer(url: &str, mime: &str, range: &str) -> Seen {
    Seen {
        url: url.into(),
        tab_id: 3,
        kind: "xmlhttprequest".into(),
        status_code: 206,
        response_headers: vec![
            Header {
                name: "Content-Type".into(),
                value: Some(mime.into()),
            },
            Header {
                name: "Content-Range".into(),
                value: Some(range.into()),
            },
        ],
        ..Default::default()
    }
}

/// Bilibili's player (gaps row 155): a picture and a sound, each one whole `.m4s` file fetched
/// in ranges its requests' `Range` header asks for, both answered `206 video/mp4`. Once the
/// sound's first bytes are read, they are one line, as a picture and a sound fetched apart are.
#[test]
fn a_picture_and_a_sound_fetched_whole_by_ranged_requests_become_one_line_once_read() {
    let picture =
        "https://upos-hz.akamaized.net/upgcxcode/77/66/4336/4336-1-30032.m4s?e=ig8e&deadline=1";
    let sound =
        "https://upos-hz.akamaized.net/upgcxcode/77/66/4336/4336-1-30216.m4s?e=ig8e&deadline=1";
    let mut streams = Streams::default();
    streams.absorb(&ranged_answer(
        picture,
        "video/mp4",
        "bytes 1024-200000/5651664",
    ));
    streams.absorb(&ranged_answer(
        sound,
        "video/mp4",
        "bytes 1024-90000/547149",
    ));
    // Both said to be video: before they are read, the larger is a file on its own.
    assert!(streams.wants_track_read(3, picture));
    assert!(streams.wants_track_read(3, sound));
    assert!(!streams.wants_track_read(3, sound), "read once");
    assert!(streams.read_track(3, picture, false, Some(480)));
    assert!(streams.read_track(3, sound, true, None));
    let lines = streams.of(3);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].label, "480p video file + sound");
    assert_eq!(lines[0].url, picture);
    assert_eq!(lines[0].sound.as_deref(), Some(sound));
    assert_eq!(lines[0].size, Some(5651664 + 547149));
}

#[test]
fn whole_files_by_ranges_are_not_lines_beside_a_manifest_nor_change_a_plain_mp4() {
    // A page whose DASH manifest names single-file renditions fetched by range.
    let mut streams = Streams::default();
    let mut manifest = seen(
        "https://cdn.example/v/manifest.mpd",
        "xmlhttprequest",
        &[("Content-Type", "application/dash+xml")],
    );
    manifest.tab_id = 3;
    streams.absorb(&manifest);
    streams.absorb(&ranged_answer(
        "https://cdn.example/v/video-720.m4s",
        "video/mp4",
        "bytes 900-90000/9000000",
    ));
    let lines = streams.of(3);
    assert!(lines.iter().all(|s| !s.by_ranges), "{lines:?}");
    // A plain MP4 a <video> fetches from its first byte stays the file it always was.
    let mut plain = ranged_answer(
        "https://cdn.example/clip.mp4",
        "video/mp4",
        "bytes 0-/9000000",
    );
    plain.kind = "media".into();
    let found = stream_seen(&plain).unwrap();
    assert_eq!((found.kind, found.by_ranges), (StreamKind::File, false));
    assert_eq!(found.label, "MP4 video file");
    // A whole 200 answer of a segment is still a segment; so is a tiny one.
    let mut whole = ranged_answer("https://cdn.example/v/seg-1.m4s", "video/mp4", "");
    whole.status_code = 200;
    whole.response_headers.pop();
    assert!(stream_seen(&whole).is_none());
    let tiny = ranged_answer(
        "https://cdn.example/v/a.m4s",
        "video/mp4",
        "bytes 10-20/30000",
    );
    assert!(stream_seen(&tiny).is_none());
}
