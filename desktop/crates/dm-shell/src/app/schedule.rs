// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The schedule's own rules, apart from any window: when its window opens and
//! closes, whether work is left in it, which rows each schedule looks after,
//! and when the downloads it ran are over (after their tries again). What is
//! done at those moments (arming When done, a sentence, the settings that
//! change) is the presentation's, which drives these.

use std::sync::Arc;

use dm_core::{Entry, State};

/// Watches the window open and close between two reads of the list.
#[derive(Debug, Default)]
pub struct Watch {
    was_open: bool,
    /// The action armed from the schedule when the window opened, until it
    /// is used or the window closes.
    armed: Option<super::power::WhenDone>,
    /// Rounds of tries again made since the window opened.
    rounds: u8,
    /// Whether something was waiting to run since the window opened.
    had_work: bool,
    /// Whether the program to run at the end has been run in this window.
    ran: bool,
}

/// What a read of the list finds of the window.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// The window is as it was at the last read.
    Nothing,
    /// The window has just opened.
    Opened,
    /// The window has just closed.
    Closed,
}

/// What the end of the scheduled downloads calls for at a read of the list.
#[derive(Debug, PartialEq, Eq)]
pub enum AtEnd {
    /// Nothing is due.
    Nothing,
    /// Start the failed ones again: a round of tries again is owed.
    Retry,
    /// They are done: the program to run, then the hang-up.
    Done,
}

impl Watch {
    /// What this read finds of the window, `open` saying whether it is open
    /// now; opening starts its rounds of tries again and its run anew.
    pub fn step(&mut self, open: bool) -> Step {
        match (self.was_open, open) {
            (false, true) => {
                self.was_open = true;
                self.rounds = 0;
                self.had_work = false;
                self.ran = false;
                Step::Opened
            }
            (true, false) => {
                self.was_open = false;
                Step::Closed
            }
            _ => Step::Nothing,
        }
    }

    /// Whether the program to run at the end is due: the window is open, something
    /// was waiting in it, nothing is now, and it has not been run in this window.
    pub fn run_due(&mut self, open: bool, work_left: bool) -> bool {
        if !open {
            return false;
        }
        if work_left {
            self.had_work = true;
            return false;
        }
        if self.had_work && !self.ran {
            self.ran = true;
            return true;
        }
        false
    }

    /// At a read of the list: with nothing left to run and some failed, a
    /// round of tries again while rounds are left (`retries`); once none is
    /// owed, the end, once in a window after something ran in it. So the
    /// program and the hang-up come after the tries again, as reference A's
    /// "when done" comes after each file's retries, not between them.
    pub fn at_end(&mut self, open: bool, work_left: bool, failed: bool, retries: u8) -> AtEnd {
        if open && !work_left && failed && self.retry_round(retries) {
            return AtEnd::Retry;
        }
        if self.run_due(open, work_left) {
            AtEnd::Done
        } else {
            AtEnd::Nothing
        }
    }

    /// Keeps the schedule's When done as armed by it.
    pub fn arm(&mut self, action: super::power::WhenDone) {
        self.armed = Some(action);
    }

    /// The armed action, taken back: the window closed or it was spent.
    pub fn disarm(&mut self) -> Option<super::power::WhenDone> {
        self.armed.take()
    }

    /// Whether another round of tries again is owed: the person allowed
    /// `retries`, and this window has made fewer.
    pub fn retry_round(&mut self, retries: u8) -> bool {
        if self.rounds < retries.min(5) {
            self.rounds += 1;
            true
        } else {
            false
        }
    }
}

/// Whether anything is left to run: running, queued, or held by the schedule
/// (which starts again when it opens). What is stopped by a person or has
/// settled is not.
pub fn work_left(rows: &[Arc<Entry>]) -> bool {
    rows.iter().any(|r| {
        matches!(r.state, State::Running | State::Queued)
            || (r.state == State::Paused && r.paused_by_schedule)
    })
}

/// Whether `schedule` is closed with downloads waiting on it: queued, or
/// paused by it (what runs now is not waiting). A presentation says when
/// it next opens.
pub fn holds_waiting(schedule: &dm_core::Schedule, rows: &[Arc<Entry>]) -> bool {
    schedule.enabled
        && !schedule.open()
        && rows
            .iter()
            .any(|r| r.state == State::Queued || (r.state == State::Paused && r.paused_by_schedule))
}

/// The rows a schedule looks after: with `Some(queue)`, that queue's own
/// schedule's (its rows); with `None`, the one for every queue's (the rows of
/// every queue that has no schedule of its own).
pub fn looked_after(
    rows: &[Arc<Entry>],
    own: &std::collections::BTreeMap<String, dm_core::Schedule>,
    queue: Option<&str>,
) -> Vec<Arc<Entry>> {
    rows.iter()
        .filter(|r| match queue {
            Some(q) => r.queue == q,
            None => !own.contains_key(&r.queue),
        })
        .cloned()
        .collect()
}

