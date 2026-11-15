// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A released extension keeps working with every later app: the frames each released extension
//! sends (`tests/wire/extension-*.json`, never edited) are read by this side whole, nothing they
//! carry is dropped, and what this side replies reads as the replies that extension knew.

use dm_protocol::{feature, Greeting, Hello, Reply, Request};
use serde_json::{json, Value};

fn released() -> Vec<(&'static str, Value)> {
    [(
        "extension-3.4.0.json",
        include_str!("wire/extension-3.4.0.json"),
    )]
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
