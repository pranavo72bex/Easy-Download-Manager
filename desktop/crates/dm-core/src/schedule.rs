//! When the queue is allowed to run.
//!
//! The reason anyone sets one of these is a link that is cheap or idle at
//! night, so the window is local time and it is expected to wrap past
//! midnight. A schedule that only worked between 09:00 and 17:00 would be
//! the one case nobody has.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use chrono::{Datelike, Local, Timelike};

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
    let now = Local::now();
    (
        now.weekday().num_days_from_sunday() as u8,
        now.time().hour() * 60 + now.time().minute(),
    )
}

/// The local day (counted from the calendar's start) and the minute past
/// its midnight, for things that happen at a time of day.
pub fn now_local() -> (i64, u32) {
    let now = Local::now();
    (
        i64::from(now.num_days_from_ce()),
        now.time().hour() * 60 + now.time().minute(),
    )
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
        let now = Local::now();
        // With a once-only date set, the date decides (`open_now`).
        self.open_now(
            (now.year(), now.month() as u8, now.day() as u8),
            now.weekday().num_days_from_sunday() as u8,
            now.time().hour() * 60 + now.time().minute(),
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
    let at = chrono::NaiveDateTime::parse_from_str(
        schedule.once_at.as_deref()?.trim(),
        "%Y-%m-%d %H:%M",
    )
    .ok()?;
    let to = minutes(&schedule.to)?;
    let from = at.time().hour() * 60 + at.time().minute();
    let day = i64::from(at.date().num_days_from_ce());
    let start = day * 24 * 60 + i64::from(from);
    let end_day = if to > from { day } else { day + 1 };
    Some((start, end_day * 24 * 60 + i64::from(to)))
}

fn since_day_one((year, month, day): (i32, u8, u8), minute_of_day: u32) -> Option<i64> {
    let date = chrono::NaiveDate::from_ymd_opt(year, u32::from(month), u32::from(day))?;
    Some(i64::from(date.num_days_from_ce()) * 24 * 60 + i64::from(minute_of_day))
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
mod tests {
    use super::*;

    #[test]
    fn the_next_start_is_the_window_or_a_queue_whichever_comes_first() {
        let schedule = Schedule {
            enabled: true,
            from: "23:00".into(),
            to: "07:00".into(),
            days: vec![5], // Friday nights only
            wake: false,
            when_done: String::new(),
            retries: 0,
            run_when_done: String::new(),
            once_at: None,
            force: false,
        };
        // Monday 08:00: Friday 23:00 is four days and fifteen hours away.
        assert_eq!(
            minutes_until_next_start(&schedule, &[], 1, 8 * 60),
            Some(4 * 24 * 60 + 15 * 60)
        );
        // A queue starting daily at 02:00 comes first: tomorrow 02:00.
        assert_eq!(
            minutes_until_next_start(&schedule, &[120], 1, 8 * 60),
            Some(18 * 60)
        );
        // Nothing scheduled at all: nothing to wake for.
        assert_eq!(
            minutes_until_next_start(&Schedule::default(), &[], 1, 0),
            None
        );
        // Exactly at a start is not "next": the following one is.
        assert_eq!(
            minutes_until_next_start(&Schedule::default(), &[60], 2, 60),
            Some(24 * 60)
        );
    }

    /// Overnight, and therefore wrapping — which is the case the arithmetic
    /// is for. The default window, 01:00 to 07:00, does not wrap at all.
    fn nightly() -> Schedule {
        Schedule {
            enabled: true,
            from: "23:00".into(),
            to: "07:00".into(),
            days: (0..=6).collect(),
            wake: false,
            when_done: String::new(),
            retries: 0,
            run_when_done: String::new(),
            once_at: None,
            force: false,
        }
    }

    #[test]
    fn no_schedule_means_any_time() {
        let schedule = Schedule::default();
        assert!(!schedule.enabled);
        assert!(schedule.open_at(3, 12 * 60));
    }

    #[test]
    fn a_window_that_wraps_midnight_is_open_on_both_sides_of_it() {
        let schedule = nightly();
        assert!(schedule.open_at(3, 23 * 60), "23:00 opens the window");
        assert!(schedule.open_at(3, 2 * 60), "02:00 is still inside it");
        assert!(!schedule.open_at(3, 12 * 60), "midday is not");
        assert!(!schedule.open_at(3, 7 * 60), "07:00 is the end, not inside");
    }

    #[test]
    fn a_window_within_one_day_does_not_wrap() {
        let schedule = Schedule {
            from: "09:00".into(),
            to: "17:00".into(),
            ..nightly()
        };
        assert!(schedule.open_at(3, 12 * 60));
        assert!(!schedule.open_at(3, 8 * 60));
        assert!(!schedule.open_at(3, 18 * 60));
    }

    #[test]
    fn the_small_hours_belong_to_the_day_the_window_opened() {
        /* A Friday-night window running to 07:00 is still the Friday window
         * at two on Saturday morning. Asking whether Saturday is scheduled
         * would close it halfway through. */
        let friday_only = Schedule {
            days: vec![5],
            ..nightly()
        };
        assert!(friday_only.open_at(5, 23 * 60), "Friday night");
        assert!(friday_only.open_at(6, 2 * 60), "Saturday 02:00 is Friday's");
        assert!(!friday_only.open_at(6, 23 * 60), "Saturday night is not");
    }

    #[test]
    fn a_day_not_listed_is_closed() {
        let weekdays = Schedule {
            days: vec![1, 2, 3, 4, 5],
            ..nightly()
        };
        assert!(weekdays.open_at(3, 2 * 60));
        assert!(!weekdays.open_at(0, 2 * 60), "Sunday was not listed");
    }

    #[test]
    fn no_day_ticked_means_every_day() {
        let any_day = Schedule {
            days: Vec::new(),
            ..nightly()
        };
        for day in 0..=6 {
            assert!(any_day.open_at(day, 2 * 60), "day {day} at 02:00");
            assert!(!any_day.open_at(day, 12 * 60), "day {day} at midday");
        }
    }

    #[test]
    fn only_a_24_hour_time_is_a_time() {
        for good in ["00:00", "7:05", "23:59", " 01:00 "] {
            assert!(is_time(good), "{good}");
        }
        for bad in ["25:00", "1am", "12:60", "", "12", "ab:cd"] {
            assert!(!is_time(bad), "{bad}");
        }
    }

    #[test]
    fn a_schedule_missing_fields_reads_with_defaults() {
        let read: Schedule = serde_json::from_str(r#"{"enabled": true}"#).unwrap_or_default();
        assert!(read.enabled);
        assert_eq!(read.from, Schedule::default().from);
    }

    #[test]
    fn a_time_that_is_not_a_time_leaves_the_queue_running() {
        // Better than a queue that silently never runs because of a typo.
        let broken = Schedule {
            from: "not a time".into(),
            ..nightly()
        };
        assert!(broken.open_at(3, 12 * 60));
    }

    #[test]
    fn an_empty_window_is_not_a_closed_one() {
        let empty = Schedule {
            from: "03:00".into(),
            to: "03:00".into(),
            ..nightly()
        };
        assert!(empty.open_at(3, 12 * 60));
    }

    fn once(at: &str) -> Schedule {
        Schedule {
            once_at: Some(at.into()),
            ..nightly()
        }
    }

    #[test]
    fn a_once_only_date_opens_at_its_moment_and_closes_at_the_end_time() {
        let s = once("2026-11-03 23:00");
        let (nov3, nov4) = ((2026, 11, 3), (2026, 11, 4));
        assert!(!s.open_now(nov3, 2, 22 * 60 + 59), "a minute before");
        assert!(s.open_now(nov3, 2, 23 * 60), "at its moment");
        assert!(s.open_now(nov4, 3, 60), "past midnight, still inside");
        assert!(!s.open_now(nov4, 3, 7 * 60), "at the end time");
        assert!(!s.open_now((2026, 11, 2), 1, 23 * 60), "the day before");
    }

    #[test]
    fn a_once_only_date_that_is_over_is_cleared_by_the_caller() {
        let s = once("2026-11-03 23:00");
        assert!(!s.once_over((2026, 11, 4), 6 * 60 + 59));
        assert!(s.once_over((2026, 11, 4), 7 * 60));
        assert!(!s.once_over((2026, 11, 3), 12 * 60), "not yet begun");
    }

    #[test]
    fn a_once_only_window_within_one_day_ends_that_day() {
        let s = Schedule {
            to: "17:00".into(),
            ..once("2026-11-03 09:00")
        };
        assert!(s.open_now((2026, 11, 3), 2, 12 * 60));
        assert!(!s.open_now((2026, 11, 3), 2, 17 * 60));
        assert!(!s.open_now((2026, 11, 4), 3, 10 * 60), "not tomorrow");
    }

    #[test]
    fn a_once_only_date_that_is_not_a_date_falls_back_to_the_weekly_window() {
        let s = once("tomorrow evening");
        assert_eq!(s.open_now((2026, 11, 3), 2, 2 * 60), s.open_at(2, 2 * 60));
        assert_eq!(s.open_now((2026, 11, 3), 2, 12 * 60), s.open_at(2, 12 * 60));
        assert!(!s.once_over((2026, 11, 9), 0));
    }

    #[test]
    fn two_schedules_answer_each_their_own_window() {
        let night = nightly();
        let day = Schedule {
            from: "09:00".into(),
            to: "17:00".into(),
            ..nightly()
        };
        assert!(
            night.open_now((2026, 11, 3), 2, 2 * 60) && !day.open_now((2026, 11, 3), 2, 2 * 60)
        );
        assert!(
            !night.open_now((2026, 11, 3), 2, 12 * 60) && day.open_now((2026, 11, 3), 2, 12 * 60)
        );
    }

    #[test]
    fn a_schedule_written_before_once_at_still_reads() {
        let read: Schedule =
            serde_json::from_str(r#"{"enabled": true, "from": "22:00"}"#).unwrap_or_default();
        assert_eq!(read.once_at, None);
        assert!(!read.force);
    }

    #[test]
    fn asking_the_clock_agrees_with_asking_directly() {
        // Guards the wiring between `open` and `open_at`, which is the only
        // part `open_at`'s tests cannot see.
        let now = Local::now();
        let schedule = nightly();
        assert_eq!(
            schedule.open(),
            schedule.open_at(
                now.weekday().num_days_from_sunday() as u8,
                now.time().hour() * 60 + now.time().minute()
            )
        );
    }
}
