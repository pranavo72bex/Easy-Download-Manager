// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the list forgets of its own accord (gaps row W17): a row that keeps no record, once its
//! file is whole, and the finished downloads older than Options › Downloads' "Forget finished
//! downloads after … days". Only rows go: the files stay where they were saved.

use super::*;

/// Seconds in a day.
const DAY: u64 = 24 * 60 * 60;

impl DownloadList {
    /// Row `id`, where it keeps no record (`Entry::keep_no_record`) and its file is whole: taken
    /// off the list with its address, page and sign-in, its file left where it was saved, and
    /// the next write of the queue made whole, so that no journal line names it and its sign-in
    /// leaves the secrets file. Whether it went; the caller writes the queue.
    pub(super) async fn forget_if_no_record(&self, id: Id) -> bool {
        let taken = {
            let mut slots = self.inner.slots.lock().await;
            let whole = slots
                .get(&id)
                .is_some_and(|s| s.entry.keep_no_record && s.entry.state == State::Completed);
            if !whole {
                return false;
            }
            slots.remove(&id)
        };
        let Some(slot) = taken else {
            return false;
        };
        self.inner.order.lock().await.retain(|x| *x != id);
        self.inner.whole_next.store(true, Ordering::SeqCst);
        stop_sharing(&slot.entry);
        true
    }

    /// The rows that keep no record whose files were whole when dm last stopped, between the
    /// two steps: taken off the list as they would have been then.
    pub(super) async fn forget_whole_no_record(&self) {
        let whole: Vec<Id> = self
            .inner
            .slots
            .lock()
            .await
            .values()
            .filter(|s| s.entry.keep_no_record && s.entry.state == State::Completed)
            .map(|s| s.entry.id)
            .collect();
        for id in whole {
            self.forget_if_no_record(id).await;
        }
    }

    /// "Keep no record" for row `id`, set or cleared (Download details, for a row begun while it
    /// asked): one whose file is already whole goes now. Whether it changed.
    pub async fn set_keep_no_record(&self, id: Id, on: bool) -> bool {
        let changed = match self.inner.slots.lock().await.get_mut(&id) {
            Some(slot) if slot.entry.keep_no_record != on => {
                slot.entry_mut().keep_no_record = on;
                true
            }
            _ => false,
        };
        if changed {
            self.forget_if_no_record(id).await;
            self.save().await;
        }
        changed
    }

    /// Forgets the finished downloads that finished more than `days` days before `now` (seconds
    /// since 1970): Options › Downloads' "Forget finished downloads after … days", where 0 is
    /// never. Their files stay where they were saved. Kept: a row in the Recheck queue, which is
    /// there to be checked again, and one whose "Delete file after" is still to take it with its
    /// file. How many went.
    pub async fn forget_finished_older_than(&self, days: u32, now: u64) -> usize {
        if days == 0 {
            return 0;
        }
        let before = now.saturating_sub(u64::from(days) * DAY);
        let taken: Vec<Slot> = {
            let mut slots = self.inner.slots.lock().await;
            let due: Vec<Id> = slots
                .values()
                .filter(|s| forgettable(&s.entry, before))
                .map(|s| s.entry.id)
                .collect();
            due.iter().filter_map(|id| slots.remove(id)).collect()
        };
        if taken.is_empty() {
            return 0;
        }
        let gone: HashSet<Id> = taken.iter().map(|s| s.entry.id).collect();
        self.inner
            .order
            .lock()
            .await
            .retain(|id| !gone.contains(id));
        for slot in &taken {
            stop_sharing(&slot.entry);
        }
        self.save().await;
        taken.len()
    }
}

/// A finished torrent still shared stops with its row, as one deleted does (gaps row 147).
fn stop_sharing(row: &Entry) {
    if !row.path.is_empty() {
        crate::torrent::stop_sharing(std::path::Path::new(&row.path));
    }
}

/// Whether `row` is a finished download to forget for having finished before `before`
/// (seconds since 1970): not one in the Recheck queue, nor one "Delete file after" will take.
fn forgettable(row: &Entry, before: u64) -> bool {
    row.state == State::Completed
        && row.finished_at.is_some_and(|at| at < before)
        && row.queue != RECHECK_QUEUE
        && row.delete_after.is_none()
}
