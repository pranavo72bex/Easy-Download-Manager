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