/// The settings with each once-only date that is over put away, so its
/// schedule goes back to its weekly window (left set, its queue would stay
/// shut for good): the Main queue's and every queue's own. `None` when none
/// is over. `today` is (year, month, day) and `minute` the minute of the
/// day, by the computer's clock.
pub fn spent_once_put_away(
    settings: &crate::Settings,
    today: (i32, u8, u8),
    minute: u32,
) -> Option<crate::Settings> {
    let over = |s: &dm_core::Schedule| s.once_at.is_some() && s.once_over(today, minute);
    let main = over(&settings.schedule);
    let queues: Vec<&String> = settings
        .queue_schedules
        .iter()
        .filter(|(_, s)| over(s))
        .map(|(name, _)| name)
        .collect();
    if !main && queues.is_empty() {
        return None;
    }
    let mut wanted = settings.clone();
    if main {
        wanted.schedule.once_at = None;
    }
    for name in queues {
        if let Some(s) = wanted.queue_schedules.get_mut(name) {
            s.once_at = None;
        }
    }
    Some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting(state: State, held: bool) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": 1,
                "url": "https://e.com/a",
                "state": serde_json::to_value(state).unwrap(),
                "paused_by_schedule": held,
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "a",
            }))
            .unwrap(),
        )
    }

    #[test]
    fn a_closed_schedule_holds_what_waits_on_it_and_not_what_runs() {
        // Closed whenever the test runs: a once-only window long over (no day
        // ticked means every day, and the default 01:00 to 07:00 was open
        // when CI ran it at 04:50).
        let closed = dm_core::Schedule {
            enabled: true,
            once_at: Some("2000-01-01 01:00".into()),
            ..dm_core::Schedule::default()
        };
        assert!(!closed.open());
        assert!(holds_waiting(&closed, &[waiting(State::Queued, false)]));
        assert!(holds_waiting(&closed, &[waiting(State::Paused, true)]));
        assert!(!holds_waiting(&closed, &[waiting(State::Paused, false)]));
        assert!(!holds_waiting(&closed, &[waiting(State::Running, false)]));
        let off = dm_core::Schedule {
            enabled: false,
            ..closed
        };
        assert!(!holds_waiting(&off, &[waiting(State::Queued, false)]));
    }

    #[test]
    fn the_window_opening_and_closing_is_seen_once_each() {
        let mut watch = Watch::default();
        assert_eq!(watch.step(false), Step::Nothing);
        assert_eq!(watch.step(true), Step::Opened);
        assert_eq!(watch.step(true), Step::Nothing);
        assert_eq!(watch.step(false), Step::Closed);
        assert_eq!(watch.step(false), Step::Nothing);
    }

    #[test]
    fn tries_again_are_as_many_as_allowed_in_one_window_and_start_over_at_the_next() {
        let mut watch = Watch::default();
        watch.step(true);
        assert!(!watch.retry_round(0), "zero leaves it failed, as before");
        assert!(watch.retry_round(2));
        assert!(watch.retry_round(2));
        assert!(!watch.retry_round(2), "no more than two rounds");
        watch.step(false);
        watch.step(true);
        assert!(watch.retry_round(2), "a new window starts over");
        let mut capped = Watch::default();
        capped.step(true);
        let rounds = (0..10).filter(|_| capped.retry_round(200)).count();
        assert_eq!(rounds, 5, "never more than five");
    }

    #[test]
    fn an_armed_action_is_taken_back_once() {
        let mut watch = Watch::default();
        watch.arm(super::super::power::WhenDone::Sleep);
        assert_eq!(watch.disarm(), Some(super::super::power::WhenDone::Sleep));
        assert_eq!(watch.disarm(), None);
    }

    #[test]
    fn the_kept_words_name_the_actions_and_back() {
        use super::super::power::WhenDone;
        for w in WhenDone::ALL {
            assert_eq!(WhenDone::from_word(w.word()), w);
        }
        assert_eq!(WhenDone::from_word(""), WhenDone::Nothing);
        assert_eq!(WhenDone::from_word("reboot"), WhenDone::Nothing);
        assert_eq!(WhenDone::from_word(" sleep "), WhenDone::Sleep);
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn each_schedule_looks_after_its_own_rows() {
        let row = |id: dm_core::Id, queue: &str| -> Arc<Entry> {
            Arc::new(
                serde_json::from_value(serde_json::json!({
                    "id": id,
                    "url": "https://e.com/f.bin",
                    "state": serde_json::to_value(State::Paused).unwrap(),
                    "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                    "filename": "f.bin",
                    "queue": queue,
                }))
                .unwrap(),
            )
        };
        let rows = vec![
            row(1, ""),
            row(2, "Night"),
            row(3, "Music"),
            row(4, "Night"),
        ];
        let ids = |v: Vec<Arc<dm_core::Entry>>| v.iter().map(|r| r.id).collect::<Vec<_>>();
        let mut own = std::collections::BTreeMap::new();
        // No queue has a schedule of its own: the shared one looks after all.
        assert_eq!(ids(looked_after(&rows, &own, None)), [1, 2, 3, 4]);
        own.insert("Night".to_string(), dm_core::Schedule::default());
        assert_eq!(ids(looked_after(&rows, &own, None)), [1, 3]);
        assert_eq!(ids(looked_after(&rows, &own, Some("Night"))), [2, 4]);
    }
}

