// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! When the queue is allowed to run.
//!
//! The reason anyone sets one of these is a link that is cheap or idle at
//! night, so the window is local time and it is expected to wrap past
//! midnight. A schedule that only worked between 09:00 and 17:00 would be
//! the one case nobody has.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use crate::clock;

/// A window in the local week.
///
/// Every field has a default, so a settings file written by hand or by an
/// older dm with one of them missing still reads, rather than taking the
/// whole file down with it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Schedule {
    /// Whether the schedule is used at all.
    pub enabled: bool,
    /// `HH:MM`, local time.
    pub from: String,
    /// `HH:MM`, local time; the window ends here.
    pub to: String,
    /// Days the window applies on, 0 = Sunday through 6 = Saturday. None
    /// listed means every day, which is what the Scheduler says under its
    /// day boxes: unticking them all is not a way to switch it off.
    pub days: Vec<u8>,
    /// Wake a sleeping computer for the window's start (and a queue's
    /// start time), where the system lets dm: Windows' wake timers.
    pub wake: bool,
    /// What dm does when the queue runs dry after the window opened: `""` or
    /// `"nothing"`, `"exit"`, `"sleep"`, `"hibernate"`, `"shutdown"`. It is
    /// carried out through the same cancellable countdown as Downloads ›
    /// When done, never without it.
    pub when_done: String,
    /// How many more times a download that failed is tried again while the
    /// window is open, 0 to 5; zero leaves it failed, as before.
    pub retries: u8,
    /// A program, with any arguments, run once when the scheduled downloads
    /// are done. Empty is none. Never set by an imported settings file or by
    /// a script driving the window, as the command run after each download is not.
    pub run_when_done: String,
    /// Hang up the dial-up or VPN connection that Options › Advanced › Dial Up /
    /// VPN sets once the scheduled downloads are done (Windows; reference A's
    /// "Hang up modem when done"). Off unless asked.
    pub hang_up: bool,
    /// One run on a date instead of the weekly window: `YYYY-MM-DD HH:MM`,
    /// local. Closed until then, open from then to `to` (the next morning
    /// when `to` is not later than the start), after which the caller
    /// clears it (`Schedule::once_over`).
    pub once_at: Option<String>,
    /// When the window closes, stop what is running and end processes that
    /// would hold the computer awake ("force processes to terminate").
    pub force: bool,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            enabled: false,
            from: "01:00".into(),
            to: "07:00".into(),
            days: (0..=6).collect(),
            wake: false,
            when_done: String::new(),
            retries: 0,
            run_when_done: String::new(),
            hang_up: false,
            once_at: None,
            force: false,
        }
    }
}

/// Whether `text` is a time a schedule can use, `HH:MM` in 24 hours. The
/// Scheduler refuses anything else rather than saving a window that would
/// then be quietly ignored.
pub fn is_time(text: &str) -> bool {
    minutes(text).is_some()
}

/// Today's weekday (0 = Sunday) and the minute past local midnight.
pub fn weekday_now() -> (u8, u32) {
    let now = clock::local_now();
    (now.weekday_from_sunday() as u8, now.minute_of_day())
}

/// The local day (counted from the calendar's start) and the minute past
/// its midnight, for things that happen at a time of day.
pub fn now_local() -> (i64, u32) {
    let now = clock::local_now();
    (now.date.days_from_ce(), now.minute_of_day())
}

