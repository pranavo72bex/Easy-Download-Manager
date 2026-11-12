// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Grabber, driven by the words on screen: the sign-in it searches with, a search saved
//! only whole, and what a search found said. Ported from the egui window's
//! `tests/dialogs_cover.rs`.

use crate::harness::world::world;
use crate::harness::{settings_file, Harness};

/// The Grabber, as its toolbar button opens it.
fn open_grabber(h: &mut Harness) {
    h.click("Grabber");
    h.until("the Grabber", |h| h.dialog_open("Grabber"));
    h.until_shown("Start page");
}

/// A start page whose site Options › Sign-ins keeps a login for says the search signs in
/// with it.
#[test]
fn the_grabber_says_it_signs_in_with_the_sites_login() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| {
        s.site_logins = vec![dm_shell::settings::SiteLogin {
            host: "127.0.0.1".into(),
            username: "reader".into(),
            password: ["made", "at", "run", "time"].join("-"),
        }]
    });
    open_grabber(&mut h);
    let note = "Signed in with the login Options › Sign-ins keeps for this site.";
    assert!(!h.shows(note));
    h.fill("Start page", &world.url("page.html"));
    h.until("the note", |h| h.shows(note));
    h.close_dialogs();
    h.settings_to(|s| s.site_logins.clear());
}

/// A search is saved only with a name and a start page, and saved again under its name it is
/// one search, which keeps when it last ran; Delete search takes it away.
#[test]
fn a_grabber_search_is_saved_only_whole_and_once_under_its_name() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_grabber(&mut h);
    h.click("Save search");
    h.until_shown("A saved search needs a name.");
    h.fill("Save this search as", "Weekly");
    h.click("Save search");
    h.until_shown("A saved search needs a start page.");
    h.fill("Start page", &world.url("page.html"));
    h.click("Save search");
    let saved = |h: &mut Harness| h.view(|view| view.backend.settings().site_searches);
    h.until("the search saved", |h| saved(h).len() == 1);
    // As if it ran a day ago: saved again, it keeps that.
    h.settings_to(|s| s.site_searches[0].last_run -= 86_400);
    let ran = saved(&mut h)[0].last_run;
    h.click("Save search");
    let again = saved(&mut h);
    assert_eq!(again.len(), 1, "saved twice under one name");
    assert_eq!(again[0].last_run, ran);
    h.click("Delete search");
    h.until("the search deleted", |h| saved(h).is_empty());
    h.close_dialogs();
}

/// What a search found is said: nothing of those types, a page no server answers for; while a
/// page is read, it says so; Close closes the Grabber.
#[test]
fn the_grabber_says_what_a_search_found() {
    let world = world();
    let mut h = world.harness();
    open_grabber(&mut h);
    h.fill("Start page", &world.url("page.html"));
    h.fill("File types", "xyz");
    h.click("Find files");
    h.until_shown("No files of those types are linked from that page.");

    // A page no server answers for: why, said.
    h.fill("Start page", "http://127.0.0.1:9/nothing.html");
    h.click("Find files");
    h.until("the failure to be said", |h| {
        !h.shows("No files of those types are linked from that page.")
            && !h.shows("Reading the page…")
            && h.texts().iter().any(|t| t.contains("127.0.0.1:9"))
    });

    // A page that never answers: it reads on.
    h.fill("Start page", &world.url("stalled.bin"));
    h.click("Find files");
    h.until_shown("Reading the page…");
    h.click("Close");
    h.until("the Grabber to close", |h| !h.dialog_open("Grabber"));
}
