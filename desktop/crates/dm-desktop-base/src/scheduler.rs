// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Scheduler and the Speed limiter as they are being edited, for every
//! window (B49, B62, B65, B20): every queue's hours, the queue whose hours
//! show, and what cannot be kept; the limiter's slower hours checked. What
//! each window draws is its own.

use std::collections::BTreeMap;

use dm_core::{Schedule, MAIN_QUEUE};
use dm_shell::Settings;

use crate::i18n::{t, tf};

/// The Scheduler as it is being edited: every queue's hours, and the queue
/// whose hours show, as reference A's lists its queues beside their
/// schedule.
#[derive(Debug, Clone)]
pub struct SchedulerDraft {
    /// The queue chosen in the list.
    pub queue: String,
    /// The hours shown: the chosen queue's.
    pub schedule: Schedule,
    /// Whether the chosen queue has hours of its own. The Main queue's are
    /// the ones every queue without its own follows, so it always has.
    pub own: bool,
    /// Started once, on `date`, rather than every week on the days ticked.
    pub once: bool,
    /// The date of the once-only start, `YYYY-MM-DD`.
    pub date: String,
    /// The Main queue's hours, which every queue without its own follows.
    pub shared: Schedule,
    /// The queues with hours of their own.
    pub queues: BTreeMap<String, Schedule>,
    /// What is wrong with what was typed, said under it.
    pub says: String,
    /// Whether the queues stand beside the hours, decided once, as it
    /// first opens: a dialog that is a window of its own reads its own size
    /// in its own frames, and decided each frame it flipped between the two
    /// layouts, its buttons moving so that no click took.
    pub beside: Option<bool>,
}

impl SchedulerDraft {
    /// The Scheduler over these settings, showing `queue`'s hours.
    pub fn new(settings: &Settings, queue: &str) -> Self {
        // No days kept means every day, shown as every day ticked.
        let every_day = |mut s: Schedule| {
            if s.days.is_empty() {
                s.days = (0..=6).collect();
            }
            s
        };
        let mut draft = SchedulerDraft {
            queue: String::new(),
            schedule: Schedule::default(),
            own: true,
            once: false,
            date: String::new(),
            shared: every_day(settings.schedule.clone()),
            // The Main queue's hours are the shared ones; an entry of its
            // own (only a settings file written by hand has one) is not
            // shown, so it is not kept.
            queues: settings
                .queue_schedules
                .iter()
                .filter(|(name, _)| name.as_str() != MAIN_QUEUE)
                .map(|(name, s)| (name.clone(), every_day(s.clone())))
                .collect(),
            says: String::new(),
            beside: None,
        };
        draft.load(queue);
        draft
    }

    /// Shows `queue`'s hours.
    pub fn load(&mut self, queue: &str) {
        self.queue = queue.to_string();
        let (schedule, own) = if queue == MAIN_QUEUE {
            (self.shared.clone(), true)
        } else if let Some(own) = self.queues.get(queue) {
            (own.clone(), true)
        } else {
            (self.shared.clone(), false)
        };
        self.schedule = schedule;
        self.own = own;
        // A once-only start is shown as its date, its time in Start at.
        let once = self.schedule.once_at.clone();
        match once.as_deref().and_then(|at| at.trim().split_once(' ')) {
            Some((date, time)) => {
                self.once = true;
                self.date = date.to_string();
                self.schedule.from = time.trim().to_string();
            }
            None => {
                self.once = false;
                self.date = chrono::Local::now().format("%Y-%m-%d").to_string();
            }
        }
    }

    /// Keeps what the form shows as the chosen queue's hours.
    pub fn store(&mut self) {
        let mut schedule = self.schedule.clone();
        schedule.once_at = self
            .once
            .then(|| format!("{} {}", self.date.trim(), schedule.from.trim()));
        if self.queue == MAIN_QUEUE {
            self.shared = schedule;
        } else if self.own {
            self.queues.insert(self.queue.clone(), schedule);
        } else {
            self.queues.remove(&self.queue);
        }
    }

    /// The first queue whose hours cannot be kept, and why; Main first.
    pub fn problem(&self) -> Option<(String, String)> {
        std::iter::once((MAIN_QUEUE, &self.shared))
            .chain(self.queues.iter().map(|(name, s)| (name.as_str(), s)))
            .find_map(|(name, s)| schedule_problem(s).map(|why| (name.to_string(), why)))
    }
}

/// Why these hours cannot be kept, where they cannot.
pub fn schedule_problem(schedule: &Schedule) -> Option<String> {
    if !schedule.enabled {
        return None;
    }
    if !(dm_core::is_time(&schedule.from) && dm_core::is_time(&schedule.to)) {
        // Refused here, where it was typed: saved, "25:00" or "1am" turned
        // the schedule off without a word.
        return Some(tf(
            "\"{}\" to \"{}\": times are HH:MM in 24 hours, such as 01:00 or 23:30.",
            &[&schedule.from, &schedule.to],
        ));
    }
    match schedule.once_at.as_deref() {
        Some(at) => {
            let date = at.trim().split(' ').next().unwrap_or_default();
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .is_err()
                .then(|| {
                    tf(
                        "\"{}\" is not a date: write it as YYYY-MM-DD, such as 2026-12-31.",
                        &[&date],
                    )
                })
        }
        None if schedule.days.is_empty() => Some(t("Tick at least one day.").to_string()),
        None => None,
    }
}

/// Why the Speed limiter's slower hours cannot be kept, where they cannot:
/// on, with a time that is not one.
pub fn slow_hours_problem(slow: &dm_shell::settings::SlowHours) -> Option<String> {
    (slow.enabled && !(dm_core::is_time(&slow.from) && dm_core::is_time(&slow.to))).then(|| {
        tf(
            "\"{}\" to \"{}\": times are HH:MM in 24 hours, such as 01:00 or 23:30.",
            &[&slow.from, &slow.to],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_without_hours_follows_the_main_queues_and_a_bad_time_is_said() {
        let settings = Settings::default();
        let mut draft = SchedulerDraft::new(&settings, "Night");
        assert!(
            !draft.own,
            "a queue with no hours of its own follows Main's"
        );
        draft.own = true;
        draft.schedule.enabled = true;
        draft.schedule.from = "25:00".into();
        draft.store();
        let (queue, why) = draft.problem().expect("a time that is not one");
        assert_eq!(queue, "Night");
        assert!(why.contains("25:00"), "{why}");
        draft.load(MAIN_QUEUE);
        assert!(draft.own, "the Main queue always has hours of its own");
    }
}
