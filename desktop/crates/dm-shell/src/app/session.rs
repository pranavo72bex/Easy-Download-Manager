// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The application's work over time, apart from any window: what is due at a
//! tick (files whose "Delete file after" has come, and, as they move here, the
//! other timers), done by the application and reported, so each presentation
//! only reacts (forgets a row's dialogs, says what could not be done).

use std::collections::HashMap;
use std::sync::Arc;

use dm_core::{Entry, Id, State};

use super::backend::Backend;
use super::detect::{Detected, Detector, DryWatch};
use super::snapshot::Snapshot;
use super::watch::Watched;
use crate::Settings;

/// The application's memory between ticks, for whichever presentation drives
/// it (the window, or a loop with no window): what was told, and the watched
/// folder's. It does not own the `Backend`, which the presentation holds; its
/// methods are handed it.
#[derive(Debug, Default)]
pub struct Session {
    detector: Detector,
    dry: DryWatch,
    watched: Watched,
}

/// One read of the queue: what to draw from, what happened since the last
/// read, and problems met doing what follows from it.
#[derive(Clone, Default)]
pub struct Read {
    /// The rows and what each line says beside its state.
    pub snapshot: Snapshot,
    /// What happened since the last read.
    pub detected: Detected,
    /// Problems met (a command run after a download that could not start).
    pub problems: Vec<String>,
}

impl Session {
    /// Reads the queue and does what follows from it that is the
    /// application's: the command a person set to run on each finished file
    /// (Options › Downloads). Call `changes::take` before, never after, so a
    /// change made during the read is still owed one.
    pub fn read(&mut self, backend: &Backend, settings: &Settings) -> Read {
        let snapshot = backend.snapshot();
        let detected = self.detector.read(&snapshot.rows);
        let mut problems = Vec::new();
        let command = settings.on_complete.trim();
        if !command.is_empty() && !detected.finished.is_empty() {
            for row in snapshot
                .rows
                .iter()
                .filter(|r| detected.finished.contains(&r.id) && !r.path.is_empty())
            {
                if let Err(why) = crate::shell::run_after(command, std::path::Path::new(&row.path))
                {
                    problems.push(why);
                }
            }
        }
        Read {
            snapshot,
            detected,
            problems,
        }
    }

    /// A row the presentation added itself: not "started elsewhere".
    pub fn own(&mut self, id: Id) {
        self.detector.own(id);
    }

    /// Each row's state at the read before the latest (When done's dry-queue
    /// watch compares the latest rows with it).
    pub fn before(&self) -> &HashMap<Id, State> {
        self.detector.before()
    }

    /// Whether the queue has just run dry since the read before (When done:
    /// the sound and the action it arms are the presentation's to start).
    /// Call once per read, after it.
    pub fn ran_dry(&mut self, rows: &[Arc<Entry>]) -> bool {
        self.dry.look(rows, self.detector.before())
    }

    /// The watched folder's look (`watch::Watched::look`).
    pub fn look_at_watched(
        &mut self,
        folder: &str,
        rows: &[Arc<Entry>],
        backend: &Backend,
    ) -> Vec<Vec<String>> {
        self.watched.look(folder, rows, backend)
    }
}

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
