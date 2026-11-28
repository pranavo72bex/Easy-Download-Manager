// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Rows from elsewhere taken into the list: a backup's, restored (gaps row W16).

use super::*;

impl DownloadList {
    /// Adds `rows` from elsewhere (a backup restored, on this computer or another) at the end of
    /// the list, each under a number of its own, and writes the queue once: their ids, in order.
    ///
    /// Nothing of them touches a file on this computer ([`adopted`]): a finished row is the
    /// record it was, an unfinished one waits, paused, to be fetched again from its start.
    pub async fn adopt(&self, rows: Vec<Entry>) -> Vec<Id> {
        if rows.is_empty() {
            return Vec::new();
        }
        let mut ids = Vec::with_capacity(rows.len());
        {
            let mut slots = self.inner.slots.lock().await;
            let mut order = self.inner.order.lock().await;
            for entry in rows {
                let entry = adopted(entry, self.inner.next_id.fetch_add(1, Ordering::SeqCst));
                let id = entry.id;
                let own_limit =
                    crate::limit::Limiter::new(entry.speed_limit.saturating_mul(crate::KB));
                slots.insert(
                    id,
                    Slot {
                        entry: Arc::new(entry),
                        control: Control::new(),
                        task: None,
                        outside_schedule: false,
                        retrying: None,
                        swarm: None,
                        own_limit,
                    },
                );
                order.push(id);
                ids.push(id);
            }
        }
        self.save().await;
        ids
    }
}

/// `entry` as a row of this list numbered `id`, made so that it touches no file here: a finished
/// row keeps where its file was written (Open finds it, or says it is gone) but not "Delete file
/// after", which would delete whatever is at that path now; an unfinished one starts again from
/// nothing, paused until a person resumes it, as its parts were another computer's (or this
/// one's, under its old number); and a newer version a recheck was fetching replaces nothing.
fn adopted(mut entry: Entry, id: Id) -> Entry {
    entry.id = id;
    if entry.state != State::Completed {
        if matches!(entry.state, State::Running | State::Queued) {
            entry.state = State::Paused;
        }
        entry.progress.downloaded = 0;
        entry.parts_dir.clear();
        entry.began_in.clear();
        entry.stalled.clear();
    }
    entry.paused_by_schedule = false;
    entry.delete_after = None;
    entry.replaces = None;
    entry.post = None;
    entry.progress.speed = 0.0;
    entry.progress.eta = None;
    if entry.fail_kind.is_none() {
        entry.fail_kind = entry.failed_as();
    }
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: State) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": 7,
            "url": "http://127.0.0.1:9/a.bin",
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": 5, "total": 10, "speed": 3.0, "eta": null},
            "filename": "a.bin",
            "parts_dir": "/elsewhere/parts/7",
            "began_in": "/elsewhere",
            "delete_after": 60,
            "replaces": "/elsewhere/a.bin",
        }))
        .unwrap()
    }

    /// A row taken in touches no file here: no deletion after, no replacing, no parts of
    /// another's; a running one waits, paused, and a finished one keeps its record.
    #[test]
    fn a_row_taken_in_touches_no_file_here() {
        let running = adopted(row(State::Running), 40);
        assert_eq!((running.id, &running.state), (40, &State::Paused));
        assert_eq!(running.progress.downloaded, 0);
        assert!(running.parts_dir.is_empty() && running.began_in.is_empty());
        assert_eq!((running.delete_after, &running.replaces), (None, &None));
        let done = adopted(row(State::Completed), 41);
        assert_eq!((done.id, &done.state), (41, &State::Completed));
        assert_eq!(done.progress.downloaded, 5);
        assert!(done.progress.speed.abs() < f64::EPSILON);
        assert_eq!((done.delete_after, &done.replaces), (None, &None));
    }
}
