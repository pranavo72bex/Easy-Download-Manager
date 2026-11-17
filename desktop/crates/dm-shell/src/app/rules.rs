// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Decisions the application makes about a row, once, for every place that
//! acts on them: the menu, the progress window, Add URL, the control port and
//! any other presentation. Each place keeps its own way of saying what it
//! decided (a question, a sentence, an error), never its own copy of the rule.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dm_core::{Entry, Id, State};

/// What "go on" means for a row as it stands: Resume on a stopped one, Retry
/// on a failed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoOn {
    /// Stopped by a person: resumed where it stopped.
    Resume,
    /// Failed: tried again.
    Retry,
    /// Stopped by the schedule or the download limit: resuming would only put
    /// it back to be held again, so the person is asked (or told) instead.
    Held,
    /// Finished or cancelled: there is nothing to go on with.
    Settled,
    /// Running or waiting its turn already.
    NotStopped,
}

/// What going on with a row in `state` is; `held` is whether the schedule or
/// the download limit stopped it (`Entry::paused_by_schedule`).
pub fn go_on(state: &State, held: bool) -> GoOn {
    match state {
        State::Paused if held => GoOn::Held,
        State::Paused => GoOn::Resume,
        State::Failed(_) => GoOn::Retry,
        State::Completed | State::Cancelled => GoOn::Settled,
        _ => GoOn::NotStopped,
    }
}

/// Whether `row` failed as a link that has run out does: refused (403,
/// 404 or 410) by its server after part of it had arrived, where the same
/// address worked a moment before.
pub fn link_expired(row: &Entry) -> bool {
    let State::Failed(why) = &row.state else {
        return false;
    };
    row.progress.downloaded > 0
        && ["answered 403", "answered 404", "answered 410"]
            .iter()
            .any(|code| why.contains(code))
}

/// Which finished rows another, later one has taken the file of: a row whose
/// file was deleted turned "Complete" again when the same address was
/// downloaded anew under its old name. The later row is the one finished last
/// (in the same second, the higher id).
pub fn superseded<R: std::borrow::Borrow<Entry>>(rows: &[R]) -> HashSet<Id> {
    let rows: Vec<&Entry> = rows.iter().map(|r| r.borrow()).collect();
    let mut latest: HashMap<&str, (u64, Id)> = HashMap::new();
    let finished = rows
        .iter()
        .filter(|r| r.state == State::Completed && !r.path.is_empty());
    for row in finished.clone() {
        let at = row.finished_at.unwrap_or(0);
        let held = latest.entry(row.path.as_str()).or_insert((at, row.id));
        if (at, row.id) > *held {
            *held = (at, row.id);
        }
    }
    finished
        .filter(|r| {
            latest
                .get(r.path.as_str())
                .is_some_and(|(_, id)| *id != r.id)
        })
        .map(|r| r.id)
        .collect()
}

/// A finished row whose file is no longer where dm saved it: gone from the
/// disk, or taken by a later row (`superseded`).
pub fn file_gone(entry: &Entry, superseded: &HashSet<Id>) -> bool {
    !entry.path.is_empty()
        && (!std::path::Path::new(&entry.path).exists() || superseded.contains(&entry.id))
}

/// The row an address is already listed as: the same address, its login
/// aside (`lists::bare`), in a row not cancelled.
pub fn already_listed(rows: &[Arc<Entry>], address: &str) -> Option<Id> {
    let wanted = crate::lists::bare(address);
    rows.iter()
        .find(|r| r.state != State::Cancelled && crate::lists::bare(&r.url) == wanted)
        .map(|r| r.id)
}

/// The row a caught download of `address` would duplicate: one listed and
/// still worth keeping. A failed row, or a finished one whose file has gone
/// from the disk, is not: the person clicking the link again wants it.
pub fn worth_keeping(rows: &[Arc<Entry>], address: &str) -> Option<Id> {
    let wanted = crate::lists::bare(address);
    rows.iter()
        .find(|r| {
            r.state != State::Cancelled
                && !matches!(r.state, State::Failed(_))
                && crate::lists::bare(&r.url) == wanted
                && (r.state != State::Completed
                    || (!r.path.is_empty() && std::path::Path::new(&r.path).exists()))
        })
        .map(|r| r.id)
}

/// How an address already in the list is answered: show its row (and go on
/// with it if it is stopped), download it again as a numbered copy, or have
/// the new download take the old file's place once it is whole. Kept as a
/// word in the settings (`duplicate_choice`) when the person ticks
/// "Remember my choice"; Options › General undoes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dup {
    /// Show the listed row, going on with it if it is stopped.
    Show,
    /// Download it again, as a numbered copy.
    Again,
    /// Download it again to take the old file's place once whole.
    Replace,
}

impl Dup {
    /// The word the settings keep it as.
    pub fn word(self) -> &'static str {
        match self {
            Dup::Show => "show",
            Dup::Again => "again",
            Dup::Replace => "replace",
        }
    }

    /// The choice a kept word names, if it names one.
    pub fn from_word(word: &str) -> Option<Dup> {
        [Dup::Show, Dup::Again, Dup::Replace]
            .into_iter()
            .find(|d| d.word() == word.trim())
    }
}

/// The options that download a row's address again over its finished file
/// (Add URL's "Replace the old file"): the same folder and name, the old
/// file replaced once the new one is whole. `None` when the row is not
/// finished or its file is gone (a numbered copy is the way then).
pub fn replacing(row: &Entry) -> Option<dm_core::AddOptions> {
    if row.state != State::Completed || row.path.is_empty() {
        return None;
    }
    let path = std::path::Path::new(&row.path);
    if !path.exists() {
        return None;
    }
    Some(dm_core::AddOptions {
        replaces: Some(row.path.clone()),
        save_dir: Some(path.parent()?.to_string_lossy().into_owned()),
        save_name: Some(path.file_name()?.to_string_lossy().into_owned()),
        ..Default::default()
    })
}

/// A running row that has every byte of a file of known length: it is
/// joining its parts, not fetching.
pub fn fetched_all(row: &Entry) -> bool {
    row.progress
        .total
        .is_some_and(|total| total > 0 && row.progress.downloaded >= total)
}

/// Whether a row was added for later and waits for its queue: stopped, in
/// a queue, nothing fetched and not held by the schedule.
pub fn waits_for_its_queue(row: &Entry) -> bool {
    row.state == State::Paused
        && !row.paused_by_schedule
        && !row.queue.is_empty()
        && row.progress.downloaded == 0
}

/// What the status bar counts.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tally {
    /// Rows running and still fetching.
    pub running: usize,
    /// Rows running with every byte, joining their parts.
    pub finishing: usize,
    /// The fetching rows' speeds, together, in bytes a second.
    pub rate: f64,
    /// Rows added for later, waiting for Start queue.
    pub later: usize,
}

/// The status bar's counts over `rows`. A row with every byte is only
/// putting its file together: counted as finishing, not as downloading at
/// 0 B/s.
pub fn tally(rows: &[Arc<Entry>]) -> Tally {
    let mut tally = Tally::default();
    for row in rows {
        if row.state == State::Running {
            if fetched_all(row) {
                tally.finishing += 1;
            } else {
                tally.running += 1;
                tally.rate += row.progress.speed;
            }
        } else if waits_for_its_queue(row) {
            tally.later += 1;
        }
    }
    tally
}

#[cfg(test)]
mod tests;
