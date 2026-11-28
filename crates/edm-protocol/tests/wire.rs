// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A released extension keeps working with every later app: the frames each released extension
//! sends (`tests/wire/extension-*.json`, never edited) are read by this side whole, nothing they
//! carry is dropped, and what this side replies reads as the replies that extension knew.

use edm_protocol::{feature, Greeting, Hello, Reply, Request};
use serde_json::{json, Value};

fn released() -> Vec<(&'static str, Value)> {
    [
        (
            "extension-3.4.0.json",
            include_str!("wire/extension-3.4.0.json"),
        ),
        (
            "extension-3.5.0.json",
            include_str!("wire/extension-3.5.0.json"),
        ),
        (
            "extension-3.6.0.json",
            include_str!("wire/extension-3.6.0.json"),
        ),
    ]
    .into_iter()
    .map(|(name, text)| (name, serde_json::from_str(text).expect(name)))
    .collect()
}

fn frames<'a>(file: &'a Value, key: &str) -> &'a [Value] {
    file[key].as_array().expect("a list of frames")
}

/// Whether `read` holds everything `sent` does: the same values, an object's keys each held in
/// turn (`read` may hold more, as a newer side writes more).
fn holds(read: &Value, sent: &Value) -> bool {
    match (read, sent) {
        (Value::Object(read), Value::Object(sent)) => sent
            .iter()
            .all(|(k, v)| read.get(k).is_some_and(|r| holds(r, v))),
        // A list holds what was sent item by item, in order: a newer side may say more of each.
        (Value::Array(read), Value::Array(sent)) => {
            read.len() == sent.len() && read.iter().zip(sent).all(|(r, s)| holds(r, s))
        }
        _ => read == sent,
    }
}

/// Keys in `sent` whose value `read` lost or changed.
fn lost(sent: &Value, read: &Value) -> Vec<String> {
    sent.as_object()
        .expect("a frame is an object")
        .iter()
        .filter(|(k, v)| !read.get(k.as_str()).is_some_and(|r| holds(r, v)))
        .map(|(k, _)| k.clone())
        .collect()
}

#[test]
fn every_frame_a_released_extension_sends_is_read_whole() {
    for (name, file) in released() {
        for sent in frames(&file, "requests") {
            let read = Request::from_frame(sent)
                .unwrap_or_else(|e| panic!("{name}: {sent} is not read: {e}"));
            let again = read.to_frame();
            assert!(
                lost(sent, &again).is_empty(),
                "{name}: {sent} lost {:?}",
                lost(sent, &again)
            );
        }
    }
}

#[test]
fn every_reply_a_released_extension_reads_is_read_and_goes_out_as_it_was() {
    for (name, file) in released() {
        for heard in frames(&file, "replies") {
            let read = Reply::from_frame(heard)
                .unwrap_or_else(|e| panic!("{name}: {heard} is not read: {e}"));
            let again = read.to_frame();
            assert_eq!(again["protocolVersion"], 1, "{name}: {heard}");
            assert!(
                lost(heard, &again).is_empty(),
                "{name}: {heard} lost {:?}",
                lost(heard, &again)
            );
        }
    }
}

#[test]
fn a_reply_of_a_newer_version_is_read_for_what_this_side_knows() {
    // An app updated ahead of the extension: a version this side has never heard of, fields
    // and features it does not know. It reads what it knows and goes on.
    let newer = json!({
        "protocolVersion": 99, "type": "engine.hello", "engineVersion": "9.0.0",
        "protocol": 9, "features": ["renews", "something-new"], "somethingNew": {"a": 1},
    });
    let Ok(Reply::Hello(hello)) = Reply::from_frame(&newer) else {
        panic!("a newer hello is not read")
    };
    assert_eq!(hello.engine_version, "9.0.0");
    assert!(hello.judges_pages() && hello.takes_renewals() && !hello.renews);
    assert!(hello.has("something-new"));
    let started = json!({ "protocolVersion": 7, "type": "download.started", "id": "x", "more": 1 });
    assert!(matches!(Reply::from_frame(&started), Ok(Reply::Started { id, .. }) if id == "x"));
    // A reply of a type this side does not know is not mistaken for one it does.
    let unknown = json!({ "protocolVersion": 3, "type": "app.something", "id": "x" });
    assert!(Reply::from_frame(&unknown).is_err());
    // A frame with no version, or one older than any, is still not read.
    assert!(Reply::from_frame(&json!({ "type": "app.shown" })).is_err());
    assert!(Reply::from_frame(&json!({ "protocolVersion": 0, "type": "app.shown" })).is_err());
}

