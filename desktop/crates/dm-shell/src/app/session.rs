// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The application's work over time, apart from any window: what is due at a
//! tick (files whose "Delete file after" has come, and, as they move here, the
//! other timers), done by the application and reported, so each presentation
//! only reacts (forgets a row's dialogs, says what could not be done).

use std::sync::Arc;

use dm_core::{Entry, Id};

use super::backend::Backend;

/// What a tick of "Delete file after" did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeletedDue {
    /// Rows removed: their file moved to the Trash, or already gone.
    pub gone: Vec<Id>,
    /// Files that could not be moved, each with why; their rows stay, and are
    /// not tried again.
    pub failed: Vec<String>,
}

/// "Delete file after": each finished row whose time has come at `now`
/// (seconds since the epoch) has its file moved to the Trash and is removed.
/// Only its own file: one moved away, or since written by another row, is not
/// this row's to delete. A finished torrent still shared lets go of its file
/// first. `None` when nothing was due.
pub fn delete_due(backend: &Backend, rows: &[Arc<Entry>], now: u64) -> Option<DeletedDue> {
    let due: Vec<&Arc<Entry>> = rows
        .iter()
        .filter(|r| r.delete_due_in(now) == Some(0))
        .collect();
    if due.is_empty() {
        return None;
    }
    let superseded = super::rules::superseded(rows);
    let mut done = DeletedDue::default();
    for row in due {
        let present = !row.path.is_empty() && !super::rules::file_gone(row, &superseded);
        match present.then(|| {
            dm_core::stop_sharing(std::path::Path::new(&row.path));
            backend.platform.trash(&row.path)
        }) {
            Some(Err(e)) => {
                done.failed.push(format!("{}: {e}", row.path));
                backend.set_delete_after(row.id, None);
            }
            _ => done.gone.push(row.id),
        }
    }
    if !done.gone.is_empty() {
        backend.remove_many(&done.gone);
    }
    Some(done)
}

/// What carrying out a When done action that came due leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carried {
    /// dm closes: the presentation does it, as only it can close itself.
    Quit,
    /// The computer was asked (to sleep, hibernate or shut down).
    Asked,
}

/// Carries out `action` once its countdown is over. Quit is the
/// presentation's to do; anything else is asked of the computer through the
/// platform, and a refusal that comes back later goes on `backend`'s problems,
/// as anything running apart from the window does: whoever set it is likely
/// away. An `Err` is a refusal said at once.
pub fn carry_out(backend: &Backend, action: super::power::WhenDone) -> Result<Carried, String> {
    if action == super::power::WhenDone::Quit {
        return Ok(Carried::Quit);
    }
    let notices = backend.notices();
    let report: super::power::Report = Box::new(move |why| dm_core::lock(&notices).push(why));
    backend
        .platform
        .power(action, report)
        .map(|()| Carried::Asked)
}
