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
pub struct SavedSizes(HashMap<Id, (String, u64)>);

impl SavedSizes {
    /// Gives each finished row of `rows` its file's size on disk, as
    /// downloaded and as the whole. A row is copied only where the figures
    /// differ: an unchanged row stays the queue's own, which is how the
    /// next read knows it. Rows no longer finished are forgotten.
    pub fn apply(&mut self, rows: &mut [Arc<Entry>]) {
        use dm_core::State;
        self.0.retain(|id, _| {
            rows.iter()
                .any(|r| r.id == *id && r.state == State::Completed)
        });
        for row in rows.iter_mut() {
            if row.state != State::Completed || row.path.is_empty() {
                continue;
            }
            let known = self.0.get(&row.id).filter(|(path, _)| *path == row.path);
            let size = match known {
                Some((_, size)) => *size,
                None => match std::fs::metadata(&row.path) {
                    Ok(meta) if meta.is_file() => {
                        self.0.insert(row.id, (row.path.clone(), meta.len()));
                        meta.len()
                    }
                    _ => continue,
                },
            };
            if row.progress.total != Some(size) || row.progress.downloaded != size {
                let row = Arc::make_mut(row);
                row.progress.total = Some(size);
                row.progress.downloaded = size;
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
    }
}
