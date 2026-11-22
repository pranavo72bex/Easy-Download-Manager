// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! When the schedule's alarm goes off, and what the phone's start does for dm (gaps row N12),
//! decided here, where the host's tests reach it; `android/receivers.rs` makes the calls.
//!
//! The alarm is set for the next start dm's schedule has (`dm_core::minutes_until_next_start`,
//! read by dm-desktop-base's wake watch), a minute early where it can be, as the desktop's wake
//! timer is, so dm is up when its time comes. Its time is also kept in dm's home
//! ([`NEXT_START`]), since a phone that restarts forgets every alarm: at the phone's start it is
//! set again, or, past already, said. dm's window needs the activity (GPUI draws in it), so the
//! phone's start does not run dm: where the saved list has downloads that were going, or the
//! schedule's time went by, a notification says so and its tap opens dm, whose start goes on
//! with them.

use dm_core::State;

/// The file in dm's home that keeps the alarm's time, in milliseconds since 1970.
pub const NEXT_START: &str = "next-start";

/// A minute, in milliseconds.
const MINUTE: i64 = 60_000;

/// When the alarm goes off for a start `minutes` from now (whole minutes, counted from the
/// start of this one), `now` in milliseconds since 1970: a minute before it, where that is
/// still to come.
pub fn alarm_at(now: i64, minutes: u32) -> i64 {
    let this_minute = now - now.rem_euclid(MINUTE);
    let start = this_minute + i64::from(minutes) * MINUTE;
    if minutes >= 2 {
        start - MINUTE
    } else {
        start
    }
}

/// The alarm's time as [`NEXT_START`] keeps it.
pub fn read_next_start(text: &str) -> Option<i64> {
    text.trim().parse().ok()
}

/// Whether a row saved in this state is one dm goes on with when it starts.
pub fn goes_on(state: &State) -> bool {
    matches!(state, State::Running | State::Queued)
}

/// What is said at the phone's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tell {
    /// Downloads were going when the phone stopped.
    Resume,
    /// The schedule's start went by while the phone was off.
    Due,
}

/// What the phone's start does: the alarm set again (its time), and what is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterBoot {
    /// The alarm, where its time is still to come.
    pub alarm: Option<i64>,
    /// The notification, where there is something to go on with.
    pub tell: Option<Tell>,
}

/// What the phone's start does, from whether the saved list has downloads dm goes on with,
/// the alarm's kept time and the time now.
pub fn after_boot(going: bool, next_start: Option<i64>, now: i64) -> AfterBoot {
    let alarm = next_start.filter(|at| *at > now);
    let due = next_start.is_some_and(|at| at <= now);
    let tell = if going {
        Some(Tell::Resume)
    } else if due {
        Some(Tell::Due)
    } else {
        None
    };
    AfterBoot { alarm, tell }
}

impl Tell {
    /// The notification's title and line; a tap opens dm.
    pub fn words(self) -> (&'static str, &'static str) {
        match self {
            Tell::Resume => (
                "dm has downloads to resume",
                "Tap to open dm and go on with them",
            ),
            Tell::Due => (
                "dm's schedule is due",
                "Tap to open dm and start the scheduled downloads",
            ),
        }
    }
}

/// The ongoing notification's words while the schedule's alarm holds dm up for its start.
pub const SCHEDULE_TITLE: &str = "dm's schedule is starting";
/// Its second line.
pub const SCHEDULE_BODY: &str = "Tap to see the downloads";

/// The file in dm's home whose presence says dm has asked for exact alarms once (gaps row N12).
pub const EXACT_ASKED: &str = "exact-alarm-asked";

/// The notification that asks, once, for Android's "Alarms & reminders" (Android 12 and up),
/// without which the schedule's alarm is inexact and Android may start it late; a tap opens
/// that page of dm's settings.
pub const EXACT_TITLE: &str = "dm's schedule may start late";
/// Its second line: the reason.
pub const EXACT_BODY: &str = "Tap to allow dm Alarms & reminders, so it starts on time";

/// Whether to ask for exact alarms now: only where the alarm just set could not be exact and
/// dm has not asked before (a person who said no is not asked again).
pub fn asks_for_exact(exact: bool, asked_before: bool) -> bool {
    !exact && !asked_before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_alarms_are_asked_for_once_and_only_where_refused() {
        assert!(asks_for_exact(false, false));
        assert!(!asks_for_exact(false, true), "asked twice");
        assert!(!asks_for_exact(true, false), "asked though allowed");
    }

    const NOON: i64 = 1_800_000_000_000 - 1_800_000_000_000 % MINUTE;

    #[test]
    fn the_alarm_goes_off_a_minute_early_on_the_minute() {
        // 25 seconds into a minute, a start 10 minutes away: 9 minutes from this minute's start.
        assert_eq!(alarm_at(NOON + 25_000, 10), NOON + 9 * MINUTE);
        // A start a minute away: on it, as there is no minute to spare.
        assert_eq!(alarm_at(NOON + 25_000, 1), NOON + MINUTE);
        // Asked again a minute later, the same time: the alarm is not set again.
        assert_eq!(alarm_at(NOON + MINUTE + 5_000, 9), NOON + 9 * MINUTE);
    }

    #[test]
    fn the_kept_time_is_read_back() {
        assert_eq!(read_next_start(" 1800000000000\n"), Some(1_800_000_000_000));
        assert_eq!(read_next_start("soon"), None);
    }

    #[test]
    fn running_and_waiting_rows_go_on_and_the_rest_do_not() {
        assert!(goes_on(&State::Running));
        assert!(goes_on(&State::Queued));
        assert!(!goes_on(&State::Paused));
        assert!(!goes_on(&State::Completed));
        assert!(!goes_on(&State::Failed("x".into())));
    }

    #[test]
    fn the_phones_start_sets_the_alarm_again_or_says_what_is_due() {
        let later = Some(NOON + 60 * MINUTE);
        let past = Some(NOON - MINUTE);
        assert_eq!(
            after_boot(false, later, NOON),
            AfterBoot {
                alarm: later,
                tell: None
            }
        );
        assert_eq!(
            after_boot(false, past, NOON),
            AfterBoot {
                alarm: None,
                tell: Some(Tell::Due)
            }
        );
        assert_eq!(
            after_boot(true, later, NOON),
            AfterBoot {
                alarm: later,
                tell: Some(Tell::Resume)
            }
        );
        assert_eq!(
            after_boot(false, None, NOON),
            AfterBoot {
                alarm: None,
                tell: None
            }
        );
    }
}
