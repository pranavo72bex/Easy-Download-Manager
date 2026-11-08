// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The choices of gaps row 67 (contract rows B49 to B51): what the Scheduler
//! does when it is done, the question about a listed address and its
//! remembered answer, and how dm identifies itself.

use crate::support::*;

/// The Scheduler offers what dm does when the scheduled downloads are done
/// and how many times a failed one is tried again; both are kept with the
/// schedule.
#[test]
fn the_scheduler_keeps_what_to_do_when_done_and_the_tries_again() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::Scheduler);
    harness.run_steps(2);
    if let Some(schedule) = harness
        .state_mut()
        .dialogs
        .scheduler
        .as_mut()
        .map(|d| &mut d.schedule)
    {
        schedule.enabled = true;
        schedule.days = (0..=6).collect();
    }
    harness.run_steps(2);
    assert!(harness
        .query_by_label("When the scheduled downloads are done")
        .is_some());
    assert!(harness
        .query_by_label("Try a failed download again this many times (0 for none)")
        .is_some());
    click(&mut harness, "Exit dm");
    if let Some(schedule) = harness
        .state_mut()
        .dialogs
        .scheduler
        .as_mut()
        .map(|d| &mut d.schedule)
    {
        assert_eq!(schedule.when_done, "exit");
        schedule.retries = 9;
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    let held = harness.state().backend().settings().schedule;
    assert_eq!(held.when_done, "exit");
    let mut back = harness.state().backend().settings();
    back.schedule.enabled = false;
    harness.state_mut().apply_settings(back).unwrap();
}

/// The schedule's choice is armed, with the usual countdown, when its hours
/// begin with a download waiting, and put away when the hours end unused.
#[test]
fn the_schedules_when_done_is_armed_with_its_hours_and_put_away_after() {
    let _file = settings_file();
    use dm_desktop::power::WhenDone;
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "the download to start", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    let mut wanted = harness.state().applied.clone();
    wanted.schedule = dm_core::Schedule {
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
    };
    harness.state_mut().apply_settings(wanted).unwrap();
    for _ in 0..4 {
        harness.state_mut().refresh();
        harness.step();
    }
    assert_eq!(harness.state().when_done, WhenDone::Quit);
    let mut off = harness.state().applied.clone();
    off.schedule.enabled = false;
    harness.state_mut().apply_settings(off).unwrap();
    for _ in 0..4 {
        harness.state_mut().refresh();
        harness.step();
    }
    assert_eq!(
        harness.state().when_done,
        WhenDone::Nothing,
        "the schedule's choice outlived its hours"
    );
    assert!(!harness.state().quit_asked);
}

/// "Remember my choice" keeps the answer in Options and stops the question;
/// Options brings it back; Replace needs a finished file on disk.
#[test]
fn the_duplicate_question_can_remember_its_answer() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let url = world.url("file.zip");
    add_url(&mut harness, &url, "Add for later");
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    assert!(harness.query_by_label("Already in the list").is_some());
    assert!(
        harness.query_by_label("Replace the old file").is_none(),
        "nothing on disk to replace"
    );
    click(&mut harness, "Remember my choice");
    click(&mut harness, "Download again");
    assert_eq!(harness.state().applied.duplicate_choice, "again");
    click(&mut harness, "Add for later");
    assert_eq!(harness.state().rows().len(), 2);

    // Asked no more: the remembered answer is taken.
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    assert!(harness.query_by_label("Already in the list").is_none());
    assert!(harness.query_by_label("Download details").is_some());
    click(&mut harness, "Cancel");

    let mut wanted = harness.state().applied.clone();
    wanted.duplicate_choice = String::new();
    harness.state_mut().apply_settings(wanted).unwrap();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    assert!(harness.query_by_label("Already in the list").is_some());
}

/// Options shows the row for a listed address (General) and how dm
/// identifies itself with its switch (Advanced).
#[test]
fn options_has_the_duplicate_row_and_the_identity_switch() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    harness.run_steps(3);
    assert!(harness
        .query_by_label("When an address is already in the list")
        .is_some());
    click(&mut harness, "Advanced");
    harness.run_steps(3);
    assert!(harness
        .query_by_label("dm identifies itself to servers as:")
        .is_some());
    click(&mut harness, "Identify as dm only");
    click(&mut harness, "OK");
    assert!(harness.state().applied.identify_as_dm);
    assert_eq!(dm_core::first_agent(), dm_core::USER_AGENT);
    let mut back = harness.state().applied.clone();
    back.identify_as_dm = false;
    harness.state_mut().apply_settings(back).unwrap();
    assert_eq!(dm_core::first_agent(), dm_core::IE_AGENT);
}
