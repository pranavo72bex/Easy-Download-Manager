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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use dm_core::{Entry, Id, State, Swarm};

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

/// Which rows of a read are not the entries the read before had, found by
/// pointer: the queue hands out a row it did not change as the same shared
/// entry, and copies one before changing it while a read still holds it, so a
/// read that keeps the last read's rows can tell what changed without looking
/// inside a row it did not (gaps row 181).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Changes {
    /// Rows came, went or moved, or there was no read before: every row is
    /// to be looked at.
    #[default]
    Every,
    /// The same rows in the same order: these places, in order, hold
    /// another entry than they did.
    At(Vec<usize>),
}

impl Changes {
    /// How `now` differs from `before`, the rows of the read before, which
    /// the caller held since (so none of their entries was changed in place).
    pub fn between(before: &[Arc<Entry>], now: &[Arc<Entry>]) -> Changes {
        if before.len() != now.len() {
            return Changes::Every;
        }
        let mut at = Vec::new();
        for (i, (was, is)) in before.iter().zip(now).enumerate() {
            if Arc::ptr_eq(was, is) {
                continue;
            }
            if was.id != is.id {
                return Changes::Every;
            }
            at.push(i);
        }
        Changes::At(at)
    }

    /// The places that changed, where the rows are the same in the same
    /// order; none where every row is to be looked at.
    pub fn at(&self) -> Option<&[usize]> {
        match self {
            Changes::Every => None,
            Changes::At(at) => Some(at),
        }
    }
}

/// Each finished file's size on disk, by the path it was read at: a stream
/// put into an MP4 is not the size of the joined stream the engine counted.
/// Read once a path.
#[derive(Debug, Default)]
pub struct SavedSizes {
    sizes: HashMap<Id, Saved>,
    /// The places of the finished rows whose file could not be read at the
    /// last apply: looked for again at each, as every row once was.
    missing: BTreeMap<usize, Id>,
}

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

/// What [`SavedSizes::give`] made of a row.
#[derive(Debug, PartialEq, Eq)]
enum Given {
    /// Not finished, or no file named: as the queue has it.
    Not,
    /// Finished, but its file could not be read.
    Missing,
    /// Given its file's size.
    Sized,
}

impl SavedSizes {
    /// Gives each finished row of `rows` its file's size on disk, as
    /// downloaded and as the whole. A row is copied only where the figures
    /// differ: an unchanged row stays the queue's own, which is how the
    /// next read knows it. Rows no longer finished are forgotten.
    pub fn apply(&mut self, rows: &mut [Arc<Entry>]) {
        // Gathered once: a search of every row for each size kept was ten
        // thousand finished rows times ten thousand rows, at every read.
        let finished: HashSet<Id> = rows
            .iter()
            .filter(|r| r.state == State::Completed)
            .map(|r| r.id)
            .collect();
        self.sizes.retain(|id, _| finished.contains(id));
        self.missing.clear();
        for (at, row) in rows.iter_mut().enumerate() {
            if self.give(row) == Given::Missing {
                self.missing.insert(at, row.id);
            }
        }
    }

    /// [`Self::apply`] to `rows` where only those at the places `at` (in
    /// order) are new since the last apply, the rest the rows that apply
    /// left, in the same order: only those at `at` are looked at, and the
    /// rows whose file was missing looked for again, as `apply` looks for
    /// them. At 10,000 finished rows, a read where a few changed had looked
    /// every one up (gaps row 181). Where a missing file is found now, the
    /// row's place, not among `at`.
    pub fn apply_at(&mut self, rows: &mut [Arc<Entry>], at: &[usize]) -> Vec<usize> {
        for &i in at {
            self.missing.remove(&i);
            let Some(row) = rows.get_mut(i) else {
                continue;
            };
            if row.state != State::Completed {
                self.sizes.remove(&row.id);
                continue;
            }
            if self.give(row) == Given::Missing {
                self.missing.insert(i, row.id);
            }
        }
        let again: Vec<(usize, Id)> = self
            .missing
            .iter()
            .filter(|(i, _)| at.binary_search(*i).is_err())
            .map(|(i, id)| (*i, *id))
            .collect();
        let mut found = Vec::new();
        for (i, id) in again {
            match rows.get_mut(i) {
                Some(row) if row.id == id => {
                    if self.give(row) == Given::Sized {
                        self.missing.remove(&i);
                        found.push(i);
                    }
                }
                _ => {
                    self.missing.remove(&i);
                }
            }
        }
        found
    }

    /// Gives `row` its file's size, where it is finished: read once a path,
    /// and the same copy while the queue's row is the same.
    fn give(&mut self, row: &mut Arc<Entry>) -> Given {
        if row.state != State::Completed || row.path.is_empty() {
            return Given::Not;
        }
        if !self
            .sizes
            .get(&row.id)
            .is_some_and(|saved| saved.path == row.path)
        {
            match std::fs::metadata(&row.path) {
                Ok(meta) if meta.is_file() => {
                    self.sizes.insert(
                        row.id,
                        Saved {
                            path: row.path.clone(),
                            size: meta.len(),
                            given: None,
                        },
                    );
                }
                _ => return Given::Missing,
            }
        }
        let Some(saved) = self.sizes.get_mut(&row.id) else {
            return Given::Missing;
        };
        let size = saved.size;
        if row.progress.total != Some(size) || row.progress.downloaded != size {
            if let Some((queues, given)) = &saved.given {
                if Arc::ptr_eq(queues, row) {
                    *row = Arc::clone(given);
                    return Given::Sized;
                }
            }
            let queues = Arc::clone(row);
            let copy = Arc::make_mut(row);
            copy.progress.total = Some(size);
            copy.progress.downloaded = size;
            saved.given = Some((queues, Arc::clone(row)));
        }
        Given::Sized
    }

    /// Forgets every size read, so each is read again.
    pub fn clear(&mut self) {
        self.sizes.clear();
        self.missing.clear();
    }
}

#[cfg(test)]
mod tests;