#[test]
fn the_extension_says_who_it_is_and_an_older_ones_bare_hello_still_reads() {
    let greeting = Greeting {
        extension_version: "3.5.0".into(),
        features: vec![feature::READS_ANY_REPLY.into()],
    };
    let frame = Request::Hello(greeting.clone()).to_frame();
    assert_eq!(frame["protocolVersion"], 1, "every app reads the hello");
    assert_eq!(frame["extensionVersion"], "3.5.0");
    assert_eq!(
        Request::from_frame(&frame),
        Ok(Request::Hello(greeting.clone()))
    );
    assert!(greeting.has(feature::READS_ANY_REPLY));
    // 3.4.0's hello says nothing of itself; nothing is sent where nothing is known.
    let bare = json!({ "protocolVersion": 1, "type": "engine.hello" });
    assert_eq!(
        Request::from_frame(&bare),
        Ok(Request::Hello(Greeting::default()))
    );
    assert_eq!(Request::Hello(Greeting::default()).to_frame(), bare);
}

#[test]
fn an_app_says_its_abilities_by_name_and_an_older_ones_fields_still_count() {
    let older: Hello = serde_json::from_value(json!({ "engineVersion": "0.10.0", "renews": true }))
        .expect("a hello");
    assert!(older.takes_renewals());
    let named = Hello {
        features: vec![feature::RENEWS.into()],
        ..Hello::default()
    };
    assert!(named.takes_renewals());
    assert!(!Hello::default().takes_renewals());
}

/// What the app drives, said in a newer app's replies, reads in the shapes extension 3.5.0
/// knows: the menu it serves, its catch verdict, its own words and the panel's list.
#[test]
fn the_mechanisms_an_app_drives_read_from_a_newer_app() {
    let hello = json!({
        "protocolVersion": 4, "type": "engine.hello", "engineVersion": "9.0.0",
        "features": ["menus", "judges-catch", "lists-streams", "later"],
        "menus": [
            { "id": "edm-later", "title": "Later with EDM", "contexts": ["image"], "does": "app", "icon": "x" }
        ],
        "status": { "line": "EDM 9 here", "bad": false, "colour": "green" }
    });
    let Ok(Reply::Hello(hello)) = Reply::from_frame(&hello) else {
        panic!("a newer hello is not read")
    };
    assert!(hello.has(feature::MENUS) && hello.has(feature::JUDGES_CATCH));
    let menus = hello.menus.expect("menus served");
    assert_eq!(menus[0].does, edm_protocol::menu_does::APP);
    assert_eq!(hello.status.expect("a status").line, "EDM 9 here");
    let judged =
        json!({ "protocolVersion": 4, "type": "catch.judged", "id": "j", "take": false, "why": 1 });
    assert!(matches!(
        Reply::from_frame(&judged),
        Ok(Reply::Judged { take: false, .. })
    ));
    let failed = json!({
        "protocolVersion": 4, "type": "download.failed", "id": "x",
        "notice": { "title": "Kept", "body": "why", "icon": "x" }
    });
    let Ok(Reply::Failed {
        notice: Some(notice),
        ..
    }) = Reply::from_frame(&failed)
    else {
        panic!("a refusal's words are not read")
    };
    assert_eq!(
        (notice.title.as_str(), notice.body.as_str()),
        ("Kept", "why")
    );
}

