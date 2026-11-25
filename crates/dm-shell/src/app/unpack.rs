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
//! what became of it (`dm_core::NOTES`). An archive with a password is opened with the passwords
//! kept in Options › Folders, tried in turn (gaps row C2); one none of them opens is left packed.

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
            Outcome::Kept => dm_core::UNPACKED,
            Outcome::Trashed => dm_core::UNPACKED_AND_TRASHED,
            Outcome::NotTrashed => dm_core::UNPACKED_NOT_TRASHED,
            Outcome::Password => dm_core::NOT_UNPACKED_PASSWORD,
            Outcome::Refused => dm_core::NOT_UNPACKED_REFUSED,
        }
    }
}

/// Whether the finished `row` is unpacked when done: an archive dm unpacks (a ZIP, a 7z, a
/// tar plain or gzipped, a RAR) whose own box is ticked, or whose kind `settings` names.
/// A part of an archive in parts stands for the whole archive (`film.part2.rar` for `film.rar`).
pub fn wanted(row: &Entry, settings: &Settings) -> bool {
    if row.state != State::Completed || row.path.is_empty() {
        return false;
    }
    let name = file_name(row);
    let name = volumes::part_of(&name).map_or(name, |(set, _)| set);
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
    // The sets in parts already started in this read, by folder and archive.
    let mut sets: Vec<(PathBuf, String)> = Vec::new();
    for row in rows
        .iter()
        .filter(|r| finished.contains(&r.id) && wanted(r, settings))
    {
        let (row, parts) = match volumes::part_of(&file_name(row)) {
            None => (row.as_ref(), vec![PathBuf::from(&row.path)]),
            // A part: once every part of its set is a finished row in its folder, the set is
            // unpacked once, from its first part, and its parts go to the Trash together (gaps
            // row C3d). Each was unpacked on its own, and each but the first refused.
            Some((set, _)) => {
                let folder = folder_of(row);
                if sets.contains(&(folder.clone(), set.to_ascii_lowercase())) {
                    continue;
                }
                // A part's name cannot say it is the last: the set waits while any row of the
                // list that is one of its parts is unfinished.
                if pending_part(rows, &set) {
                    continue;
                }
                let parts = parts_of(rows, &folder, &set);
                let names: Vec<String> = parts.iter().map(|r| file_name(r)).collect();
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                if !volumes::all_present(&set, &names) {
                    continue;
                }
                let Some(first) = parts.iter().find(|r| {
                    volumes::part_of(&file_name(r)).is_some_and(|(_, number)| number == 1)
                }) else {
                    continue;
                };
                sets.push((folder, set.to_ascii_lowercase()));
                let paths = parts.iter().map(|r| PathBuf::from(&r.path)).collect();
                (*first, paths)
            }
        };
        if let Err(e) = start(backend, row, &parts, trash) {
            dm_core::note!(warn, "dm: an archive could not be unpacked when done: {e}");
            backend.set_said(row.id, Outcome::Refused.note());
        }
    }
}

/// A row's file's name.
fn file_name(row: &Entry) -> String {
    Path::new(&row.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The folder a row's file is in.
fn folder_of(row: &Entry) -> PathBuf {
    Path::new(&row.path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Whether a row of the list that is a part of `set` is not finished yet: its name its file's,
/// or before it has one the name it is to be saved under, else its address's last part (a row
/// not started yet has no name of its own).
fn pending_part(rows: &[Arc<Entry>], set: &str) -> bool {
    rows.iter().any(|r| {
        let name = if !r.path.is_empty() {
            file_name(r)
        } else if !r.filename.is_empty() {
            r.filename.clone()
        } else if let Some(name) = r.save_name.clone().filter(|n| !n.is_empty()) {
            name
        } else {
            let address = r.url.split(['?', '#']).next().unwrap_or("");
            address.rsplit('/').next().unwrap_or("").to_string()
        };
        r.state != State::Completed
            && volumes::part_of(&name).is_some_and(|(of, _)| of.eq_ignore_ascii_case(set))
    })
}

/// The finished rows in `folder` that are parts of `set`.
fn parts_of<'a>(rows: &'a [Arc<Entry>], folder: &Path, set: &str) -> Vec<&'a Entry> {
    rows.iter()
        .map(Arc::as_ref)
        .filter(|r| r.state == State::Completed && !r.path.is_empty() && folder_of(r) == folder)
        .filter(|r| {
            volumes::part_of(&file_name(r)).is_some_and(|(of, _)| of.eq_ignore_ascii_case(set))
        })
        .collect()
}

/// Unpacks the finished `row`'s archive off this thread, then keeps it or, where `trash`,
/// sends it (`parts`: its file, or every part of an archive in parts) to the Trash
/// (`Platform::trash`, as "Delete file after" does), and says on the row what became of it.
/// Err where the thread could not be started.
fn start(
    backend: &Backend,
    row: &Entry,
    parts: &[PathBuf],
    trash: bool,
) -> Result<(), crate::Error> {
    let archive = PathBuf::from(&row.path);
    let kept = archive.clone();
    let parts = parts.to_vec();
    let (id, backend) = (row.id, backend.clone());
    // The passwords a person keeps for archives, tried in turn on one that has a password, the
    // one that opens it moved to the top (gaps row C2).
    let home = crate::settings::home();
    let passwords = dm_core::extract::passwords::load(&home);
    super::files::extract_later_trying(
        archive,
        row.url.clone(),
        None,
        passwords,
        move |answer, opened_with| {
            if let Some(password) = opened_with {
                dm_core::extract::passwords::used(&home, &password);
            }
            let platform = Arc::clone(&backend.platform);
            // Every part, the first's failure said (the archive's own path is among them).
            let send = |_: &Path| {
                parts
                    .iter()
                    .map(|part| platform.trash(&part.to_string_lossy()))
                    .fold(Ok(()), Result::and)
            };
            let outcome = settle(answer, &kept, trash, &send);
            backend.set_said(id, outcome.note());
        },
    )
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

/// Archives in parts, known by their names (gaps row C3a).
pub(crate) mod volumes;

#[cfg(test)]
mod tests;
