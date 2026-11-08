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
