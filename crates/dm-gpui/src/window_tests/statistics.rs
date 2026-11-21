// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm has done, in figures (gaps row W15): View › Statistics, and a progress window's
//! speed line.

use dm_core::State;
use serde_json::json;

use crate::harness::world::world;

/// View › Statistics opens a window of its own with its figures and headings, the finished
/// download counted by its site and its kind; Close closes it.
#[test]
fn view_statistics_opens_with_its_headings() {
    let world = world();
    let mut h = world.harness();
    h.view(|view| view.backend.add(&world.url("file.zip")).unwrap());
    h.until("it to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    h.click("View");
    h.click("Statistics…");
    h.until("Statistics", |h| h.dialog_open("Statistics"));
    for words in [
        "Files finished",
        "Downloaded in all",
        "By day, the last 30 days",
        "By site, the 10 largest",
        "By category",
        "Archives",
    ] {
        h.until(words, |h| h.shows_in_dialog(words));
    }
    assert!(!h.shows("Nothing has finished yet."), "{:?}", h.texts());
    // The rest under them, as a person scrolls to it.
    h.call("ui.scroll", json!({ "text": "By category", "lines": 10 }))
        .unwrap();
    for words in ["Average speed", "Best speed", "Close"] {
        h.until(words, |h| h.shows_in_dialog(words));
    }
    h.click("Close");
    h.until("Statistics to close", |h| !h.dialog_open("Statistics"));
    // Nothing of it was written: the settings know no statistics.
    let settings = std::fs::read_to_string(dm_shell::settings::home().join("settings.json"))
        .unwrap_or_default();
    assert!(!settings.to_lowercase().contains("statistic"), "{settings}");
}

/// A running download's progress window draws its speed over the last five minutes.
#[test]
fn a_progress_window_has_its_speed_line() {
    let world = world();
    let mut h = world.harness();
    h.view(|view| view.backend.add(&world.url("stalled.bin")).unwrap());
    h.until("it to run", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    h.until_shown("Speed over the last 5 minutes");
}
