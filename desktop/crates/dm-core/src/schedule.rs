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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Schedule {
    pub enabled: bool,
    /// `HH:MM`, local time.
    pub from: String,
    pub to: String,
    /// Days the window applies on, 0 = Sunday through 6 = Saturday.
    pub days: Vec<u8>,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            enabled: false,
            from: "01:00".into(),
            to: "07:00".into(),
            days: (0..=6).collect(),
        }
    }
}

/// Minutes past local midnight, or `None` if that is not a time.
fn minutes(text: &str) -> Option<u32> {
    let (h, m) = text.split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl Schedule {
    /// Whether the queue may run now.
    pub fn open(&self) -> bool {
        let now = Local::now();
        self.open_at(
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

        if from < to {
            self.days.contains(&weekday) && (from..to).contains(&minute_of_day)
        } else {
            /* Wrapping past midnight, which is the ordinary case.
             *
             * The day is the day the window *opened*, not the day it is now:
             * a Friday-night window that runs to 07:00 is still the Friday
             * window at two in the morning, and checking today's date at
             * that point would close it. */
            if minute_of_day >= from {
                self.days.contains(&weekday)
            } else if minute_of_day < to {
                let yesterday = (weekday + 6) % 7;
                self.days.contains(&yesterday)
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Overnight, and therefore wrapping — which is the case the arithmetic
    /// is for. The default window, 01:00 to 07:00, does not wrap at all.
    fn nightly() -> Schedule {
        Schedule {
            enabled: true,
            from: "23:00".into(),
            to: "07:00".into(),
            days: (0..=6).collect(),
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
