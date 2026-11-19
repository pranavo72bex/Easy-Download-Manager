// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What holds a download back, in words, for every window: when a closed
//! schedule next opens, and why a download waits (the download limit's
//! wait, or the schedule's hours).

use std::time::Duration;

use dm_shell::Settings;

use crate::format;
use crate::i18n::{t, tf};

/// The Scheduler's days, Sunday first, as `Schedule::days` numbers them.
pub const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// The first minute after `minute` of weekday `today` that the schedule is
/// open: how many days on, and the minute of that day.
pub fn opens_after(schedule: &dm_core::Schedule, today: u32, minute: u32) -> Option<(u32, u32)> {
    (1..=8 * 1440).map(|step| minute + step).find_map(|at| {
        let (days, at) = (at / 1440, at % 1440);
        let weekday = ((today + days) % 7) as u8;
        schedule.open_at(weekday, at).then_some((days, at))
    })
}

/// When a closed schedule next opens, as a person reads it: "01:00" later
/// today, "Mon 01:00" on another day. `None` if it never does.
pub fn opens_next(schedule: &dm_core::Schedule) -> Option<String> {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let today = now.weekday().num_days_from_sunday();
    let minute = now.time().hour() * 60 + now.time().minute();
    opens_after(schedule, today, minute).map(|(days, at)| {
        let clock = format!("{:02}:{:02}", at / 60, at % 60);
        if days == 0 {
            clock
        } else {
            let day = DAYS
                .get(((today + days) % 7) as usize)
                .copied()
                .unwrap_or_default();
            format!("{} {clock}", t(day))
        }
    })
}

/// The settings with a once-only schedule date that has passed put away
/// (`schedule::spent_once_put_away`, as of now), where one has.
pub fn spent_once_now(settings: &Settings) -> Option<Settings> {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    dm_shell::app::schedule::spent_once_put_away(
        settings,
        (now.year(), now.month() as u8, now.day() as u8),
        now.hour() * 60 + now.minute(),
    )
}

/// Why downloads wait now, where something holds them: the download limit
/// (`allowance`: how long until it allows more) or the schedule's hours.
pub fn held_because(settings: &Settings, allowance: Option<Duration>) -> Option<String> {
    if let Some(wait) = allowance {
        return Some(tf(
            "Held by the download limit; it goes on by itself in {}.",
            &[&format::span(wait)],
        ));
    }
    let schedule = &settings.schedule;
    if schedule.enabled && !schedule.open() {
        return Some(match opens_next(schedule) {
            Some(at) => tf("Held by the schedule; it starts by itself at {}.", &[&at]),
            None => t("Held by the schedule.").to_string(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_schedule_opens_later_today_or_on_its_next_day() {
        let schedule = dm_core::Schedule {
            enabled: true,
            from: "01:00".into(),
            to: "07:00".into(),
            days: vec![1],
            wake: false,
            when_done: String::new(),
            retries: 0,
            run_when_done: String::new(),
            hang_up: false,
            once_at: None,
            force: false,
        };
        // Monday 00:30: later today. Monday 08:00: next Monday.
        assert_eq!(opens_after(&schedule, 1, 30), Some((0, 60)));
        assert_eq!(opens_after(&schedule, 1, 480), Some((7, 60)));
        let never = dm_core::Schedule {
            days: vec![],
            from: "bad".into(),
            ..schedule
        };
        // A time that is not one is no schedule: open at once.
        assert_eq!(opens_after(&never, 1, 480), Some((0, 481)));
    }

    #[test]
    fn the_download_limit_is_said_before_the_schedule() {
        let settings = Settings::default();
        assert_eq!(held_because(&settings, None), None);
        let said = held_because(&settings, Some(Duration::from_secs(120))).unwrap();
        assert!(said.starts_with("Held by the download limit"), "{said}");
    }
}
