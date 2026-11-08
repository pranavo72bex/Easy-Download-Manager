// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What happened between two reads of the queue: the application's events,
//! found by comparing each `Snapshot` with what was already told, so none is
//! told twice and none is lost to a missed read.
//!
//! A presentation reads the rows, hands them to its [`Detector`], and reacts
//! to the [`Detected`] in its own way (a toast, a sound, a dialog). The first
//! read seeds it: what had already finished or failed before dm started is
//! remembered, not announced.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dm_core::{Entry, Id, State};

/// What happened since the last read, each in the rows' order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Detected {
    /// Rows that finished.
    pub finished: Vec<Id>,
    /// Rows that failed, once per failure: a row retried and back in line is
    /// told again when it fails again.
    pub failed: Vec<Id>,
    /// Of the failed, those whose signed link ran out part-way
    /// ([`super::rules::link_expired`]), to be asked for a fresh one.
    pub expired: Vec<Id>,
    /// Rows that appeared already running or waiting, started by something
    /// other than this presentation (a browser's hand-off, a second launch).
    pub started_elsewhere: Vec<Id>,
    /// A new row whose address was already in the list (a browser's hand-off
    /// added by the inbox before the window heard of it).
    pub added_again: Option<Id>,
}

/// Remembers what was told, between reads.
#[derive(Debug, Default)]
pub struct Detector {
    finished_told: HashSet<Id>,
    failed_told: HashSet<Id>,
    last: HashMap<Id, State>,
    before: HashMap<Id, State>,
    own: HashSet<Id>,
    ready: bool,
}

impl Detector {
    /// A row this presentation added itself: not "started elsewhere", not
    /// "added again".
    pub fn own(&mut self, id: Id) {
        self.own.insert(id);
    }

    /// Whether the first read has been made: after it, every completion
    /// already on disk is seeded and later ones are told.
    pub fn ready(&self) -> bool {
        self.ready
    }

    /// Each row's state at the read before the latest: what a watch that
    /// runs after a read compares the latest rows with (the queue running
    /// dry, for When done).
    pub fn before(&self) -> &HashMap<Id, State> {
        &self.before
    }

    /// Whether the last read had the row `id`.
    pub fn knew(&self, id: Id) -> bool {
        self.last.contains_key(&id)
    }

    /// Compares `rows` with the last read, and remembers them for the next.
    pub fn read(&mut self, rows: &[Arc<Entry>]) -> Detected {
        let mut detected = Detected::default();
        for row in rows {
            if !matches!(row.state, State::Failed(_)) {
                self.failed_told.remove(&row.id);
            }
            if matches!(row.state, State::Failed(_))
                && self.failed_told.insert(row.id)
                && self.ready
            {
                detected.failed.push(row.id);
                if super::rules::link_expired(row) {
                    detected.expired.push(row.id);
                }
            }
            if row.state == State::Completed && self.finished_told.insert(row.id) && self.ready {
                detected.finished.push(row.id);
            }
        }
        if self.ready {
            let fresh =
                |row: &&Arc<Entry>| !self.last.contains_key(&row.id) && !self.own.contains(&row.id);
            detected.started_elsewhere = rows
                .iter()
                .filter(fresh)
                .filter(|row| matches!(row.state, State::Running | State::Queued))
                .map(|row| row.id)
                .collect();
            detected.added_again = rows
                .iter()
                .filter(fresh)
                .find(|row| {
                    let address = crate::lists::bare(&row.url);
                    rows.iter().any(|other| {
                        other.id != row.id
                            && other.state != State::Cancelled
                            && self.last.contains_key(&other.id)
                            && crate::lists::bare(&other.url) == address
                    })
                })
                .map(|row| row.id);
        }
        let ids: HashSet<Id> = rows.iter().map(|r| r.id).collect();
        self.own.retain(|id| ids.contains(id));
        self.before = std::mem::replace(
            &mut self.last,
            rows.iter().map(|r| (r.id, r.state.clone())).collect(),
        );
        self.ready = true;
        detected
    }
}

/// What "When done" watches the list for, apart from the window so it can
/// be tested: what counts as done is a stretch
/// that had something downloading and finished or failed something, now with
/// nothing busy: a queue a person stopped is not done.
#[derive(Debug, Default)]
pub struct DryWatch {
    /// Something was downloading since the queue last ran dry.
    busy_seen: bool,
    /// A row finished or failed since the queue last ran dry.
    settled_seen: bool,
    /// The list has been looked at once: rows already finished then are
    /// history, not news.
    primed: bool,
}

