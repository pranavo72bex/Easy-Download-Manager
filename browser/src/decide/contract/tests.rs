// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The extension's side of what it and the app say to each other (gaps row A22), down to each
//! way out of each decision: an app updated ahead of the extension, or behind it, must never
//! leave a download, a menu click or the panel's list without an answer.

use super::*;
use dm_protocol::{feature, menu_does, MenuItem, Notice, StatusLine};
use serde_json::json;

fn hello_with(features: &[&str]) -> Hello {
    Hello {
        engine_version: "0.11.0".into(),
        protocol: Some(2),
        features: features.iter().map(|f| f.to_string()).collect(),
        ..Hello::default()
    }
}

/* ------------------------------------------------------------- answers */

#[test]
fn a_hand_off_answer_is_read_with_or_without_the_apps_words() {
    let started =
        json!({ "protocolVersion": 1, "type": "download.started", "id": "a", "shown": true });
    assert_eq!(taken_said(&started, "a"), Some((Ok(true), None)));
    assert_eq!(taken(&started, "a"), Some(Ok(true)));
    // Another hand-off's answer, or another kind of frame, is not this one's.
    assert_eq!(taken_said(&started, "b"), None);
    let hello = json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "1" });
    assert_eq!(taken_said(&hello, "a"), None);
    assert_eq!(
        taken_said(&json!({ "type": "download.started" }), "a"),
        None
    );
    // Words that say nothing are none: the extension's own are said.
    let blank = json!({
        "protocolVersion": 1, "type": "download.started", "id": "a",
        "notice": { "title": " ", "body": "" }
    });
    assert_eq!(taken_said(&blank, "a"), Some((Ok(false), None)));
    let body_only = json!({
        "protocolVersion": 1, "type": "download.started", "id": "a", "notice": { "body": "x" }
    });
    assert_eq!(
        taken_said(&body_only, "a").unwrap().1,
        Some(Notice {
            title: String::new(),
            body: "x".into()
        })
    );
    // A refusal with no reason, or an empty one, still says something.
    for failure in [json!(null), json!({ "message": "" })] {
        let failed = json!({ "protocolVersion": 1, "type": "download.failed", "id": "a", "failure": failure });
        assert_eq!(
            taken(&failed, "a"),
            Some(Err("dm would not take it".into()))
        );
    }
}

#[test]
fn a_catch_verdict_is_read_for_its_own_question_only() {
    let take = json!({ "protocolVersion": 1, "type": "catch.judged", "id": "j", "take": true });
    assert_eq!(judged(&take, "j"), Some(Ok(())));
    assert_eq!(judged(&take, "k"), None);
    for reason in [json!(null), json!(""), json!("  ")] {
        let leave = json!({ "protocolVersion": 1, "type": "catch.judged", "id": "j", "take": false, "reason": reason });
        assert_eq!(
            judged(&leave, "j"),
            Some(Err("dm's settings leave it with the browser".into()))
        );
    }
    let started = json!({ "protocolVersion": 1, "type": "download.started", "id": "j" });
    assert_eq!(
        judged(&started, "j"),
        None,
        "a hand-off's answer is no verdict"
    );
    assert_eq!(judged(&json!({ "type": "catch.judged" }), "j"), None);
}

#[test]
fn the_panels_list_from_the_app_is_read_and_nothing_else_is_taken_for_it() {
    let listed = json!({
        "protocolVersion": 3, "type": "media.streams",
        "streams": [{ "url": "https://c.example/a.m3u8", "kind": "Hls", "label": "HLS stream", "more": 1 }]
    });
    assert_eq!(streams_listed(&listed).unwrap()[0].kind, StreamKind::Hls);
    let empty = json!({ "protocolVersion": 1, "type": "media.streams" });
    assert_eq!(streams_listed(&empty), Some(Vec::new()));
    let formats = json!({ "protocolVersion": 1, "type": "media.formats" });
    assert_eq!(streams_listed(&formats), None);
    assert_eq!(streams_listed(&json!({ "type": "media.streams" })), None);
}

