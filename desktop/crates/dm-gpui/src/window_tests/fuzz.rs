// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's answer to the control port fed random and hostile requests: each is answered
//! or refused, and the window goes on. `PROPTEST_CASES=20000` for a deep hunt. Ported from
//! the egui window's `tests/fuzz.rs` (its readers every window shares are in
//! `dm-desktop-base/tests/fuzz.rs`).

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{json, Value};

use crate::harness::world::world;

fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        any::<String>().prop_map(Value::String),
        // Commands that act on the list, never Program's (Exit would end the run).
        "(tasks|file|downloads|view|help)\\.[a-z_.]{0,20}".prop_map(Value::String),
        "(all|finished|unfinished|queue:.{0,6}|kind:[a-z]{0,10})".prop_map(Value::String),
    ];
    leaf.prop_recursive(2, 12, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::hash_map(
                prop_oneof![
                    Just("id".to_string()),
                    Just("ids".to_string()),
                    Just("place".to_string()),
                    Just("name".to_string()),
                    "[a-z]{0,6}",
                ],
                inner,
                0..4
            )
            .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

#[test]
fn any_control_request_is_answered_or_refused() {
    let world = world();
    let h = std::cell::RefCell::new(world.harness());
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(64);
    let method = prop_oneof![
        Just("commands.list".to_string()),
        Just("command.run".to_string()),
        Just("ui.inspect".to_string()),
        Just("ui.select".to_string()),
        Just("ui.place".to_string()),
        Just("queue.list".to_string()),
        any::<String>(),
    ];
    let mut runner = TestRunner::new(Config::with_cases(cases));
    runner
        .run(&(method, any_json()), |(method, params)| {
            let mut h = h.borrow_mut();
            // Answered either way, never left waiting.
            let answered = h.call(&method, params);
            prop_assert!(
                !matches!(&answered, Err(why) if why.ends_with("was not answered")),
                "{method}: not answered"
            );
            // Whatever it opened is put away for the next.
            h.close_dialogs();
            Ok(())
        })
        .unwrap();
    // And the window still answers.
    assert!(h.borrow_mut().call("queue.list", json!({})).is_ok());
}
