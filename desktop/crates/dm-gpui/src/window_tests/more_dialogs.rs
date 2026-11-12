// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! More of the windows over the list: a progress window whose row is gone, Update download
//! link given what is no link, About's pages. Ported from the egui window's
//! `tests/dialogs_more_cover.rs`.

use dm_shell::app::commands::Command;

use crate::harness::world::world;
use crate::harness::Harness;

/// `name` added for later through the port, and selected.
fn later(h: &mut Harness, url: String, name: &str) {
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [url], "later": true }),
    )
    .unwrap();
    h.until("the row", |h| !h.rows().is_empty());
    h.select(name);
}

/// A progress window whose row is taken out of the list closes.
#[test]
fn a_progress_window_whose_row_is_gone_closes() {
    let world = world();
    let mut h = world.harness();
    later(&mut h, world.url("gone.bin"), "gone.bin");
    let id = h.row("gone.bin").id;
    h.run(Command::ShowProgress);
    h.until("its progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
    h.view(|view| view.backend.remove(id));
    h.until("the window to close", |h| {
        !h.dialog_names().iter().any(|d| d == "Download progress")
    });
}

/// Update download link: an address that is no link is said and the dialog stays; Cancel
/// closes it and the row keeps its own.
#[test]
fn a_new_address_that_is_no_link_is_said_and_cancel_closes() {
    let world = world();
    let mut h = world.harness();
    later(&mut h, world.url("old.bin"), "old.bin");
    let row = h.row("old.bin");
    h.run(Command::UpdateLink);
    h.until("Update download link", |h| {
        h.dialog_open("Update download link")
    });
    h.type_text("not a link");
    h.click("OK");
    h.until("the refusal", |h| h.shows_part("dm can download"));
    assert!(h.dialog_open("Update download link"));
    h.click("Cancel");
    h.until("the dialog to close", |h| {
        !h.dialog_open("Update download link")
    });
    assert_eq!(h.view(|view| view.backend.row(row.id)).unwrap().url, row.url);
}

/// About opens the project's privacy page.
#[test]
fn about_opens_its_privacy_page() {
    let world = world();
    let mut h = world.harness();
    h.run(Command::About);
    h.until_shown("Privacy");
    h.click("Privacy");
    h.until("the privacy page", |h| {
        h.fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o == dm_desktop_base::project::PRIVACY)
    });
}