/// Minutes past local midnight, or `None` if that is not a time.
pub fn minutes(text: &str) -> Option<u32> {
    let (h, m) = text.split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl Schedule {
    /// Whether the queue may run now.
    pub fn open(&self) -> bool {
        let now = clock::local_now();
        // With a once-only date set, the date decides (`open_now`).
        self.open_now(
            (now.date.year(), now.date.month() as u8, now.date.day() as u8),
            now.weekday_from_sunday() as u8,
            now.minute_of_day(),
        )
    }

    /// The same decision, for a given day and minute — which is what makes it
    /// testable without waiting until one in the morning.
    pub fn open_at(&self, weekday: u8, minute_of_day: u32) -> bool {
        if !self.enabled {
            return true;
        }

        // A malformed time is treated as no schedule at all. The alternative
        // is a queue that silently never runs because of a typo in a file.
        let (Some(from), Some(to)) = (minutes(&self.from), minutes(&self.to)) else {
            return true;
        };
        if from == to {
            return true;
        }
        let on = |day: u8| self.days.is_empty() || self.days.contains(&day);

        if from < to {
            on(weekday) && (from..to).contains(&minute_of_day)
        } else {
            /* Wrapping past midnight, which is the ordinary case.
             *
             * The day is the day the window *opened*, not the day it is now:
             * a Friday-night window that runs to 07:00 is still the Friday
             * window at two in the morning, and checking today's date at
             * that point would close it. */
            if minute_of_day >= from {
                on(weekday)
            } else if minute_of_day < to {
                on((weekday + 6) % 7)
            } else {
                false
            }
        }
    }
}

/// The span a once-only run covers, as (start, end) in minutes since the
/// calendar's day one; `None` where `once_at` or the times are not usable.
fn once_span(schedule: &Schedule) -> Option<(i64, i64)> {
    let (date, hour, minute) = clock::ymd_hm(schedule.once_at.as_deref()?.trim())?;
    let to = minutes(&schedule.to)?;
    let from = hour * 60 + minute;
    let day = date.days_from_ce();
    let start = day * 24 * 60 + i64::from(from);
    let end_day = if to > from { day } else { day + 1 };
    Some((start, end_day * 24 * 60 + i64::from(to)))
}

fn since_day_one((year, month, day): (i32, u8, u8), minute_of_day: u32) -> Option<i64> {
    let date = clock::Date::new(year, u32::from(month), u32::from(day))?;
    Some(date.days_from_ce() * 24 * 60 + i64::from(minute_of_day))
}

impl Schedule {
    /// `open_at`, with a once-only date honoured: with `once_at` set the
    /// queue is closed until that moment, open from it to `to`, and closed
    /// after, until the caller clears it. Without one, or with one that is not
    /// a date, it is `open_at`'s answer.
    pub fn open_now(&self, today: (i32, u8, u8), weekday: u8, minute_of_day: u32) -> bool {
        if !self.enabled {
            return true;
        }
        let (Some((start, end)), Some(now)) =
            (once_span(self), since_day_one(today, minute_of_day))
        else {
            return self.open_at(weekday, minute_of_day);
        };
        (start..end).contains(&now)
    }

    /// Whether a once-only date has been and gone, so the caller clears it.
    pub fn once_over(&self, today: (i32, u8, u8), minute_of_day: u32) -> bool {
        match (once_span(self), since_day_one(today, minute_of_day)) {
            (Some((_, end)), Some(now)) => now >= end,
            _ => false,
        }
    }
}

/// Minutes from `minute_of_day` on `weekday` (0 = Sunday) until the next
/// moment something is due to start: the schedule's window opening, where
/// a schedule is on, or one of `starts` (minutes past midnight, every day).
/// `None` where nothing will. For waking a sleeping computer for it.
pub fn minutes_until_next_start(
    schedule: &Schedule,
    starts: &[u32],
    weekday: u8,
    minute_of_day: u32,
) -> Option<u32> {
    let window = (schedule.enabled)
        .then(|| (minutes(&schedule.from), minutes(&schedule.to)))
        .and_then(|times| match times {
            (Some(from), Some(to)) if from != to => Some(from),
            _ => None,
        });
    let on = |day: u8| schedule.days.is_empty() || schedule.days.contains(&day);
    let mut soonest: Option<u32> = None;
    // A week and a day ahead covers every weekday once and today again.
    for ahead in 0..=7u32 {
        let day = ((u32::from(weekday) + ahead) % 7) as u8;
        let mut at: Vec<u32> = starts.iter().copied().filter(|m| *m < 24 * 60).collect();
        if let Some(from) = window.filter(|_| on(day)) {
            at.push(from);
        }
        for m in at {
            let until = (ahead * 24 * 60 + m).checked_sub(minute_of_day);
            if let Some(until) = until.filter(|u| *u > 0) {
                soonest = Some(soonest.map_or(until, |s| s.min(until)));
            }
        }
        if soonest.is_some_and(|s| s <= (ahead + 1) * 24 * 60) {
            break;
        }
    }
    soonest
}

#[cfg(test)]
mod tests;