#[cfg(test)]
mod run_tests {
    use super::{AtEnd, Watch};

    #[test]
    fn the_end_of_the_scheduled_downloads_comes_after_their_tries_again() {
        let mut watch = Watch::default();
        watch.step(true);
        assert_eq!(
            watch.at_end(true, true, false, 2),
            AtEnd::Nothing,
            "running"
        );
        assert_eq!(
            watch.at_end(true, false, true, 2),
            AtEnd::Retry,
            "one failed"
        );
        assert_eq!(watch.at_end(true, true, false, 2), AtEnd::Nothing);
        assert_eq!(
            watch.at_end(true, false, true, 2),
            AtEnd::Retry,
            "the second round"
        );
        assert_eq!(watch.at_end(true, true, false, 2), AtEnd::Nothing);
        assert_eq!(
            watch.at_end(true, false, true, 2),
            AtEnd::Done,
            "no round left: done, failed or not"
        );
        assert_eq!(watch.at_end(true, false, true, 2), AtEnd::Nothing, "once");
        // None to try again: done as the queue runs dry.
        let mut clean = Watch::default();
        clean.step(true);
        assert_eq!(clean.at_end(true, true, false, 0), AtEnd::Nothing);
        assert_eq!(clean.at_end(true, false, true, 0), AtEnd::Done);
        // Shut: neither.
        let mut shut = Watch::default();
        assert_eq!(shut.at_end(false, false, true, 2), AtEnd::Nothing);
        assert_eq!(shut.at_end(false, true, false, 2), AtEnd::Nothing);
    }

    #[test]
    fn the_program_at_the_end_runs_once_after_something_ran_in_the_window() {
        let mut watch = Watch::default();
        watch.step(true);
        // Nothing waited in this window: nothing to do at its end.
        assert!(!watch.run_due(true, false));
        // Something runs, then the queue runs dry: due once.
        assert!(!watch.run_due(true, true));
        assert!(watch.run_due(true, false));
        assert!(!watch.run_due(true, false), "twice in one window");
        // Closed: never. A new window starts over.
        assert!(!watch.run_due(false, false));
        watch.step(false);
        watch.step(true);
        assert!(!watch.run_due(true, true));
        assert!(watch.run_due(true, false));
    }
}

#[cfg(test)]
mod spent_once_tests {
    use super::*;

    /// Overnight, 23:00 to 07:00, once only on the day `at` gives.
    fn once(at: &str) -> dm_core::Schedule {
        dm_core::Schedule {
            enabled: true,
            from: "23:00".into(),
            to: "07:00".into(),
            days: (0..=6).collect(),
            once_at: Some(at.to_string()),
            ..dm_core::Schedule::default()
        }
    }

    #[test]
    fn a_once_only_date_that_is_over_is_put_away_and_one_to_come_is_kept() {
        let mut settings = crate::Settings {
            schedule: once("2026-11-03 23:00"),
            ..crate::Settings::default()
        };
        settings
            .queue_schedules
            .insert("Night".into(), once("2026-11-02 23:00"));
        settings
            .queue_schedules
            .insert("Later".into(), once("2026-12-01 23:00"));
        // 4 November, 06:59: the Main queue's night is not over; Night's is.
        let wanted = spent_once_put_away(&settings, (2026, 11, 4), 6 * 60 + 59).unwrap();
        assert!(wanted.schedule.once_at.is_some(), "not over yet");
        assert_eq!(wanted.queue_schedules["Night"].once_at, None);
        assert!(wanted.queue_schedules["Later"].once_at.is_some());
        // 07:00: the Main queue's is over too.
        let wanted = spent_once_put_away(&wanted, (2026, 11, 4), 7 * 60).unwrap();
        assert_eq!(wanted.schedule.once_at, None);
        assert!(wanted.queue_schedules["Later"].once_at.is_some());
        assert!(
            spent_once_put_away(&wanted, (2026, 11, 4), 8 * 60).is_none(),
            "nothing left over"
        );
    }
}