impl DryWatch {
    /// One look at the list, with each row's state at the last look. True
    /// once, when a stretch of downloading has just ended with something
    /// finished or failed and nothing left that dm will finish by itself.
    /// Whether the queue has just run dry, comparing `rows` with `last`, the
    /// states at the read before.
    pub fn look<R: std::borrow::Borrow<Entry>>(
        &mut self,
        rows: &[R],
        last: &HashMap<Id, State>,
    ) -> bool {
        let rows: Vec<&Entry> = rows.iter().map(|r| r.borrow()).collect();
        let settled = rows.iter().any(|r| {
            matches!(r.state, State::Completed | State::Failed(_))
                && last.get(&r.id).is_some_and(|was| *was != r.state)
        });
        if settled {
            self.settled_seen = true;
        }
        // A row added and finished between two looks: it was downloading,
        // though no look saw it so, and it settled. A 1 MB file handed over
        // under When done › Exit never fired it.
        let primed = std::mem::replace(&mut self.primed, true);
        if primed
            && rows.iter().any(|r| {
                matches!(r.state, State::Completed | State::Failed(_)) && !last.contains_key(&r.id)
            })
        {
            self.settled_seen = true;
            self.busy_seen = true;
        }
        let busy = rows.iter().any(|r| match r.state {
            State::Running | State::Queued => true,
            State::Paused => r.paused_by_schedule,
            _ => false,
        });
        if busy {
            self.busy_seen = true;
            return false;
        }
        // Both taken at every dry look, so neither outlives the stretch it
        // was seen in: a row that settled while nothing was busy (a late
        // outcome written over a paused row) once left `settled_seen` set,
        // and a later stretch ended by Stop all fired.
        let busy_seen = std::mem::take(&mut self.busy_seen);
        let settled_seen = std::mem::take(&mut self.settled_seen);
        busy_seen && settled_seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: Id, url: &str, state: State) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": id,
                "url": url,
                "state": serde_json::to_value(&state).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "f",
            }))
            .unwrap(),
        )
    }

    /// What had finished or failed before the first read is remembered, not
    /// told; later finishes and failures are told once each.
    #[test]
    fn the_first_read_seeds_and_later_ones_tell_each_change_once() {
        let mut detector = Detector::default();
        let first = detector.read(&[
            row(1, "https://h.example/a", State::Completed),
            row(2, "https://h.example/b", State::Failed("x".into())),
            row(3, "https://h.example/c", State::Running),
        ]);
        assert_eq!(first, Detected::default());
        assert!(detector.ready() && detector.knew(3));
        let rows = [
            row(1, "https://h.example/a", State::Completed),
            row(2, "https://h.example/b", State::Failed("x".into())),
            row(3, "https://h.example/c", State::Completed),
        ];
        assert_eq!(detector.read(&rows).finished, vec![3]);
        assert_eq!(detector.read(&rows), Detected::default(), "told once");
    }

    /// A row retried and failing again is told again.
    #[test]
    fn a_retried_row_that_fails_again_is_told_again() {
        let mut detector = Detector::default();
        detector.read(&[row(1, "https://h.example/a", State::Running)]);
        let failed = [row(1, "https://h.example/a", State::Failed("x".into()))];
        assert_eq!(detector.read(&failed).failed, vec![1]);
        assert!(detector.read(&failed).failed.is_empty());
        detector.read(&[row(1, "https://h.example/a", State::Queued)]);
        assert_eq!(detector.read(&failed).failed, vec![1]);
    }

    /// A new running row this presentation did not add was started elsewhere;
    /// one it added is not. A new row of a listed address is added again.
    #[test]
    fn rows_started_elsewhere_and_added_again_are_told_and_own_ones_are_not() {
        let mut detector = Detector::default();
        detector.read(&[row(1, "https://h.example/a", State::Completed)]);
        detector.own(3);
        let detected = detector.read(&[
            row(1, "https://h.example/a", State::Completed),
            row(2, "https://h.example/b", State::Running),
            row(3, "https://h.example/c", State::Running),
            row(4, "https://h.example/a", State::Queued),
        ]);
        assert_eq!(detected.started_elsewhere, vec![2, 4]);
        assert_eq!(detected.added_again, Some(4));
    }
}

#[cfg(test)]
mod dry_tests {
    use super::*;

    fn row(id: Id, state: State) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": id, "url": "https://e.com/f.bin",
            "state": serde_json::to_value(&state).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "f.bin",
        }))
        .unwrap()
    }

    fn states(rows: &[Entry]) -> HashMap<Id, State> {
        rows.iter().map(|r| (r.id, r.state.clone())).collect()
    }

    /// A row that settled while nothing was busy (a late outcome
    /// over a paused row) must not count for a later stretch that a person
    /// ended with Stop all.
    #[test]
    fn a_settle_seen_while_idle_does_not_carry_into_the_next_stretch() {
        let mut watch = DryWatch::default();
        let paused = vec![row(1, State::Paused)];
        let mut last = states(&paused);
        let done = vec![row(1, State::Completed)];
        assert!(!watch.look(&done, &last), "nothing was busy");
        last = states(&done);

        // A new stretch, ended by a person's stop with nothing finished.
        let running = vec![row(1, State::Completed), row(2, State::Running)];
        assert!(!watch.look(&running, &last));
        last = states(&running);
        let stopped = vec![row(1, State::Completed), row(2, State::Paused)];
        assert!(
            !watch.look(&stopped, &last),
            "fired on a queue a person stopped"
        );
    }

    #[test]
    fn a_stretch_that_finishes_something_fires_once() {
        let mut watch = DryWatch::default();
        let running = vec![row(1, State::Running)];
        assert!(!watch.look(&running, &HashMap::new()));
        let done = vec![row(1, State::Completed)];
        assert!(watch.look(&done, &states(&running)));
        assert!(!watch.look(&done, &states(&done)), "fired twice");
    }

    /// A small file added and finished between two looks is a
    /// stretch that finished something; rows already finished at the first
    /// look are history.
    #[test]
    fn a_download_that_came_and_went_between_two_looks_fires() {
        let mut watch = DryWatch::default();
        let history = vec![row(1, State::Completed)];
        assert!(!watch.look(&history, &HashMap::new()), "history fired");
        let last = states(&history);
        let quick = vec![row(1, State::Completed), row(2, State::Completed)];
        assert!(watch.look(&quick, &last));
        assert!(!watch.look(&quick, &states(&quick)), "fired twice");
    }
}
