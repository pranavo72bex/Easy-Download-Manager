// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Undo, after downloads were taken off the list (gaps row U14, B24).

use dm_core::State;
use dm_shell::app::commands::Command;
use serde_json::json;

use crate::harness::world::world;
use crate::harness::Harness;

/// One finished download in the list.
fn one_finished(h: &mut Harness, url: &str) {
    h.call("downloads.add", json!({ "urls": [url] })).unwrap();
    h.until("it to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
}

/// Delete Completed offers Undo, as Remove does, and Undo puts the rows back.
#[test]
fn delete_completed_offers_undo_and_undo_puts_the_rows_back() {
    let world = world();
    let mut h = world.harness();
    one_finished(&mut h, &world.url("file.zip"));
    h.call(
        "command.run",
        json!({ "id": Command::DeleteCompleted.id() }),
    )
    .unwrap();
    h.until("the row to go", |h| h.rows().is_empty());
    h.until_shown("Undo");
    h.click("Undo");
    h.until("the row to come back", |h| h.rows().len() == 1);
    assert_eq!(h.rows()[0].state, State::Completed);
    assert!(!h.shows("Undo"), "Undo still offered after it was used");
}

/// Undo stands until the next command that changes the list: after it, nothing is put back.
#[test]
fn undo_goes_with_the_next_command_that_changes_the_list() {
    let world = world();
    let mut h = world.harness();
    one_finished(&mut h, &world.url("file.zip"));
    h.call(
        "command.run",
        json!({ "id": Command::DeleteCompleted.id() }),
    )
    .unwrap();
    h.until_shown("Undo");
    // Another download added and stopped: Stop All changes the list.
    h.call(
        "downloads.add",
        json!({ "urls": [world.url("stalled.bin")] }),
    )
    .unwrap();
    h.until("it to run", |h| {
        h.rows()
            .iter()
            .any(|r| matches!(r.state, State::Running | State::Queued))
    });
    h.call("command.run", json!({ "id": Command::StopAll.id() }))
        .unwrap();
    h.until("Undo to go", |h| !h.shows("Undo"));
    assert!(h.view(|view| !view.undo.is_kept()));
}
