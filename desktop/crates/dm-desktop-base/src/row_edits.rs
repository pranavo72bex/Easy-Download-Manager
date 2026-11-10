// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the row dialogs change, whichever window shows them: Update
//! download link's fresh address, Move to queue's queue, Move/Rename's
//! folder and names, and Properties' note, address, sign-in and a
//! torrent's choice of files. The fields and their layout are each
//! window's; what OK does is here, so the two windows cannot differ.

use dm_core::{Entry, Id, State, TorrentPriority};
use dm_shell::app::backend::Backend;

use crate::i18n::{t, Said};

/// Whether a row's referrer is a web page. It can come from an imported
/// list, so only an address the browser would open is handed to the
/// system's opener: a file path there would open a local or network file
/// instead.
pub fn is_web_page(referrer: &str) -> bool {
    let lower = referrer.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// An address split into itself and the user and password in it.
pub fn split_login(url: &str) -> (String, String, String) {
    match url::Url::parse(url) {
        Ok(mut parsed) if !parsed.username().is_empty() => {
            let decode = |s: &str| {
                url::form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes())
                    .next()
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default()
            };
            let login = decode(parsed.username());
            let password = decode(parsed.password().unwrap_or(""));
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            (parsed.to_string(), login, password)
        }
        _ => (url.to_string(), String::new(), String::new()),
    }
}

/// The address with a user and password put back in it.
pub fn join_login(address: &str, login: &str, password: &str) -> String {
    match url::Url::parse(address.trim()) {
        Ok(mut parsed) if !login.is_empty() => {
            let _ = parsed.set_username(login);
            let _ = parsed.set_password(Some(password).filter(|p| !p.is_empty()));
            parsed.to_string()
        }
        _ => address.trim().to_string(),
    }
}

/// Update download link's OK: `address` becomes row `id`'s link, and a
/// download that failed for want of it goes on, from the parts it has.
/// The refusal's words where the address cannot be taken.
pub fn update_link(backend: &Backend, id: Id, address: &str) -> Result<(), String> {
    let failed = backend
        .row(id)
        .is_some_and(|r| matches!(r.state, State::Failed(_)));
    backend.set_url(id, address)?;
    if failed {
        backend.retry(id);
    }
    Ok(())
}

/// Move to queue's choice, in dm's own name: a new name where one is
/// typed, else the queue picked. The pick is held as shown, so typing
/// "Principal" in Spanish is the Main queue, not a new one beside it.
/// `None` where neither is given.
pub fn queue_chosen(picked: &str, new: &str) -> Option<String> {
    let chosen = if new.trim().is_empty() {
        picked.trim()
    } else {
        new.trim()
    };
    (!chosen.is_empty()).then(|| crate::i18n::queue_named(chosen))
}

/// Move/Rename: the finished downloads it moves, the new name where it is
/// one, and the folder they go to.
#[derive(Debug, Clone, Default)]
pub struct MoveRename {
    /// The finished downloads to move.
    pub ids: Vec<Id>,
    /// The new name, where one download is moved.
    pub name: String,
    /// The folder they go to.
    pub folder: String,
    /// For several files: how to name them. Empty keeps their names;
    /// `{n}` counts from 1 in the order they are listed, `{name}` is the
    /// old name without its ending, and the ending is kept.
    pub pattern: String,
}

impl MoveRename {
    /// As the dialog opens for `rows`: the first one's name and folder.
    pub fn of(rows: &[Entry]) -> MoveRename {
        MoveRename {
            ids: rows.iter().map(|r| r.id).collect(),
            name: rows.first().map(|r| r.filename.clone()).unwrap_or_default(),
            folder: rows
                .first()
                .and_then(|r| std::path::Path::new(&r.path).parent())
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            pattern: String::new(),
        }
    }

    /// OK: each moved first where the folder changed, then renamed in it;
    /// what to say where something was refused, or where the name asked
    /// for was taken and a number given, as a download's is.
    pub fn carry(&self, backend: &Backend) -> Option<Said> {
        let folder = std::path::PathBuf::from(self.folder.trim());
        let mut said: Option<Said> = None;
        for (nth, id) in self.ids.iter().enumerate() {
            let Some(row) = backend.row(*id) else {
                continue;
            };
            let here = std::path::Path::new(&row.path)
                .parent()
                .map(|p| p.to_path_buf());
            if !self.folder.trim().is_empty() && here.as_deref() != Some(folder.as_path()) {
                if let Err(e) = backend.move_to(*id, &folder) {
                    said = Some(e.into());
                    continue;
                }
            }
            if self.ids.len() > 1 && !self.pattern.trim().is_empty() {
                let asked = crate::format::pattern_name(&self.pattern, nth + 1, &row.filename);
                if let Err(e) = backend.rename(*id, &asked) {
                    said = Some(e.into());
                }
            }
            if self.ids.len() == 1 {
                let asked = self.name.trim().to_string();
                match backend.rename(*id, &asked) {
                    // Said, rather than a different name appearing without
                    // a word.
                    Ok(()) => {
                        if let Some(given) = backend.row(*id).map(|r| r.filename) {
                            if given != asked {
                                said = Some(Said::new(
                                    "{} is taken in that folder, so it is called {}.",
                                    &[&asked, &given],
                                ));
                            }
                        }
                    }
                    Err(e) => said = Some(e.into()),
                }
            }
        }
        said
    }
}

