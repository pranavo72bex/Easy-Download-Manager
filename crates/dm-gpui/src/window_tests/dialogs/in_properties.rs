// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Properties: what a double click opens, the address, the description, a sign-in and a torrent's
//! files.

use super::*;

#[test]
fn a_double_click_opens_properties_and_open_runs_a_finished_file() {
    let world = world();
    let mut settings = world.settings();
    // Options › General's other choice; "opens the file" is the new install's, below.
    settings.general.double_click = "properties".into();
    let mut h = world.harness_with(settings);
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("one to finish", finished);

    // One still going: its progress window, and not Properties.
    h.click("stalled.bin");
    h.call(
        "ui.click",
        serde_json::json!({ "text": "stalled.bin", "double": true }),
    )
    .unwrap();
    h.until("the progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
    assert!(
        !h.dialog_open("File properties"),
        "Properties opened as well"
    );
    // Its Properties are still there from the menu, with nothing yet to open.
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_a_persons_moment();
    h.person_click("Open", 1);
    assert!(
        h.fake.opened.lock().unwrap().is_empty(),
        "nothing to open yet"
    );
    h.close_dialogs();

    // A finished one: Properties, whose Open runs the file.
    let row = h.row("file.zip");
    h.call(
        "ui.click",
        serde_json::json!({ "text": "file.zip", "double": true }),
    )
    .unwrap();
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_a_persons_moment();
    h.person_click("Open", 1);
    h.until("the file to be opened", |h| {
        !h.fake.opened.lock().unwrap().is_empty()
    });
    assert_eq!(
        *h.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );

    // Options › General's "opens the file": a person's double-click opens a finished one
    // straight away; one still going shows its progress.
    h.close_dialogs();
    h.fake.opened.lock().unwrap().clear();
    h.settings_to(|s| s.general.double_click = "open".into());
    h.until_a_persons_moment();
    h.person_click("file.zip", 2);
    h.until("the file to be opened", |h| {
        !h.fake.opened.lock().unwrap().is_empty()
    });
    assert_eq!(
        *h.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );
    assert!(
        !h.dialog_open("File properties"),
        "Properties opened instead"
    );
    h.person_click("stalled.bin", 2);
    h.until("the progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
}

#[test]
fn properties_keep_the_login_out_of_the_address_box() {
    let password = crate::test_secrets::secret("password");
    let world = world();
    let mut h = world.harness();
    let url = world
        .url("private.bin")
        .replace("http://", &format!("http://me:{password}@"));
    h.view(|v| {
        v.backend.add_with(
            &url,
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
    })
    .unwrap();
    h.until("the row", |h| !h.rows().is_empty());
    let name = dm_shell::app::names::name_of(&h.rows()[0]);
    h.select_by_port(&name);
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_shown("me");
    assert!(
        !h.texts().iter().any(|t| t.contains(&password)),
        "the password is on screen: {:?}",
        h.texts()
    );
    // Changing the password changes it on the download.
    h.type_into("Password", "better");
    h.click("OK");
    h.until("the new password", |h| {
        h.rows()[0].url.contains("me:better@")
    });
}

#[test]
fn properties_change_the_description_and_refresh_an_expired_address() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("gone.bin"), "Start now");
    // A 404 is told in a window of its own (B41), answered before the list is used.
    h.until_shown("The file doesn't exist");
    h.click("OK");
    h.until("the missing file to fail", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    h.select_by_port("gone.bin");
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.type_into("Note", "fixed link");
    h.type_into("Address", &world.url("file.zip"));
    h.click("OK");
    h.until("the row changed", |h| {
        h.rows()[0].description == "fixed link"
    });
    let row = h.rows()[0].clone();
    assert!(row.url.ends_with("/file.zip"), "{}", row.url);
    // Not yet named by a server, the row goes by its new address's name.
    h.select_by_port("file.zip");
    // A failed row is retried, and its button says so.
    h.until_shown("Retry");
    assert!(!h.shows("Resume"), "{:?}", h.texts());
    h.click("Retry");
    h.until("the refreshed download to complete", all_finished);
}

/// A sign-in saved in Properties on a download refused for want of one tries it again, and it
/// downloads.
#[test]
fn a_sign_in_saved_in_properties_retries_the_download() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("locked.zip"), "Start now");
    h.until("the refusal", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    h.select_by_port("locked.zip");
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    // The keyboard is in Login already.
    h.type_text("user");
    h.until("the login typed", |h| h.shows("user"));
    h.type_into("Password", "pass");
    h.click("OK");
    h.until("Trying it again", |h| {
        h.said().contains("again with the sign-in")
    });
    h.until("it to download with the sign-in", all_finished);
}

/// A stopped torrent's Properties lists its files, a tick and a priority each (gaps row 149):
/// High set on one is kept on the download, and so the queue fetches that file first.
#[test]
fn a_torrents_file_is_given_a_priority_in_properties() {
    let world = world();
    let mut h = world.harness();
    h.view(|v| {
        v.backend.add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
    })
    .unwrap();
    h.until("the row", |h| !h.rows().is_empty());
    // The row as a launch reads a paused torrent of three files back.
    let mut torrent: dm_core::Entry = h.rows()[0].clone();
    torrent.state = State::Paused;
    torrent.torrent_files = ["a.bin", "b.bin", "c.bin"]
        .iter()
        .map(|name| dm_core::TorrentFile {
            name: (*name).into(),
            size: 1024,
            wanted: true,
            priority: dm_core::TorrentPriority::Normal,
        })
        .collect::<Vec<_>>()
        .into();
    let stored = world.dir.path().join("torrent-queue.json");
    std::fs::write(&stored, serde_json::to_string(&vec![torrent]).unwrap()).unwrap();
    let mut h = world.harness();
    let queue = h.view(|v| v.backend.queue.clone());
    world.runtime.block_on(queue.persist_to(stored));
    h.until("the torrent", |h| {
        h.rows().first().is_some_and(|r| r.torrent_files.len() == 3)
    });
    let name = dm_shell::app::names::name_of(&h.rows()[0]);
    h.select_by_port(&name);
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    // One pop-up a file, each showing its priority; the list scrolled to the third, whose
    // pop-up is the nearest the front of those on screen, set to High.
    h.until_shown("a.bin (1.0 KB)");
    h.call(
        "ui.scroll",
        serde_json::json!({ "text": "a.bin (1.0 KB)", "lines": 5 }),
    )
    .unwrap();
    h.until_shown("c.bin (1.0 KB)");
    h.click("Normal");
    h.click("High");
    h.until_shown("High");
    h.click("OK");
    h.until("the priority kept", |h| {
        h.rows()[0]
            .torrent_files
            .iter()
            .map(|f| f.priority)
            .collect::<Vec<_>>()
            == [
                dm_core::TorrentPriority::Normal,
                dm_core::TorrentPriority::Normal,
                dm_core::TorrentPriority::High,
            ]
    });
}
