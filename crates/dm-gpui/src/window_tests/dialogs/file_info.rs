// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download file info: the folder, the name and the sign-in it offers, and where the file lands.

use super::*;

#[test]
fn a_video_from_the_browsers_panel_opens_download_file_info_with_its_quality() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.general.ask_panel_downloads = true);
    let page = world.url("file.zip");
    h.hand(Request {
        url: page.clone(),
        referrer: "https://video.example/watch".into(),
        quality: Some("720".into()),
        label: Some("MP4 1280×720".into()),
        ask: true,
        ..Default::default()
    });
    h.until("Download details", |h| h.dialog_open("Download details"));
    h.until_shown("MP4 1280×720");
    assert!(h.rows().is_empty(), "nothing starts before it is asked");

    h.click("Start now");
    h.until("the row", |h| !h.rows().is_empty());
    let row = h.rows()[0].clone();
    assert_eq!(row.url, page);
    assert_eq!(row.quality.as_deref(), Some("720"));
    assert_eq!(row.referrer, "https://video.example/watch");
}

/// Download details offers the folder a kind of file was last put in, and only for that kind;
/// choosing the kind's own folder again forgets it.
#[test]
fn download_details_offers_the_folder_last_used_for_that_kind() {
    let world = world();
    let mut h = world.harness();
    let folder = folder_in(&world, "my zips");
    let shown = folder.display().to_string();
    details(&mut h, vec![world.url("file.zip")]);
    named(&mut h, "file.zip");
    assert!(!h.shows_in_dialog(&shown));
    h.type_into("Save to folder", &shown);
    h.click("Add for later");
    h.until("the folder kept for archives", |h| {
        h.view(|v| v.backend.settings().last_folders.get("Archives").cloned())
            == Some(shown.clone())
    });
    // A document is not an archive.
    details(&mut h, vec![world.url("other.pdf")]);
    named(&mut h, "other.pdf");
    assert!(
        !h.shows_in_dialog(&shown),
        "an archive's folder offered for a PDF"
    );
    h.click("Cancel");
    // The next archive is offered it.
    details(&mut h, vec![world.url("file.zip")]);
    named(&mut h, "file.zip");
    h.until("the folder offered", |h| h.shows_in_dialog(&shown));
    // Its own folder chosen again: forgotten.
    h.type_into("Save to folder", "");
    h.click("Add for later");
    h.until("the folder forgotten", |h| {
        h.view(|v| v.backend.settings().last_folders.is_empty())
    });
}

#[test]
fn the_file_info_dialog_puts_the_file_where_and_as_what_it_says() {
    let world = world();
    let mut h = world.harness();
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&world.url("file.zip"));
    h.click("Next");
    h.until("Download details", |h| h.dialog_open("Download details"));
    let folder = folder_in(&world, "picked");
    h.type_into("Save to folder", &folder.to_string_lossy());
    // Save as holds the server's name once it answers: replaced, as a person replaces it.
    named(&mut h, "file.zip");
    h.type_into("Save as", "renamed.zip");
    h.type_into("Note", "the one from the page");
    h.click("Start now");
    h.until("the download to complete", finished);
    let row = h.rows()[0].clone();
    assert_eq!(std::path::Path::new(&row.path), folder.join("renamed.zip"));
    assert_eq!(row.description, "the one from the page");
    h.until("the Note column", |h| h.shows("the one from the page"));
}

/// Download details' Category sends a file to the folder of the kind picked, here a .zip to
/// Documents rather than Archives.
#[test]
fn download_details_category_picks_the_folder() {
    let world = world();
    // With sorting into kind folders off, as it is unless turned on: the pick is still that
    // kind's folder.
    let mut h = world.harness();
    details(&mut h, vec![world.url("file.zip")]);
    h.click("As its type says");
    h.click("Documents");
    h.until("the Documents folder", |h| {
        h.texts()
            .iter()
            .any(|t| t.ends_with("Documents") && t.contains(std::path::MAIN_SEPARATOR))
    });
    h.click("Start now");
    h.until("it to complete", finished);
    let path = h.rows()[0].path.clone();
    assert!(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .ends_with("Documents"),
        "{path}"
    );
}

#[test]
fn a_handed_over_videos_sign_in_survives_download_details() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.general.ask_panel_downloads = true);
    h.hand(Request {
        cookies: "session=1".into(),
        ..handed(&world, "file.zip")
    });
    h.until("Download details", |h| h.dialog_open("Download details"));
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let rows = h.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].cookies, "session=1",
        "Download details dropped the sign-in"
    );

    // A page's links: its sign-in goes with the links on its own server only.
    h.hand(Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![
            world.url("other.pdf"),
            "http://elsewhere.invalid/x.zip".into(),
        ],
        ask: true,
        cookies: "session=2".into(),
        ..Default::default()
    });
    h.until("Download links", |h| h.dialog_open("Download links"));
    // The made-up server cannot be asked, so its link is unticked: ticked again, as a person
    // would who wants it anyway.
    h.until("the made-up server's link to be refused", |h| {
        h.shows("Error")
    });
    h.click("Check all");
    h.click("Add for later");
    h.until("the links added", |h| h.rows().len() == 3);
    let rows = h.rows();
    let of = |end: &str| {
        rows.iter()
            .find(|r| r.url.ends_with(end))
            .map(|r| r.cookies.clone())
    };
    assert_eq!(of("/other.pdf").as_deref(), Some("session=2"));
    assert_eq!(
        of("/x.zip").as_deref(),
        Some(""),
        "another server was given the page's sign-in"
    );
}

