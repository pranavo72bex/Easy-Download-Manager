// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the status bar says.

use serde_json::json;

use crate::harness::world::world;

/// A download held by a schedule that is shut is said to wait for it, with when it opens, so
/// a queue that does nothing is not taken for a broken one (gaps row U37).
#[test]
fn a_shut_schedule_is_said_to_hold_the_downloads() {
    use chrono::Timelike;
    let world = world();
    let now = chrono::Local::now();
    let mut settings = world.settings();
    settings.schedule = dm_core::Schedule {
        enabled: true,
        from: format!("{:02}:{:02}", (now.hour() + 2) % 24, now.minute()),
        to: format!("{:02}:{:02}", (now.hour() + 3) % 24, now.minute()),
        ..dm_core::Schedule::default()
    };
    let mut h = world.harness_with(settings);
    h.call("downloads.add", json!({ "urls": [world.url("file.zip")] }))
        .unwrap();
    h.until("the status bar to say the schedule holds it", |h| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("· waiting for the schedule (opens at "))
    });
}

/// Download details says what it knows of the size: the size beside what the folder has free
/// once the server has said, not a dash (gaps row U17).
#[test]
fn download_details_says_the_size_beside_the_room_left() {
    let world = world();
    let mut h = world.harness();
    h.add_url_open(&world.url("file.zip"));
    h.until("the size beside the room left", |h| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("256.0 KB · ") && t.ends_with(" free"))
    });
}

/// The bars above the list stand one at a time, with how many more wait; "+N more" shows them
/// all (gaps row U26).
#[test]
fn the_bars_above_the_list_stand_one_at_a_time() {
    let world = world();
    let mut h = world.harness();
    h.until_shown("Add dm to your browser:");
    h.view(|view| view.problems.push("the disk said no".into()));
    h.until_shown("The disk said no");
    assert!(!h.shows("Add dm to your browser:"), "two bars stacked");
    assert!(h.shows("+1 more"));
    h.click("+1 more");
    h.until_shown("Add dm to your browser:");
    assert!(h.shows("The disk said no"));
    assert!(!h.shows("+1 more"));
}
