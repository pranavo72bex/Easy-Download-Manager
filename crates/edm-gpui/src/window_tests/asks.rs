// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What reference A asks about, and EDM asks too (maintainer): the name the server
//! sent, a web page where a file was asked for; a 404 asks nothing.

use edm_core::State;

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
    h.click("Don’t download");
    h.until("the row to go", |h| h.rows().is_empty());
}

/// A 404 opens no window (B41, the maintainer, 2026-11-27): the row says "This link no longer
/// works" and the bar above the list counts it; Update download link is on the row's menu.
#[test]
fn a_404_opens_no_window() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("missing.bin"), "Start now");
    h.until("the row to fail", |h| {
        h.rows()
            .first()
            .is_some_and(|r| matches!(r.state, State::Failed(_)))
    });
    for _ in 0..20 {
        h.until("a wake", |_| true);
    }
    assert!(!h.shows("HTTP/1.1 404 Not Found"), "{:?}", h.texts());
    assert!(h.dialogs().is_empty(), "{:?}", h.dialogs());
}

/// The same file from a second address (contract row B73, gaps row W18): asked about before it
/// is fetched. Show it takes the new row off, the finished one chosen; Download again, with
/// Remember my choice ticked, fetches it beside the first and keeps the answer.
#[test]
fn the_same_file_from_another_address_is_asked_about_before_it_is_fetched() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the first copy to finish", finished);
    let first = h.rows()[0].id;
    h.add_url(&world.url("file.zip?mirror=2"), "Start now");
    h.until_shown("The same file is already here");
    assert!(h.shows("A finished download in the list has this name and size."));
    assert!(h.shows("Download again") && h.shows("Remember my choice"));
    h.click("Show it");
    h.until("the new row to go", |h| h.rows().len() == 1);
    assert!(h.view(|view| view.list.selected.contains(&first)));
    assert!(!h.shows("The same file is already here"));

    h.add_url(&world.url("file.zip?mirror=3"), "Start now");
    h.until_shown("The same file is already here");
    h.click("Remember my choice");
    h.click("Download again");
    h.until("both copies to finish", |h| {
        let rows = h.rows();
        rows.len() == 2 && rows.iter().all(|r| r.state == State::Completed)
    });
    assert!(h.rows().iter().any(|r| r.path.ends_with("file (1).zip")));
    assert_eq!(
        h.view(|view| view.backend.settings()).same_file_choice,
        "again"
    );
}
