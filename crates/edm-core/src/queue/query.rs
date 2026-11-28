// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The rows and what is running under them, read at once
/// ([`DownloadList::read_shared`]).
#[derive(Debug, Default)]
pub struct SharedRead {
    /// Every row in order, shared with the queue ([`DownloadList::list_shared`]).
    pub rows: Vec<Arc<Entry>>,
    /// The running rows a range is being retried for ([`DownloadList::retrying`]).
    pub retrying: HashMap<Id, (u32, u32, String, std::time::Instant)>,
    /// Each running torrent's swarm ([`DownloadList::swarms`]).
    pub swarms: HashMap<Id, crate::torrent::Swarm>,
}

impl DownloadList {
    /// Each running torrent's swarm at its last sample, by row.
    pub async fn swarms(&self) -> HashMap<Id, crate::torrent::Swarm> {
        self.inner
            .slots
            .lock()
            .await
            .iter()
            .filter_map(|(id, s)| s.swarm.map(|swarm| (*id, swarm)))
            .collect()
    }

    /// The rows a range is being retried for, now: each with the attempt
    /// it is on (the first try is 1) and how many there may be, 0 for no
    /// end, and what failed, in the engine's words. Over once bytes arrive
    /// again.
    pub async fn retrying(&self) -> HashMap<Id, (u32, u32, String, std::time::Instant)> {
        self.inner
            .slots
            .lock()
            .await
            .iter()
            .filter(|(_, s)| s.entry.state == State::Running)
            .filter_map(|(id, s)| s.retrying.clone().map(|r| (*id, r)))
            .collect()
    }

    /// Every row, in the order they were added.
    pub async fn list(&self) -> Vec<Entry> {
        let slots = self.inner.slots.lock().await;
        let order = self.inner.order.lock().await;
        order
            .iter()
            .filter_map(|id| slots.get(id).map(|s| Entry::clone(&s.entry)))
            .collect()
    }

    /// The rows in order, shared rather than copied: a row unchanged since
    /// the last read is the same allocation (`Arc::ptr_eq`), so a reader
    /// can keep what it worked out from it. What the window reads four
    /// times a second while anything moves.
    pub async fn list_shared(&self) -> Vec<Arc<Entry>> {
        let slots = self.inner.slots.lock().await;
        let order = self.inner.order.lock().await;
        order
            .iter()
            .filter_map(|id| slots.get(id).map(|s| s.entry.clone()))
            .collect()
    }

    /// [`Self::list_shared`], [`Self::retrying`] and [`Self::swarms`] read
    /// at once, under one hold of the rows and in one walk of them: what a
    /// window draws from. Three reads each walked all the rows, and between
    /// them a row could change (gaps row 210).
    pub async fn read_shared(&self) -> SharedRead {
        let slots = self.inner.slots.lock().await;
        let order = self.inner.order.lock().await;
        let mut read = SharedRead {
            rows: Vec::with_capacity(order.len()),
            retrying: HashMap::new(),
            swarms: HashMap::new(),
        };
        for id in order.iter() {
            let Some(slot) = slots.get(id) else { continue };
            read.rows.push(slot.entry.clone());
            if let Some(swarm) = slot.swarm {
                read.swarms.insert(*id, swarm);
            }
            if slot.entry.state == State::Running {
                if let Some(retry) = &slot.retrying {
                    read.retrying.insert(*id, retry.clone());
                }
            }
        }
        read
    }

    /// The row with this id, if it is still there.
    pub async fn get(&self, id: Id) -> Option<Entry> {
        self.inner
            .slots
            .lock()
            .await
            .get(&id)
            .map(|s| Entry::clone(&s.entry))
    }
}
