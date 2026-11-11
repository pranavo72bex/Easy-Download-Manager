// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a presentation draws from, read from the queue at once: the rows and
//! what each row's line says beside its state.
//!
//! The rows are shared, not copied (`Arc<Entry>`): a read of ten thousand rows
//! copies ten thousand pointers, and a row's own bytes are copied only where
//! the queue changed it. A presentation reads a snapshot when it is told of a
//! change (`changes`), never per frame.

use std::collections::HashMap;
use std::sync::Arc;

use dm_core::{Entry, Id, Swarm};

/// A download waiting to be tried again: its try, of how many, why the last
/// failed, and when the next is due.
pub type Retry = (u32, u32, String, std::time::Instant);

/// The queue as a presentation draws it.
#[derive(Clone, Default)]
pub struct Snapshot {
    /// Every row, in the queue's order.
    pub rows: Vec<Arc<Entry>>,
    /// The rows waiting to be tried again, by id.
    pub retrying: HashMap<Id, Retry>,
    /// What each torrent's swarm holds, by id.
    pub swarms: HashMap<Id, Swarm>,
    /// Whether the daily allowance is spent, holding the rest of the queue.
    pub allowance_spent: bool,
}

/// Each finished file's size on disk, by the path it was read at: a stream
/// put into an MP4 is not the size of the joined stream the engine counted.
/// Read once a path.
#[derive(Debug, Default)]
pub struct SavedSizes(HashMap<Id, Saved>);

/// A finished file's size, by the path it was read at, and the row last
/// given it: the queue's row and the copy with the size, handed out again
/// while the queue's is the same, so a read that changed nothing hands out
/// the same rows and a window can tell by their pointers (gaps row 181).
#[derive(Debug)]
struct Saved {
    path: String,
    size: u64,
    given: Option<(Arc<Entry>, Arc<Entry>)>,
}

impl SavedSizes {
    /// Gives each finished row of `rows` its file's size on disk, as
    /// downloaded and as the whole. A row is copied only where the figures
    /// differ: an unchanged row stays the queue's own, which is how the
    /// next read knows it. Rows no longer finished are forgotten.
    pub fn apply(&mut self, rows: &mut [Arc<Entry>]) {
        use dm_core::State;
        // Gathered once: a search of every row for each size kept was ten
        // thousand finished rows times ten thousand rows, at every read.
        let finished: std::collections::HashSet<Id> = rows
            .iter()
            .filter(|r| r.state == State::Completed)
            .map(|r| r.id)
            .collect();
        self.0.retain(|id, _| finished.contains(id));
        for row in rows.iter_mut() {
            if row.state != State::Completed || row.path.is_empty() {
                continue;
            }
            if !self
                .0
                .get(&row.id)
                .is_some_and(|saved| saved.path == row.path)
            {
                match std::fs::metadata(&row.path) {
                    Ok(meta) if meta.is_file() => {
                        self.0.insert(
                            row.id,
                            Saved {
                                path: row.path.clone(),
                                size: meta.len(),
                                given: None,
                            },
                        );
                    }
                    _ => continue,
                }
            }
            let Some(saved) = self.0.get_mut(&row.id) else {
                continue;
            };
            let size = saved.size;
            if row.progress.total != Some(size) || row.progress.downloaded != size {
                if let Some((queues, given)) = &saved.given {
                    if Arc::ptr_eq(queues, row) {
                        *row = Arc::clone(given);
                        continue;
                    }
                }
                let queues = Arc::clone(row);
                let copy = Arc::make_mut(row);
                copy.progress.total = Some(size);
                copy.progress.downloaded = size;
                saved.given = Some((queues, Arc::clone(row)));
            }
        }
    }

    /// Forgets every size read, so each is read again.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_row_takes_its_files_size_on_disk_read_once_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("joined.mp4");
        std::fs::write(&path, vec![0u8; 700]).unwrap();
        let row = |state: dm_core::State| -> Arc<Entry> {
            Arc::new(
                serde_json::from_value(serde_json::json!({
                    "id": 1,
                    "url": "http://example.com/v",
                    "state": serde_json::to_value(state).unwrap(),
                    "progress": {"downloaded": 900, "total": 900, "speed": 0.0, "eta": null},
                    "filename": "joined.mp4",
                    "path": path.display().to_string(),
                }))
                .unwrap(),
            )
        };
        let mut sizes = SavedSizes::default();
        let mut running = vec![row(dm_core::State::Running)];
        sizes.apply(&mut running);
        assert_eq!(
            running[0].progress.total,
            Some(900),
            "not finished: as counted"
        );
        let mut rows = vec![row(dm_core::State::Completed)];
        sizes.apply(&mut rows);
        assert_eq!(
            (rows[0].progress.downloaded, rows[0].progress.total),
            (700, Some(700))
        );
        // Read once: a change on disk is not seen until the sizes are cleared.
        std::fs::write(&path, vec![0u8; 1000]).unwrap();
        let mut again = vec![row(dm_core::State::Completed)];
        sizes.apply(&mut again);
        assert_eq!(again[0].progress.total, Some(700));
        sizes.clear();
        sizes.apply(&mut again);
        assert_eq!(again[0].progress.total, Some(1000));
        // The same figures: the row is not copied.
        let before = Arc::clone(&again[0]);
        sizes.apply(&mut again);
        assert!(Arc::ptr_eq(&before, &again[0]));
        // The queue's row, read again unchanged, is given the same copy:
        // a read that changed nothing hands out the same rows.
        let queues = row(dm_core::State::Completed);
        let mut first = vec![Arc::clone(&queues)];
        sizes.apply(&mut first);
        let mut second = vec![Arc::clone(&queues)];
        sizes.apply(&mut second);
        assert_eq!(second[0].progress.total, Some(1000));
        assert!(Arc::ptr_eq(&first[0], &second[0]));
    }

    /// A read costs each finished row once, not once per row of the list:
    /// ten thousand finished rows were about 10^8 comparisons a read (gaps
    /// row 181), seconds in a debug build; now a few milliseconds.
    #[test]
    fn ten_thousand_finished_rows_are_read_in_linear_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, vec![0u8; 10]).unwrap();
        let mut rows: Vec<Arc<Entry>> = (1..=10_000u64)
            .map(|id| {
                Arc::new(
                    serde_json::from_value(serde_json::json!({
                        "id": id,
                        "url": "http://example.com/f",
                        "state": serde_json::to_value(dm_core::State::Completed).unwrap(),
                        "progress": {"downloaded": 10, "total": 10, "speed": 0.0, "eta": null},
                        "filename": "f.bin",
                        "path": path.display().to_string(),
                    }))
                    .unwrap(),
                )
            })
            .collect();
        let mut sizes = SavedSizes::default();
        sizes.apply(&mut rows);
        let started = std::time::Instant::now();
        for _ in 0..5 {
            sizes.apply(&mut rows);
        }
        let each = started.elapsed() / 5;
        assert!(
            each < std::time::Duration::from_millis(100),
            "a read of 10,000 finished rows took {each:?}"
        );
    }
}
