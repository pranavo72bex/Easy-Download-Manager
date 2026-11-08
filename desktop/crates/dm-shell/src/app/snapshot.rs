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
