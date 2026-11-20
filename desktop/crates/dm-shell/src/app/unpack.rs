// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Unpack when done" (gaps row W14): a finished archive whose own box is ticked (Download
//! details) or whose kind is (Options › Folders) is unpacked by dm's own extraction
//! (`files::extract_later_with`, dm-core's `extract`) into a new folder of its name beside it,
//! off the presentation's thread. The extraction's rules hold: no file on disk is replaced (the
//! folder is `name`, else `name (1)`…), nothing is written outside that folder, nothing in it is
//! opened or run, and an archive any of whose entries breaks a rule is not unpacked at all.
//! Then the archive is kept, or sent to the Trash, as Options › Folders says, and its row says
//! what became of it (`dm_core::NOTES`). An archive with a password is left packed: no window
//! asks for one yet (gaps row F10).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dm_core::{Entry, Id, State};

use super::backend::Backend;
use crate::Settings;

/// What became of an archive unpacked when done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Unpacked; the archive kept.
    Kept,
    /// Unpacked; the archive sent to the Trash.
    Trashed,
    /// Unpacked; the archive could not be sent to the Trash and is where it was.
    NotTrashed,
    /// Not unpacked: the archive has a password.
    Password,
    /// Not unpacked for another reason (an entry that would leave its folder, a link, a damaged
    /// archive, the disk refusing): Extract here on the row's menu says which.
    Refused,
}

impl Outcome {
    /// What the row says of it, in English: one of `dm_core::NOTES`.
    pub fn note(self) -> &'static str {
        match self {
            Outcome::Kept => dm_core::NOTES[4],
            Outcome::Trashed => dm_core::NOTES[5],
            Outcome::NotTrashed => dm_core::NOTES[6],
            Outcome::Password => dm_core::NOTES[7],
            Outcome::Refused => dm_core::NOTES[8],
        }
    }
}

/// Whether the finished `row` is unpacked when done: an archive dm unpacks (a ZIP, a 7z, a
/// tar plain or gzipped, a RAR) whose own box is ticked, or whose kind `settings` names.
pub fn wanted(row: &Entry, settings: &Settings) -> bool {
    if row.state != State::Completed || row.path.is_empty() {
        return false;
    }
    let name = Path::new(&row.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !dm_core::extract::is_archive(&name) {
        return false;
    }
    let kind = row
        .category
        .unwrap_or_else(|| dm_core::classify(&name, None));
    row.unpack || settings.folders.unpack_kinds.contains(&kind)
}

/// Unpacks each row of `rows` that `finished` names (those that finished since the last read)
/// and [`wanted`] says is, each on a thread of its own, its outcome said on its row once it is
/// over. `Session::read` calls it after each read, for every presentation.
pub fn when_done(backend: &Backend, settings: &Settings, rows: &[Arc<Entry>], finished: &[Id]) {
    if finished.is_empty() {
        return;
    }
    let trash = settings.folders.unpack_trash;
    for row in rows
        .iter()
        .filter(|r| finished.contains(&r.id) && wanted(r, settings))
    {
        if let Err(e) = start(backend, row, trash) {
            dm_core::note!(warn, "dm: an archive could not be unpacked when done: {e}");
            backend.set_said(row.id, Outcome::Refused.note());
        }
    }
}

/// Unpacks the finished `row`'s archive off this thread, then keeps it or, where `trash`,
/// sends it to the Trash (`Platform::trash`, as "Delete file after" does), and says on the row
/// what became of it. Err where the thread could not be started.
fn start(backend: &Backend, row: &Entry, trash: bool) -> Result<(), crate::Error> {
    let archive = PathBuf::from(&row.path);
    let kept = archive.clone();
    let (id, backend) = (row.id, backend.clone());
    super::files::extract_later_with(archive, row.url.clone(), None, None, move |answer| {
        let platform = Arc::clone(&backend.platform);
        let send = |path: &Path| platform.trash(&path.to_string_lossy());
        let outcome = settle(answer, &kept, trash, &send);
        backend.set_said(id, outcome.note());
    })
}

/// What an extraction's `answer` comes to for `archive`: kept, or sent away by `send` where
/// `trash` (only once it is unpacked: an archive not unpacked is always kept). Why one was not
/// unpacked, or not sent away, goes to dm's log.
pub fn settle(
    answer: Result<dm_core::extract::Extracted, dm_core::extract::Error>,
    archive: &Path,
    trash: bool,
    send: &dyn Fn(&Path) -> Result<(), String>,
) -> Outcome {
    use dm_core::extract::Error;
    match answer {
        Ok(_) if !trash => Outcome::Kept,
        Ok(_) => match send(archive) {
            Ok(()) => Outcome::Trashed,
            Err(e) => {
                dm_core::note!(
                    warn,
                    "dm: an unpacked archive could not go to the Trash: {e}"
                );
                Outcome::NotTrashed
            }
        },
        Err(Error::Encrypted(_) | Error::WrongPassword(_)) => Outcome::Password,
        Err(e) => {
            dm_core::note!(warn, "dm: an archive was not unpacked when done: {e}");
            Outcome::Refused
        }
    }
}

#[cfg(test)]
mod tests;
