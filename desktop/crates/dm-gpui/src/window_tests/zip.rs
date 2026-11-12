// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A ZIP's files are listed in Download details before the file is fetched, and in a finished
//! ZIP's Properties (reference A shows a ZIP's contents; the maintainer's word of 2026-11-01).

use dm_core::State;

use crate::harness::world::{world, World};
use crate::harness::Harness;

/// The window, and Download details for `name` on its server, as the browser's hand-over
/// opens it.
fn open_details(world: &World, name: &str) -> Harness {
    // A hand-over asks first, in Download details, as the panel's click does.
    let mut h = world.harness_with(dm_shell::Settings {
        ask_panel_downloads: true,
        ..world.settings()
    });
    h.hand_over(dm_shell::host::Request {
        url: world.url(name),
        ask: true,
        ..Default::default()
    });
    h.until("Download details", |h| h.dialog_open("Download details"));
    h
}

/// Waits until the reading of the ZIP's files, in Download details, has ended: the dialog is
/// drawn, and no longer says it is reading them.
fn reading_over(h: &mut Harness) {
    let reading = dm_desktop_base::zip_list::reading_words();
    h.until("the ZIP's files to be read", |h| {
        h.shows("Save to folder") && !h.shows(&reading)
    });
}

#[test]
fn a_zips_files_are_listed_in_download_details_before_it_is_fetched() {
    let world = world();
    let mut h = open_details(&world, "pack.zip");
    h.until_shown("setup.exe");
    assert!(h.shows("Contents"), "{:?}", h.texts());
    assert!(h.shows("docs/readme.txt"), "{:?}", h.texts());
    assert!(!h.shows("Reading the contents…"));
    assert!(
        h.rows().is_empty(),
        "the archive was not fetched to list it"
    );
}

#[test]
fn a_zip_from_a_server_without_ranges_lists_nothing_and_says_nothing() {
    let world = world();
    let mut h = open_details(&world, "plain.zip");
    reading_over(&mut h);
    assert!(!h.shows("Contents"), "{:?}", h.texts());
    assert!(!h.shows("Reading the contents…"));
}

#[test]
fn a_file_named_zip_that_is_none_lists_nothing_and_says_nothing() {
    let world = world();
    let mut h = open_details(&world, "notzip.zip");
    reading_over(&mut h);
    assert!(!h.shows("Contents"), "{:?}", h.texts());
    assert!(!h.shows("Reading the contents…"));
}

#[test]
fn a_finished_zips_files_are_listed_in_its_properties() {
    let world = world();
    let mut h = world.harness_with(dm_shell::Settings {
        double_click: "properties".into(),
        ..world.settings()
    });
    h.add_url(&world.url("pack.zip"), "Start now");
    h.until("it to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    h.double_click("pack.zip");
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_shown("setup.exe");
    assert!(h.shows("Contents"), "{:?}", h.texts());
}

/// One file taken out of the archive alone, before the archive is fetched (gaps EDM-E1 part
/// 7): saved whole in the folder the archive is going to, the line under the list saying where,
/// and no row for the archive. The folder is the one Download details offers, Options' own
/// (the egui test typed another in; where it goes is the same rule).
#[test]
fn take_out_saves_one_file_of_a_zip_before_the_zip_is_fetched() {
    let world = world();
    let mut h = open_details(&world, "pack.zip");
    h.until_shown("setup.exe");
    let folder = world.settings().download_dir;
    // The first file listed with a button: docs/readme.txt (the folder has none). The list is
    // read nearest the front first, so the last Take out drawn is setup.exe's.
    let takes = h.texts().iter().filter(|t| *t == "Take out").count();
    assert_eq!(takes, 2, "{:?}", h.texts());
    h.call(
        "ui.click",
        serde_json::json!({ "text": "Take out", "nth": 1 }),
    )
    .unwrap();
    let said = format!("readme.txt saved to {}", folder.display());
    h.until_shown(&said);
    let saved = folder.join("readme.txt");
    assert_eq!(std::fs::read(&saved).unwrap(), vec![b'a'; 1000]);
    assert!(
        h.rows().is_empty(),
        "the archive was not fetched to take one file out"
    );
}

/// A server that serves no ranges gives no list, and so nothing to take out.
#[test]
fn a_zip_on_a_server_that_serves_no_ranges_offers_nothing_to_take_out() {
    let world = world();
    let mut h = open_details(&world, "plain.zip");
    reading_over(&mut h);
    assert!(!h.shows("Take out"), "{:?}", h.texts());
}
