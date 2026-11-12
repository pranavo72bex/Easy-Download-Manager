// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The choices of gaps row 67 (contract rows B49 to B51): what the Scheduler does when it is
//! done, the question about a listed address and its remembered answer, and how dm identifies
//! itself.

use dm_core::State;
use dm_shell::app::commands::Command;
use dm_shell::app::power::WhenDone;

use crate::harness::world::world;

/// The Scheduler offers what dm does when the scheduled downloads are done and how many times
/// a failed one is tried again; both are kept with the schedule.
#[test]
fn the_scheduler_keeps_what_to_do_when_done_and_the_tries_again() {
    let world = world();
    let mut h = world.harness();
    h.run(Command::Scheduler);
    h.until("the Scheduler", |h| h.dialog_open("Scheduler"));
    h.until_shown("Only download between set hours");
    // Off in a new install: its choices are greyed until it is ticked.
    assert!(!world.settings().schedule.enabled);
    h.click("Only download between set hours");
    h.until_shown("When the scheduled downloads are done");
    assert!(
        h.shows("Try a failed download again this many times (0 for none)"),
        "{:?}",
        h.texts()
    );
    h.click("Exit dm");
    h.click("OK");
    h.until("the Scheduler to close", |h| !h.dialog_open("Scheduler"));
    let held = h.view(|view| view.backend.settings()).schedule;
    assert_eq!(held.when_done, "exit");
}

/// The schedule's choice is armed, with the usual countdown, when its hours begin with a
/// download waiting, and put away when the hours end unused.
#[test]
fn the_schedules_when_done_is_armed_with_its_hours_and_put_away_after() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the download to start", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    h.apply(|s| {
        s.schedule = dm_core::Schedule {
            wake: false,
            when_done: "exit".into(),
            retries: 0,
            run_when_done: String::new(),
            hang_up: false,
            once_at: None,
            force: false,
            enabled: true,
            from: "00:00".into(),
            to: "23:59".into(),
            days: (0..=6).collect(),
        }
    });
    h.until("the schedule's choice to be armed", |h| {
        h.view(|view| view.session.when_done) == WhenDone::Quit
    });
    h.apply(|s| s.schedule.enabled = false);
    h.until("the schedule's choice to be put away", |h| {
        h.view(|view| view.session.when_done) == WhenDone::Nothing
    });
    // dm did not end: its download runs on.
    assert!(h.rows().iter().any(|r| r.state == State::Running));
}

/// "Remember my choice" keeps the answer in Options and stops the question; Options brings it
/// back; Replace needs a finished file on disk.
#[test]
fn the_duplicate_question_can_remember_its_answer() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.add_url(&url, "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    let ask = |h: &mut crate::harness::Harness| {
        h.click("Add URL");
        h.until_shown("Address");
        h.type_text(&url);
        h.click("OK");
    };
    ask(&mut h);
    h.until_shown("Already in the list");
    assert!(
        !h.shows("Replace the old file"),
        "nothing on disk to replace"
    );
    h.click("Remember my choice");
    h.click("Download again");
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert_eq!(
        h.view(|view| view.backend.settings()).duplicate_choice,
        "again"
    );
    h.click("Add for later");
    h.until("the second row", |h| h.rows().len() == 2);

    // Asked no more: the remembered answer is taken.
    ask(&mut h);
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert!(!h.shows("Already in the list"));
    h.click("Cancel");
    h.until("Download details to close", |h| {
        !h.dialog_open("Download details")
    });

    h.apply(|s| s.duplicate_choice = String::new());
    ask(&mut h);
    h.until_shown("Already in the list");
}

/// Options shows the row for a listed address (General) and how dm identifies itself with its
/// switch (Advanced).
#[test]
fn options_has_the_duplicate_row_and_the_identity_switch() {
    let world = world();
    let mut h = world.harness();
    h.run(Command::Options);
    h.until_shown("When an address is already in the list");
    h.click("Advanced");
    h.until_shown("Second network");
    // Below the fold: the page scrolled to it, as a person scrolls.
    h.scroll_to("Identify as dm only");
    assert!(
        h.shows("dm identifies itself to servers as:"),
        "{:?}",
        h.texts()
    );
    h.click("Identify as dm only");
    h.click("OK");
    h.until("Options to close", |h| h.dialogs().is_empty());
    assert!(h.view(|view| view.backend.settings()).identify_as_dm);
    assert_eq!(dm_core::first_agent(), dm_core::USER_AGENT);
    h.apply(|s| s.identify_as_dm = false);
    assert_eq!(dm_core::first_agent(), dm_core::IE_AGENT);
}
