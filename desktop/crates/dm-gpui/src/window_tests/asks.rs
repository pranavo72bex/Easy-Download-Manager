// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What reference A asks about, and dm now asks (maintainer, 2026-11-01): the name the server
//! sent, a web page where a file was asked for, a 404.

use dm_core::State;

use crate::harness::world::world;
use crate::harness::Harness;

fn finished(h: &mut Harness) -> bool {
    h.rows().iter().any(|r| r.state == State::Completed)
}

#[test]
fn a_name_the_server_sent_is_asked_about_and_use_keeps_it() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("named"), "Start now");
    h.until_shown("Name sent by the server");
    h.click("Use report-2026.pdf");
    h.until("it to finish", finished);
    assert_eq!(h.rows()[0].filename, "report-2026.pdf");
    assert!(!h.shows("Name sent by the server"));
}

#[test]
fn a_name_the_server_sent_is_given_up_for_the_addresss_on_keep() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("named"), "Start now");
    h.until_shown("Name sent by the server");
    h.click("Keep named");
    h.until("it to finish under the address's name", |h| {
        h.rows()
            .iter()
            .any(|r| r.state == State::Completed && r.filename == "named")
    });
    assert!(std::path::Path::new(&h.rows()[0].path).is_file());
}

#[test]
fn a_web_page_for_a_file_is_saved_as_html_on_save_as_html() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("pagefile.zip"), "Start now");
    h.until_shown("Web page instead of the file");
    h.click("Save as HTML");
    h.until("the page to be saved", finished);
    assert_eq!(h.rows()[0].filename, "pagefile.html");
}

/// A page typed into Add URL is asked about as a page for a file is (B40), not refused: Save
/// as HTML saves it (gaps row 136).
#[test]
fn a_typed_web_page_is_asked_about_and_saved_as_html_on_save_as_html() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("article"), "Start now");
    h.until_shown("Web page instead of the file");
    h.click("Save as HTML");
    h.until("the page to be saved", finished);
    let row = h.rows()[0].clone();
    assert!(row.filename.ends_with(".html"), "{}", row.filename);
    assert_eq!(
        std::fs::read_to_string(&row.path).unwrap(),
        "<html><title>An article</title><body>words</body></html>"
    );
}

#[test]
fn a_web_page_for_a_file_leaves_nothing_on_dont_download() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("pagefile.zip"), "Start now");
    h.until_shown("Web page instead of the file");
    h.click("Don't download");
    h.until("the row to go", |h| h.rows().is_empty());
}

#[test]
fn a_404_is_told_once_in_a_window_of_its_own() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("missing.bin"), "Start now");
    h.until_shown("The file doesn't exist");
    assert!(
        h.shows("The requested file doesn't exist."),
        "{:?}",
        h.texts()
    );
    assert!(h.shows("HTTP/1.1 404 Not Found"), "{:?}", h.texts());
    h.click("OK");
    // The row stays failed, and the window does not come back for it.
    for _ in 0..20 {
        h.until("a wake", |_| true);
    }
    assert!(!h.shows("The file doesn't exist"), "{:?}", h.texts());
    assert!(matches!(h.rows()[0].state, State::Failed(_)));
}