/// A folder's field has a Browse… button, and what it picks is where the file goes.
#[test]
fn browse_picks_the_folder_a_download_goes_to() {
    let world = world();
    let mut h = world.harness();
    let picked = folder_in(&world, "picked");
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&world.url("file.zip"));
    h.click("Next");
    h.until("Download details", |h| h.dialog_open("Download details"));
    h.choose_ahead(&picked);
    h.click("Browse…");
    h.until("the folder picked", |h| {
        h.shows(&picked.display().to_string())
    });
    h.click("Start now");
    h.until("it to finish", finished);
    assert!(std::path::Path::new(&h.rows()[0].path).starts_with(&picked));

    h.click("Options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.click("Folders");
    h.until("Options › Folders", |h| h.shows("Browse…"));
    assert_eq!(h.texts().iter().filter(|t| *t == "Browse…").count(), 2);
}

/// A file of the name already in the folder: Download details offers to replace it, and it is
/// replaced only once the new one is whole; a number is still the default.
#[test]
fn download_details_can_replace_a_file_of_the_same_name() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", finished);
    let first = std::path::PathBuf::from(&h.rows()[0].path);
    std::fs::write(&first, b"an older version").unwrap();
    h.add_url_open(&world.url("file.zip"));
    let replace = "Replace that file once this one is whole";
    h.until_shown(replace);
    assert!(h.shows("Save this one as file (1).zip"));
    h.click(replace);
    h.click("Start now");
    h.until("the second download to complete", |h| {
        h.rows().len() == 2 && all_finished(h)
    });
    assert_eq!(std::fs::read(&first).unwrap(), payload(), "not replaced");
    let dir = first.parent().unwrap();
    assert!(
        !dir.join("file (1).zip").exists(),
        "a numbered copy was left"
    );
    assert!(h
        .rows()
        .iter()
        .all(|r| std::path::Path::new(&r.path) == first));
}

/// Download details says the name a file will really get where its folder has one of that
/// name already, and that the server has no file at an address, before Start now rather than
/// after.
#[test]
fn download_details_says_the_name_it_will_get_and_a_server_refusal() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", finished);
    h.add_url_open(&world.url("file.zip"));
    h.until_shown("Save this one as file (1).zip");
    h.click("Cancel");

    h.add_url_open(&world.url("missing.bin"));
    h.until_shown("Not available: this link no longer works.");

    // The first copy still under way, not yet on disk: said as well.
    h.click("Cancel");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.add_url_open(&world.url("stalled.bin"));
    h.until_shown("A download with this name exists. This one will be numbered.");
}

/// With Options' "Start downloading while Download details is open", a plain file starts at
/// once and Start now places it under the name chosen; Cancel removes what was begun (gaps row
/// 85).
#[test]
fn a_file_starts_while_download_details_asks_and_lands_where_chosen() {
    let world = world();
    let mut settings = world.settings();
    settings.start_while_asking = true;
    let mut h = world.harness_with(settings);
    details(&mut h, vec![world.url("file.zip")]);
    h.until("the file to start before Start now", |h| {
        h.rows().iter().any(|r| r.url.ends_with("/file.zip"))
    });
    named(&mut h, "file.zip");
    h.type_into("Save as", "chosen.zip");
    h.click("Start now");
    h.until("the file to finish under the name chosen", |h| {
        h.rows()
            .iter()
            .any(|r| r.state == State::Completed && r.path.ends_with("chosen.zip"))
    });
    assert_eq!(
        h.rows()
            .iter()
            .filter(|r| r.url.ends_with("/file.zip"))
            .count(),
        1,
        "added once, not again on Start now"
    );

    // Cancel: what was begun goes.
    details(&mut h, vec![world.url("other.pdf")]);
    h.until("the second file to start", |h| {
        h.rows().iter().any(|r| r.url.ends_with("/other.pdf"))
    });
    h.click("Cancel");
    h.until("the begun file to be removed", |h| {
        !h.rows().iter().any(|r| r.url.ends_with("/other.pdf"))
    });
}

/// Download details' "Keep no record" goes with the download (contract row B69, gaps row W17):
/// unticked for an address typed in, ticked it adds the row keeping no record; a download the
/// browser handed over from a private window opens with it ticked.
#[test]
fn keep_no_record_is_ticked_in_download_details_and_goes_with_the_download() {
    use crate::dialogs::details::Details;
    let world = world();
    let mut h = world.harness();
    details(&mut h, vec![world.url("file.zip")]);
    named(&mut h, "file.zip");
    let ticked = |h: &mut Harness| h.dialog(|d: &mut Details, _, _| d.keep_no_record);
    assert!(!ticked(&mut h), "unticked for an address typed in");
    h.reveal("Keep no record");
    h.click("Keep no record");
    assert!(ticked(&mut h));
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    assert!(h.rows()[0].keep_no_record, "added keeping no record");

    h.settings_to(|s| s.general.ask_panel_downloads = true);
    h.hand(Request {
        url: world.url("other.pdf"),
        referrer: world.url("page.html"),
        ask: true,
        keep_no_record: true,
        ..Default::default()
    });
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert!(ticked(&mut h), "ticked by itself from a private window");
    h.click("Cancel");
    h.until("Download details to close", |h| {
        !h.dialog_open("Download details")
    });
}