/// The menu an app of today serves is the one an extension of 3.4.0 made for itself, less what
/// Options leaves out: serving it changes nothing a person sees.
#[test]
fn the_served_menu_is_the_menu_a_released_extension_made() {
    let all = edm_protocol::served_menus(&edm_protocol::CatchRules::default());
    let made: Vec<(&str, &str, Vec<&str>)> = all
        .iter()
        .map(|m| {
            (
                m.id.as_str(),
                m.does.as_str(),
                m.contexts.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        made,
        vec![
            ("edm-link", "link", vec!["link"]),
            ("edm-media", "media", vec!["video", "audio"]),
            ("edm-all-links", "all-links", vec!["page"]),
            ("edm-selected-links", "selected-links", vec!["selection"]),
        ]
    );
    let rules = edm_protocol::CatchRules {
        hidden_menus: vec!["edm-media".into()],
        ..edm_protocol::CatchRules::default()
    };
    let ids: Vec<String> = edm_protocol::served_menus(&rules)
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, ["edm-link", "edm-all-links", "edm-selected-links"]);
}

/// The app judges a caught download by its rules as they are now, the browser's tick in
/// Options included, and a download the person asked for whatever its type.
#[test]
fn the_app_judges_a_caught_download_by_its_rules() {
    let judge = edm_protocol::Judge {
        id: "j".into(),
        url: "https://files.example.com/a.iso".into(),
        filename: "a.iso".into(),
        size: Some(10),
        browser: "Brave".into(),
        ..edm_protocol::Judge::default()
    };
    let mut rules = edm_protocol::CatchRules::default();
    assert_eq!(judge.judged_by(&rules), Ok(()));
    rules.min_size_kb = 1;
    assert!(judge
        .judged_by(&rules)
        .unwrap_err()
        .contains("minimum size"));
    let asked = edm_protocol::Judge {
        every_type: true,
        ..judge.clone()
    };
    assert_eq!(
        asked.judged_by(&rules),
        Ok(()),
        "asked for, size does not count"
    );
    rules.off_browsers = vec!["brave".into()];
    assert!(asked.judged_by(&rules).unwrap_err().contains("Brave"));
    rules.off_browsers.clear();
    rules.skip_hosts = vec!["example.com".into()];
    assert!(
        asked.judged_by(&rules).is_err(),
        "a site to leave alone still holds"
    );
}

/// A hand-over from a private window says so (gaps row W17): the flag goes out at version 1,
/// which every app reads, and reads back; a frame without it, as every released extension sends,
/// reads as one from an ordinary window; and the app that keeps no record of one says so by name.
#[test]
fn a_private_windows_hand_over_says_so_and_an_older_frame_reads_as_before() {
    let start = edm_protocol::Start {
        id: "b1".into(),
        url: "https://e.com/f.zip".into(),
        private_window: true,
        ..edm_protocol::Start::default()
    };
    let frame = Request::Start(start.clone()).to_frame();
    assert_eq!(frame["private_window"], true);
    assert_eq!(frame["protocolVersion"], 1, "every app reads it");
    assert_eq!(Request::from_frame(&frame), Ok(Request::Start(start)));
    let older = json!({
        "protocolVersion": 1, "type": "download.start", "id": "b2", "url": "https://e.com/f.zip",
    });
    let Ok(Request::Start(read)) = Request::from_frame(&older) else {
        panic!("an older hand-over is not read")
    };
    assert!(!read.private_window);
    let ordinary = Request::Start(read).to_frame();
    assert!(
        ordinary.get("private_window").is_none(),
        "sent only when set"
    );
    let hello = Hello {
        features: vec![feature::KEEPS_NO_RECORD.into()],
        ..Hello::default()
    };
    assert!(hello.has(feature::KEEPS_NO_RECORD));
}