#[test]
fn the_question_about_a_caught_download_carries_what_the_rules_read() {
    let found = Catchable {
        id: 7,
        url: "https://cdn.example/a.zip".into(),
        asked_url: "https://example.com/get".into(),
        referrer: "https://example.com/".into(),
        filename: "/home/a/a.zip".into(),
        mime: "application/zip".into(),
        size: Some(42),
        store: Some("firefox-container-1".into()),
    };
    let judge = judge_of(&found, "j7-1", "Firefox", true);
    assert_eq!(judge.id, "j7-1");
    assert_eq!(judge.browser, "Firefox");
    assert!(judge.every_type);
    assert_eq!(judge.candidate(), found.candidate());
    // What it is asked by is what the app answers by: the same rules give the same verdict
    // here and there.
    let rules = CatchRules {
        min_size_kb: 1,
        ..CatchRules::default()
    };
    assert_eq!(
        judge.judged_by(&rules).is_ok(),
        rules.takes(&found.candidate(), true).is_ok()
    );
    let frame = dm_protocol::Request::Judge(judge).to_frame();
    assert_eq!(frame["type"], dm_protocol::kind::JUDGE);
    assert_eq!(
        frame["protocolVersion"], 1,
        "every app that says judges-catch reads it"
    );
    assert!(
        frame.get("store").is_none(),
        "the container is the browser's, not sent"
    );
}

/* ------------------------------------------------------------ the popup */

#[test]
fn the_popup_says_the_apps_line_only_where_it_sent_one() {
    let mut hello = hello_with(&[]);
    let own = status_line(&hello);
    assert!(!own.1);
    hello.status = Some(StatusLine {
        line: "  ".into(),
        bad: true,
    });
    assert_eq!(status_line(&hello), own, "a blank line is not said");
    hello.status = Some(StatusLine {
        line: "dm 9 is updating".into(),
        bad: true,
    });
    assert_eq!(status_line(&hello), ("dm 9 is updating".into(), true));
    // An app too old to judge pages is said as that, in the extension's words.
    let old = Hello {
        protocol: None,
        ..hello_with(&[])
    };
    assert!(status_line(&old).1);
}

#[test]
fn a_mismatched_app_is_named_in_the_popup_as_the_side_to_update() {
    let said = absent_popup_text(&format!("{MISMATCH}update the dm app"), "id", false);
    assert_eq!(said, "update the dm app");
}

/* ------------------------------------------------------------ the probe */

#[test]
fn the_probe_ignores_what_it_does_not_need_and_what_it_cannot_read() {
    let mut probe = Probe::default();
    probe.absorb(&json!({ "protocolVersion": 1, "type": "catch.judged", "id": "j", "take": true }));
    probe.absorb(&json!({ "protocolVersion": 1, "type": "app.something" }));
    assert!(!probe.has_hello());
    assert_eq!(probe.finished(), None);
    probe.absorb(
        &json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "1",
        "protocol": 2, "features": ["menus"], "menus": [] }),
    );
    probe.absorb(&json!({ "protocolVersion": 1, "type": "settings.get", "catch": {} }));
    let App::Present { hello, .. } = probe.give_up("closed") else {
        panic!("a whole answer is an app")
    };
    assert_eq!(hello.menus, Some(Vec::new()), "an empty menu is a menu");
    assert!(hello.has(feature::MENUS));
}

#[test]
fn this_extension_says_every_ability_it_has_and_no_other() {
    let said = greeting();
    assert_eq!(said.extension_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        said.features,
        [
            feature::READS_ANY_REPLY,
            feature::RENEWS,
            feature::MENUS,
            feature::JUDGES_CATCH,
            feature::SAYS,
            feature::LISTS_STREAMS,
        ]
    );
}

/* ------------------------------------------------------------- the menu */

fn item(id: &str, does: &str, contexts: &[&str]) -> MenuItem {
    MenuItem {
        id: id.into(),
        title: format!("{id} words"),
        contexts: contexts.iter().map(|c| c.to_string()).collect(),
        does: does.into(),
    }
}

