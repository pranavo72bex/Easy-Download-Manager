// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Addresses handed over by the browser: taken once, asked about, and the window brought forward.

use super::*;

/// A double-click on a link in the browser hands it over twice in a moment: one row, not
/// `name (1)` and `name (2)`.
#[test]
fn the_same_address_handed_twice_at_once_is_taken_once() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.view(|view| {
        let mut handed = view.backend.handed.lock().unwrap();
        for _ in 0..2 {
            handed.push_back(dm_shell::host::Request {
                url: url.clone(),
                ..Default::default()
            });
        }
    });
    h.until("the row", |h| h.rows().len() == 1);
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert_eq!(h.rows().len(), 1);
}

/// A hand-off of an address already listed is added, and said.
#[test]
fn a_hand_off_of_a_listed_address_is_said_to_be_added_again() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    // As the inbox adds a browser's hand-off: to the queue, not by the window.
    h.view(|view| {
        view.backend
            .add_with(&world.url("file.zip"), dm_core::AddOptions::default())
            .unwrap()
    });
    h.until("it to be said", |h| {
        h.said() == "Added file.zip again: that address was already in the list."
    });
}

#[test]
fn a_caught_download_of_a_listed_address_asks_before_fetching_again() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.view(|view| {
        view.backend
            .add_with(&url, dm_core::AddOptions::default())
            .unwrap()
    });
    h.until("the row", |h| h.rows().len() == 1);
    h.hand_over(dm_shell::host::Request {
        url: url.clone(),
        referrer: "https://e.com/page".into(),
        caught: true,
        ..Default::default()
    });
    h.until_shown("Already in the list");
    assert_eq!(h.rows().len(), 1, "fetched again unasked");
}

/// A download handed over and waiting for an answer has left the browser: a window comes
/// forward for it. One that starts at once does not take the person's screen.
#[test]
fn a_hand_over_that_asks_brings_the_window_forward_and_one_that_starts_does_not() {
    let world = world();
    let mut h = world.harness();
    let handed = |h: &mut Harness, name: &str| {
        h.hand_over(dm_shell::host::Request {
            url: world.url(name),
            referrer: "https://e.com/page".into(),
            caught: true,
            ..Default::default()
        });
    };
    handed(&mut h, "quiet.zip");
    h.until("it to start", |h| h.rows().len() == 1);
    // (Its address is served nothing: the window that says so is not this test's.)
    assert!(
        !h.dialog_open("Download details"),
        "raised for a download that started"
    );
    h.apply(|s| s.general.ask_caught_downloads = true);
    handed(&mut h, "asks.zip");
    // Its own window, which the system puts in front as it opens (GPUI's windows open
    // focused); the test platform does not stack windows, so that part is the system's.
    h.until("Download details", |h| h.dialog_open("Download details"));
}
