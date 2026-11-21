// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Grabber, driven by the words on screen: the sign-in it searches with, a search saved
//! only whole, what a search found said, and its two boxes under the page depth (B60, B61).

use crate::harness::world::{world, World};
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

/// Two sites on this computer for a search to cross: a start page on `127.0.0.1` that links a
/// file of its own and a page on `localhost` (another site, as two names are), which links
/// another file. The start page's address.
fn two_sites(world: &World) -> String {
    use axum::response::Html;
    use axum::routing::get;
    world.runtime.block_on(async {
        let other = axum::Router::new().route(
            "/more.html",
            get(|| async { Html(r#"<a href="far.zip">far</a>"#) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, other).await.unwrap() });
        let page = format!(
            r#"<a href="near.zip">near</a> <a href="http://localhost:{port}/more.html">away</a>"#
        );
        let home = axum::Router::new().route(
            "/",
            get(move || {
                let page = page.clone();
                async move { Html(page) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, home).await.unwrap() });
        format!("http://{addr}/")
    })
}

/// The searches saved now.
fn saved(h: &mut Harness) -> Vec<dm_shell::lists::Search> {
    h.view(|view| view.backend.settings().site_searches)
}

/// "Also follow links to other sites" (B60): left off, a search reads only the start page's
/// own site; ticked, the page on another site that the start page links to is read too, to
/// the same depth, and a search saved with it ticked keeps it.
#[test]
fn the_grabber_follows_links_to_other_sites_only_when_ticked_and_a_saved_search_keeps_it() {
    let _file = settings_file();
    let world = world();
    let start = two_sites(&world);
    let mut h = world.harness();
    open_grabber(&mut h);
    h.type_into("Start page", &start);
    h.type_into("Page depth (0 for the start page only)", "1");
    h.type_into("File types", "zip");
    h.click("Find files");
    h.until("the start page's own site read", |h| {
        h.shows_part("Files found: 1 · pages read: 1")
    });
    h.click("Also follow links to other sites");
    h.click("Find files");
    h.until("the other site read too", |h| {
        h.shows_part("Files found: 2 · pages read: 2")
    });
    h.type_into("Save this search as", "Wide");
    h.click("Save search");
    h.until("the search saved", |h| saved(h).len() == 1);
    let search = saved(&mut h).remove(0);
    assert!(search.other_sites, "{search:?}");
    assert!(!search.keep_layout, "{search:?}");
    h.click("Delete search");
    h.until("the search deleted", |h| saved(h).is_empty());
    h.close_dialogs();
}

/// "Keep the site's folders" (B61): off by default and kept ticked by a saved search; ticked,
/// each file a search found is saved under the download folder in folders that mirror its
/// address (here its site's, `127.0.0.1_<port>`), not in the download folder itself.
#[test]
fn ticked_keep_the_sites_folders_saves_each_file_in_folders_that_mirror_its_address() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_grabber(&mut h);
    h.type_into("Start page", &world.url("page.html"));
    h.type_into("File types", "zip pdf");
    h.type_into("Save this search as", "Mirrored");
    h.click("Save search");
    h.until("the search saved", |h| saved(h).len() == 1);
    let untouched = saved(&mut h).remove(0);
    assert!(!untouched.keep_layout, "ticked by default");
    assert!(!untouched.other_sites, "ticked by default");
    h.click("Keep the site's folders");
    h.click("Save search");
    h.until("the search saved with the box ticked", |h| {
        saved(h).first().is_some_and(|s| s.keep_layout)
    });
    assert_eq!(saved(&mut h).len(), 1, "saved twice under one name");
    h.click("Delete search");
    h.until("the search deleted", |h| saved(h).is_empty());

    h.click("Find files");
    h.until("the files found", |h| h.shows_part("Files found: 2 "));
    h.click("Start now");
    h.until("both fetched", |h| {
        let rows = h.rows();
        rows.len() == 2 && rows.iter().all(|r| r.state == dm_core::State::Completed)
    });
    let root = h.view(|view| view.backend.settings().folders.download_dir);
    let site = format!("127.0.0.1_{}", world.addr.port());
    let mirrored = root.join(&site);
    for row in h.rows() {
        assert_eq!(row.save_dir.as_deref(), mirrored.to_str(), "{}", row.url);
        let file = std::path::Path::new(&row.path);
        assert!(
            file.parent().is_some_and(|folder| folder.ends_with(&site)),
            "{} is not in {site}",
            row.path
        );
        assert!(file.is_file(), "{} is not on disk", row.path);
    }
}