#[test]
fn the_menu_made_is_the_apps_less_what_cannot_be_made() {
    let served = vec![
        item("dm-link", menu_does::LINK, &["link"]),
        item("", menu_does::APP, &["page"]),
        item("dm-nowhere", menu_does::APP, &[]),
        item("dm-later", "teleport", &["page"]),
        item("dm-image", menu_does::APP, &["image"]),
    ];
    let made = menus_to_make(Some(&served), &[]);
    let ids: Vec<&str> = made.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["dm-link", "dm-image"]);
    assert_eq!(
        made[0].known.map(|w| w.key),
        Some(crate::words::MENU_LINK.key)
    );
    assert_eq!(made[1].known, None);
    assert_eq!(made[1].title, "dm-image words");
    assert_eq!(made[1].contexts, ["image"]);
    // An app that served nothing in its menu: nothing is made.
    assert!(menus_to_make(Some(&[]), &[]).is_empty());
    // An app that serves none: this extension's own, every one unless hidden, in its words.
    let own = menus_to_make(None, &[]);
    assert_eq!(own.len(), MENUS.len());
    assert!(own.iter().all(|m| m.known.is_some() && !m.title.is_empty()));
    assert!(menus_to_make(
        None,
        &MENUS.iter().map(|m| m.0.to_string()).collect::<Vec<_>>()
    )
    .is_empty());
}

#[test]
fn a_click_does_what_the_app_said_or_what_this_extension_always_did() {
    let served = vec![
        item("dm-link", menu_does::MEDIA, &["link"]),
        item("dm-later", "teleport", &["page"]),
    ];
    assert_eq!(
        menu_does_for(Some(&served), "dm-link"),
        Some(menu_does::MEDIA)
    );
    assert_eq!(menu_does_for(Some(&served), "dm-later"), None);
    assert_eq!(menu_does_for(Some(&served), "dm-gone"), None);
    for (id, does) in [
        (MENU_LINK, menu_does::LINK),
        (MENU_MEDIA, menu_does::MEDIA),
        (MENU_ALL_LINKS, menu_does::ALL_LINKS),
        (MENU_SELECTED_LINKS, menu_does::SELECTED_LINKS),
    ] {
        assert_eq!(menu_does_for(None, id), Some(does));
    }
    assert_eq!(menu_does_for(None, "dm-new"), None);
}

fn click(value: serde_json::Value) -> MenuClick {
    serde_json::from_value(value).unwrap()
}

#[test]
fn an_item_by_its_mechanism_hands_over_what_its_id_used_to() {
    let on_link = click(
        json!({ "menuItemId": "dm-whatever", "linkUrl": "https://e.com/a.zip",
        "pageUrl": "https://e.com/" }),
    );
    let target = menu_target_doing(&on_link, menu_does::LINK, false).unwrap();
    assert_eq!(
        (target.url.as_str(), target.page),
        ("https://e.com/a.zip", false)
    );
    // The old entry point: by id, as an app that serves no menu has it.
    let by_id = click(json!({ "menuItemId": MENU_LINK, "linkUrl": "https://e.com/a.zip" }));
    assert_eq!(
        menu_target(&by_id, false).unwrap().url,
        "https://e.com/a.zip"
    );
    let unknown = click(json!({ "menuItemId": "dm-new", "linkUrl": "https://e.com/a.zip" }));
    assert!(menu_target(&unknown, false).unwrap_err().contains("dm-new"));
    assert!(menu_target_doing(&on_link, menu_does::APP, false).is_err());
    assert!(
        menu_target_doing(&on_link, menu_does::LINK, true).is_err(),
        "never a private window"
    );
    // A player's own address is its page, read for its video.
    let blob = click(json!({ "menuItemId": "x", "srcUrl": "blob:https://e.com/1",
        "pageUrl": "https://e.com/watch", "frameUrl": "https://player.e.com/embed" }));
    let page = menu_target_doing(&blob, menu_does::MEDIA, false).unwrap();
    assert_eq!(
        (page.url.as_str(), page.page),
        ("https://player.e.com/embed", true)
    );
}

