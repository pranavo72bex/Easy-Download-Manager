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
    assert!(page.page, "a player page goes to the app's yt-dlp");

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
    json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "0.1.0", "ytdlp": true })
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
    assert_eq!(hello.ytdlp, Some(true));
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
                ytdlp: Some(true),
                protocol: None,
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
        "ytdlp": false, "protocol": 2,
    }));
    let App::Present { hello: new, .. } = probe.give_up("no rules") else {
        panic!("not present")
    };
    assert_eq!(takes_caught(&new), Ok(()));
    let (line, bad) = status_line(&new);
    assert!(!bad && line.contains("update the dm app"), "{line}");
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
    assert_eq!(youtube_video_id("https://m.youtube.com/shorts/abc-_123?x=1").as_deref(), Some("abc-_123"));
    assert_eq!(youtube_video_id("https://www.youtube.com/playlist?list=PL1"), None);
    assert_eq!(youtube_video_id("https://www.video.example/watch?v=abc"), None);
    assert_eq!(youtube_video_id("https://www.youtube.com/watch?list=PL1"), None);
    let levels: Vec<String> = ["auto", "hd1080", "hd720", "hd1080", "large", "medium", "tiny", "odd"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(player_heights(&levels), [1080, 720, 480, 360, 144]);
}