/// What Properties keeps on OK, as edited.
pub struct Edited<'a> {
    /// The note.
    pub description: &'a str,
    /// The address, with its sign-in put back in it (`join_login`).
    pub address: &'a str,
    /// A torrent's files ticked to be fetched; empty where it has none.
    pub files: &'a [bool],
    /// When each of a torrent's files is fetched; empty where it has none.
    pub priorities: &'a [TorrentPriority],
}

/// Properties' OK for `row`: a torrent's choice of files and their
/// priorities, the note and the address kept. A sign-in given to a
/// download that failed for want of one is what it was waiting for: it is
/// tried again with it, and true is answered. The words to say in the
/// dialog where something cannot be kept; nothing after it is kept then.
pub fn keep_properties(backend: &Backend, row: &Entry, edited: &Edited) -> Result<bool, String> {
    choose_files(backend, row, edited.files)?;
    choose_priorities(backend, row, edited.priorities)?;
    if edited.description != row.description {
        backend.set_description(row.id, edited.description);
    }
    let changed = edited.address.trim() != row.url;
    if changed {
        backend.set_url(row.id, edited.address)?;
    }
    let wants_login = matches!(&row.state, State::Failed(why) if crate::rows::needs_sign_in(why));
    if changed && wants_login {
        backend.retry(row.id);
        return Ok(true);
    }
    Ok(false)
}

/// Keeps a changed choice of a torrent's files, or says why it cannot be.
fn choose_files(backend: &Backend, row: &Entry, files: &[bool]) -> Result<(), String> {
    let now: Vec<bool> = row.torrent_files.iter().map(|f| f.wanted).collect();
    if files.is_empty() || files == now.as_slice() {
        return Ok(());
    }
    if !files.iter().any(|w| *w) {
        return Err(t("Tick at least one file.").to_string());
    }
    backend
        .set_torrent_files(row.id, files)
        .map_err(|e| files_refusal(&e))
}

/// Keeps changed priorities of a torrent's files, or says why they cannot
/// be.
fn choose_priorities(
    backend: &Backend,
    row: &Entry,
    priorities: &[TorrentPriority],
) -> Result<(), String> {
    let now: Vec<_> = row.torrent_files.iter().map(|f| f.priority).collect();
    if priorities.is_empty() || priorities == now.as_slice() {
        return Ok(());
    }
    backend
        .set_torrent_priorities(row.id, priorities)
        .map_err(|e| files_refusal(&e))
}

/// The queue's refusal of a choice of files, in the language in force.
pub fn files_refusal(why: &str) -> String {
    match why {
        "Tick at least one file." => t("Tick at least one file."),
        "Stop the download to change which files are fetched." => {
            t("Stop the download to change which files are fetched.")
        }
        "The torrent's files are not known yet." => t("The torrent's files are not known yet."),
        other => other,
    }
    .to_string()
}

/// A torrent file's priority in the window's words.
pub fn priority_words(p: TorrentPriority) -> &'static str {
    match p {
        TorrentPriority::High => t("High"),
        TorrentPriority::Normal => t("Normal"),
        TorrentPriority::Low => t("Low"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_web_address_is_opened_as_the_page_a_row_came_from() {
        assert!(is_web_page("https://site.example/page"));
        assert!(is_web_page("HTTP://site.example/"));
        assert!(!is_web_page(""));
        assert!(!is_web_page("C:\\Windows\\System32\\calc.exe"));
        assert!(!is_web_page("\\\\host\\share\\x.exe"));
        assert!(!is_web_page("file:///etc/passwd"));
    }

    #[test]
    fn the_queues_refusals_are_said_and_others_kept_as_they_are() {
        for why in [
            "Tick at least one file.",
            "Stop the download to change which files are fetched.",
            "The torrent's files are not known yet.",
            "the disk is full",
        ] {
            // In English, each is its own words.
            assert_eq!(files_refusal(why), why);
        }
    }

    #[test]
    fn a_sign_in_is_taken_out_of_the_address_and_put_back() {
        let (address, login, password) = split_login("https://ann:p%40ss@host.example/f.zip");
        assert_eq!(address, "https://host.example/f.zip");
        assert_eq!(login, "ann");
        assert_eq!(password, "p@ss");
        assert_eq!(
            join_login(&address, &login, &password),
            "https://ann:p%40ss@host.example/f.zip"
        );
        // No login: the address as typed, trimmed.
        assert_eq!(
            join_login(" https://host.example/f.zip ", "", "x"),
            "https://host.example/f.zip"
        );
        let (plain, none, nothing) = split_login("https://host.example/f.zip");
        assert_eq!(plain, "https://host.example/f.zip");
        assert!(none.is_empty() && nothing.is_empty());
    }

    #[test]
    fn a_new_queue_name_wins_over_the_pick_and_nothing_is_nothing() {
        assert_eq!(queue_chosen("Night", "").as_deref(), Some("Night"));
        assert_eq!(queue_chosen("Night", " Day ").as_deref(), Some("Day"));
        assert_eq!(queue_chosen(" ", " "), None);
    }
}