#[test]
fn a_click_for_the_apps_own_item_carries_the_browsers_words_and_is_held_to_size() {
    let long = "w".repeat(5000);
    let c = click(json!({
        "menuItemId": "dm-new", "linkUrl": " https://e.com/a ", "pageUrl": "https://e.com/",
        "frameUrl": "", "selectionText": long, "mediaType": "image", "srcUrl": "data:x"
    }));
    let asked = menu_clicked(&c, "c1", &"t".repeat(400));
    assert_eq!(asked.id, "c1");
    assert_eq!(asked.item, "dm-new");
    assert_eq!(asked.link_url, "https://e.com/a");
    assert_eq!(asked.selection_text.chars().count(), 2000);
    assert_eq!(asked.title.chars().count(), 300);
    assert!(
        asked.cookies.is_empty(),
        "the sign-in is added by the web side, by the person's choice"
    );
    assert_eq!(menu_clicked_address(&asked), "https://e.com/a");
    // The first web address of the link, the media, the frame and the page.
    let on_page = menu_clicked(
        &click(json!({ "menuItemId": 7, "pageUrl": "https://e.com/p" })),
        "c2",
        "",
    );
    assert_eq!(on_page.item, "", "an id that is not text names no item");
    assert_eq!(menu_clicked_address(&on_page), "https://e.com/p");
    let nowhere = menu_clicked(
        &click(json!({ "menuItemId": "x", "pageUrl": "chrome://newtab" })),
        "c3",
        "",
    );
    assert_eq!(menu_clicked_address(&nowhere), "");
    let frame = dm_protocol::Request::MenuClicked(asked).to_frame();
    assert_eq!(frame["type"], dm_protocol::kind::MENU_CLICKED);
    assert_eq!(frame["protocolVersion"], 1);
}

/* ------------------------------------------------------------ site rules */

#[test]
fn the_served_site_rules_name_each_sites_finders_title_and_private_fetches() {
    let rules: dm_protocol::Rules = serde_json::from_value(json!({
        "format": dm_protocol::RULES_FORMAT,
        "sites": [{
            "host": "video.example",
            "finders": [{ "player": "div.player", "id_attr": "data-id" }],
            "private_fetches": ["window|a.b|fetch", "bad"],
            "title": "h1.title",
            "answers": [{ "url_contains": "/api/video", "keys": ["url"] }]
        }]
    }))
    .unwrap();
    rules::install(Some(rules));
    let page = "https://video.example/watch/1";
    assert_eq!(rules::finders_for(page).len(), 1);
    assert_eq!(rules::title_for(page), "h1.title");
    assert_eq!(
        rules::private_fetches_for(page),
        ["window|a.b|fetch"],
        "tidied as served"
    );
    assert_eq!(rules::watched_for(page), ["/api/video"]);
    // Another site, or no site at all, has none.
    for other in ["https://other.example/", "not an address"] {
        assert!(rules::finders_for(other).is_empty());
        assert_eq!(rules::title_for(other), "");
        assert!(rules::private_fetches_for(other).is_empty());
        assert!(rules::watched_for(other).is_empty());
    }
    // A document of another format is not used: the built-in tables alone decide.
    rules::install(Some(dm_protocol::Rules {
        format: dm_protocol::RULES_FORMAT + 1,
        ..dm_protocol::Rules::default()
    }));
    assert!(!rules::installed());
    assert!(rules::finders_for(page).is_empty());
    rules::install(None);
}

/* ----------------------------------------------------------- headers kept */

fn sent(url: &str, tab: i64, headers: &[(&str, &str)]) -> Sent {
    serde_json::from_value(json!({
        "url": url, "type": "xmlhttprequest", "tabId": tab,
        "requestHeaders": headers.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>()
    }))
    .unwrap()
}

