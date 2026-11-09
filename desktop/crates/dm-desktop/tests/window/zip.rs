// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A ZIP's files are listed in Download details before the file is fetched,
//! and in a finished ZIP's Properties (reference A shows a ZIP's contents;
//! the maintainer's word of 2026-11-01).

use crate::support::*;

/// Download details for `url`, as the browser's hand-over opens it.
fn open_details(harness: &mut Harness<'static, DmApp>, url: String) {
    // A hand-over asks first, in Download details, as the panel's click does.
    harness.state_mut().applied.ask_panel_downloads = true;
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url,
            ask: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(3);
    assert!(harness.query_by_label("Download details").is_some());
}

/// Waits until the reading of the ZIP's files, in Download details, has ended.
fn reading_over(harness: &mut Harness<'static, DmApp>) {
    until(harness, "the ZIP's files to be read", |app| {
        app.dialogs.file_info.as_ref().is_some_and(|d| {
            d.zip
                .as_ref()
                .is_some_and(|slot| slot.lock().unwrap().is_some())
        })
    });
    harness.run_steps(3);
}

#[test]
fn a_zips_files_are_listed_in_download_details_before_it_is_fetched() {
    let world = world();
    let mut harness = world.quick_harness();
    open_details(&mut harness, world.url("pack.zip"));
    reading_over(&mut harness);
    assert!(harness.query_by_label("Contents").is_some());
    assert!(harness.query_by_label("docs/readme.txt").is_some());
    assert!(harness.query_by_label("setup.exe").is_some());
    assert!(harness.query_by_label("Reading the contents…").is_none());
    assert!(
        harness.state().rows().is_empty(),
        "the archive was not fetched to list it"
    );
}

#[test]
fn a_zip_from_a_server_without_ranges_lists_nothing_and_says_nothing() {
    let world = world();
    let mut harness = world.quick_harness();
    open_details(&mut harness, world.url("plain.zip"));
    reading_over(&mut harness);
    assert!(harness.query_by_label("Contents").is_none());
    assert!(harness.query_by_label("Reading the contents…").is_none());
}

#[test]
fn a_file_named_zip_that_is_none_lists_nothing_and_says_nothing() {
    let world = world();
    let mut harness = world.quick_harness();
    open_details(&mut harness, world.url("notzip.zip"));
    reading_over(&mut harness);
    assert!(harness.query_by_label("Contents").is_none());
    assert!(harness.query_by_label("Reading the contents…").is_none());
}

#[test]
fn a_finished_zips_files_are_listed_in_its_properties() {
    let world = world();
    let mut harness = world.quick_harness();
    harness.state_mut().applied.double_click = "properties".into();
    add_url(&mut harness, &world.url("pack.zip"), "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(2);
    double_click(&mut harness, "pack.zip");
    harness.run_steps(3);
    assert!(harness.query_by_label("File properties").is_some());
    until(&mut harness, "the files to be read from the file", |app| {
        app.dialogs.properties.values().any(|p| {
            p.zip
                .as_ref()
                .is_some_and(|slot| slot.lock().unwrap().is_some())
        })
    });
    harness.run_steps(3);
    assert!(harness.query_by_label("Contents").is_some());
    assert!(harness.query_by_label("setup.exe").is_some());
}

/// One file taken out of the archive alone, before the archive is fetched
/// (gaps EDM-E1 part 7): saved whole in the folder the archive is going to,
/// the line under the list saying where, and no row for the archive.
#[test]
fn take_out_saves_one_file_of_a_zip_before_the_zip_is_fetched() {
    let world = world();
    let mut harness = world.quick_harness();
    open_details(&mut harness, world.url("pack.zip"));
    reading_over(&mut harness);
    let folder = tempfile::tempdir().unwrap();
    harness
        .state_mut()
        .dialogs
        .file_info
        .as_mut()
        .unwrap()
        .folder = folder.path().display().to_string();
    harness.run_steps(2);
    // The first file listed with a button: docs/readme.txt (the folder has none).
    harness.get_all_by_label("Take out").next().unwrap().click();
    harness.run_steps(2);
    until(&mut harness, "the file to be taken out", |app| {
        app.dialogs.file_info.as_ref().is_some_and(|d| {
            d.taken
                .as_ref()
                .is_some_and(|slot| slot.lock().unwrap().is_some())
        })
    });
    harness.run_steps(3);
    let saved = folder.path().join("readme.txt");
    assert_eq!(std::fs::read(&saved).unwrap(), vec![b'a'; 1000]);
    let said = format!("readme.txt saved to {}", folder.path().display());
    assert!(harness.query_by_label(&said).is_some(), "{said}");
    assert!(
        harness.state().rows().is_empty(),
        "the archive was not fetched to take one file out"
    );
}

/// A server that serves no ranges gives no list, and so nothing to take out.
#[test]
fn a_zip_on_a_server_that_serves_no_ranges_offers_nothing_to_take_out() {
    let world = world();
    let mut harness = world.quick_harness();
    open_details(&mut harness, world.url("plain.zip"));
    reading_over(&mut harness);
    assert!(harness.query_by_label("Take out").is_none());
}
