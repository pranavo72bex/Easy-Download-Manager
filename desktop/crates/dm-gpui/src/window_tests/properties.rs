// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! File properties, driven by the words on screen: a failed download's sign-in and Copy
//! details, an address dm cannot fetch refused, a category of the person's own named, a file
//! that cannot be read said so. Ported from the egui window's `tests/dialogs_cover.rs`.

use dm_core::State;
use dm_shell::app::commands::Command;

use crate::harness::world::world;
use crate::harness::{settings_file, Harness};

/// File properties of the row `name`.
fn properties_of(h: &mut Harness, name: &str) {
    h.select(name);
    h.run(Command::Properties);
    h.until("File properties", |h| h.dialog_open("File properties"));
}

/// `name` added for later through the port.
fn later(h: &mut Harness, url: String) {
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [url], "later": true }),
    )
    .unwrap();
    h.until("the row", |h| !h.rows().is_empty());
}

/// A download that failed for want of a sign-in: Properties has its Login ready to type in
/// and a Copy details for whoever helps, which copies what went wrong and says so, and leaves
/// Properties open.
#[test]
fn properties_of_a_failed_download_copies_its_details() {
    let world = world();
    let mut h = world.harness();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("secret.zip")] }),
    )
    .unwrap();
    h.until("secret.zip to fail", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    properties_of(&mut h, "secret.zip");
    // The keyboard is in Login: what is typed goes there.
    h.type_text("reader");
    h.until("the login typed", |h| h.shows("reader"));
    h.click("Copy details");
    h.until("the details copied", |h| {
        h.said() == "Copied the details of 1 download."
    });
    let copied = h
        .cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default();
    assert!(copied.contains("State: failed"), "{copied}");
    assert!(h.dialog_open("File properties"), "Copy details closed it");
    h.click("Cancel");
    h.until("Properties to close", |h| !h.dialog_open("File properties"));
}

/// An address dm cannot fetch is refused with the reason and Properties stays open with it;
/// the row keeps its own; a row taken out of the list closes its Properties.
#[test]
fn properties_refuse_an_address_dm_cannot_fetch_and_close_with_their_row() {
    let world = world();
    let mut h = world.harness();
    later(&mut h, world.url("kept.bin"));
    let row = h.row("kept.bin");
    properties_of(&mut h, "kept.bin");
    h.type_into("Address", "nothing-to-fetch");
    h.click("OK");
    h.until("the refusal", |h| h.shows_part("dm can download"));
    assert!(h.dialog_open("File properties"));
    assert_eq!(h.view(|view| view.backend.row(row.id)).unwrap().url, row.url);

    h.view(|view| view.backend.remove(row.id));
    h.until("Properties of a row that is gone to close", |h| {
        !h.dialog_open("File properties")
    });
}

/// A download of a category of the person's own is said to be of it.
#[test]
fn properties_name_a_category_of_the_persons_own() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| {
        s.custom_categories = vec![dm_core::Custom {
            name: "Lectures".into(),
            extensions: vec!["lecture".into()],
            sites: Vec::new(),
        }]
    });
    later(&mut h, world.url("week1.lecture"));
    properties_of(&mut h, "week1.lecture");
    h.until_shown("Category");
    assert!(h.shows_in_dialog("Lectures"), "{:?}", h.texts());
    h.close_dialogs();
    h.settings_to(|s| s.custom_categories.clear());
}

/// A finished file whose digest cannot be worked out (a folder where the file was) says why,
/// for its SHA-256 and for a digest pasted to check.
#[test]
fn a_finished_file_that_cannot_be_read_says_why_its_digest_is_not_known() {
    let world = world();
    let mut h = world.harness();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("file.zip")] }),
    )
    .unwrap();
    h.until("file.zip to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    let done = h.row("file.zip");
    std::fs::remove_file(&done.path).unwrap();
    std::fs::create_dir(&done.path).unwrap();
    properties_of(&mut h, "file.zip");
    h.click_beside("Check against");
    h.type_text(&"0".repeat(32));
    let why = world
        .runtime
        .block_on(dm_core::digest(
            std::path::Path::new(&done.path),
            dm_core::Algorithm::Sha256,
        ))
        .unwrap_err()
        .to_string();
    h.until("the reason said for both", |h| {
        h.texts().iter().filter(|t| t.contains(&why)).count() >= 2
    });
    std::fs::remove_dir(&done.path).unwrap();
}