#[test]
fn headers_kept_by_an_earlier_worker_are_merged_older_than_what_was_heard_since() {
    let mut now = RequestHeaders::default();
    now.keep(&sent(
        "https://a.example/1",
        3,
        &[("Authorization", "Bearer new"), ("Cookie", "s=1")],
    ));
    let older = vec![
        (
            "https://a.example/1".to_string(),
            vec![("Authorization".to_string(), "Bearer old".to_string())],
            Some(3),
        ),
        (
            "https://b.example/2".to_string(),
            vec![("X-Key".into(), "k".into())],
            Some(3),
        ),
        (
            "ftp://c.example/3".to_string(),
            vec![("X-Key".into(), "k".into())],
            None,
        ),
        (
            "https://d.example/4".to_string(),
            vec![("X".to_string(), "y".to_string()); 41],
            None,
        ),
    ];
    now.merge(older);
    let kept = now.export();
    let urls: Vec<&str> = kept.iter().map(|k| k.0.as_str()).collect();
    assert_eq!(
        urls,
        ["https://b.example/2", "https://a.example/1"],
        "older first, junk dropped"
    );
    assert_eq!(
        kept[1].1[0].1, "Bearer new",
        "an address heard again keeps its newer headers"
    );
    // As many as it keeps, the oldest going first.
    let many: Vec<_> = (0..250)
        .map(|n| {
            (
                format!("https://m.example/{n}"),
                vec![("X-K".to_string(), "v".to_string())],
                None,
            )
        })
        .collect();
    now.merge(many);
    let kept = now.export();
    assert_eq!(kept.len(), 200);
    assert_eq!(kept.last().unwrap().0, "https://a.example/1");
    // The hosts reached, merged the same way.
    let reached = now.export_reached();
    let mut fresh = RequestHeaders::default();
    fresh.keep(&sent("https://a.example/9", 3, &[("Cookie", "t=2")]));
    fresh.merge_reached(reached);
    assert!(fresh.reached(Some(3), "https://a.example/x"));
    assert_eq!(fresh.sent_cookies(Some(3), "https://a.example/x"), ["t"]);
    assert!(!fresh.reached(None, "https://a.example/x"));
    assert!(fresh
        .sent_cookies(Some(4), "https://a.example/x")
        .is_empty());
}

/* -------------------------------------------------------------- streams */

#[test]
fn a_response_names_the_page_that_asked_as_its_browser_says_it() {
    let chrome = Seen {
        initiator: "https://e.com".into(),
        origin_url: "https://ignored.example/".into(),
        ..Seen::default()
    };
    assert_eq!(chrome.requester(), "https://e.com");
    let firefox = Seen {
        origin_url: "https://e.com/page".into(),
        ..Seen::default()
    };
    assert_eq!(firefox.requester(), "https://e.com/page");
}

#[test]
fn a_private_address_is_read_again_only_for_its_own_host() {
    for inside in [
        "http://localhost/a.m3u8",
        "http://printer.local/a.m3u8",
        "http://10.0.0.1/a.m3u8",
        "http://100.64.0.1/a.m3u8",
        "http://[::1]/a.m3u8",
        "http://[fd00::1]/a.m3u8",
        "http://[fe80::1]/a.m3u8",
        "http://[::ffff:192.168.1.1]/a.m3u8",
        "http:///a.m3u8",
    ] {
        assert!(!may_reread(inside, "https://e.com/"), "{inside}");
    }
    assert!(may_reread("http://10.0.0.1/a.m3u8", "http://10.0.0.1/page"));
    assert!(may_reread("https://cdn.example/a.m3u8", "https://e.com/"));
    assert!(may_reread("http://[2001:db8::1]/a.m3u8", "https://e.com/"));
}

#[test]
fn a_tab_is_known_once_heard_and_a_restore_keeps_as_many_as_a_tab_keeps() {
    let mut streams = Streams::default();
    assert!(!streams.knows(1));
    let line = |n: usize| Stream {
        url: format!("https://c.example/{n}.mp4"),
        kind: StreamKind::File,
        size: Some(2 << 20),
        label: format!("file {n}"),
        sound: None,
        members: Vec::new(),
        length: None,
        by_ranges: false,
    };
    streams.restore(1, (0..12).map(line).collect());
    assert!(streams.knows(1));
    let raw = streams.raw(1);
    assert_eq!(raw.len(), STREAMS_PER_TAB);
    assert_eq!(raw[0].url, line(0).url, "the first stays");
    assert_eq!(streams.of(1), panel_list(raw), "the list is the shared one");
    streams.forget(1);
    assert!(!streams.knows(1));
}

/* ---------------------------------------------------------------- words */

#[test]
fn a_dollar_that_names_no_gap_is_said_as_it_is() {
    let word = crate::words::Word {
        key: "k",
        en: "costs $5 and $NAME$, $nope$ $",
        places: &["name"],
    };
    assert_eq!(word.english(&["dm"]), "costs $5 and dm, $nope$ $");
    assert_eq!(word.english(&[]), "costs $5 and , $nope$ $");
}
