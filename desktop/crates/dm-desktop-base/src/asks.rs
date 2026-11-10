// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's questions say and offer, in the language in force, so every
//! window asks them in the same words: the title, the sentences, a choice
//! to tick, and the buttons (the one that loses nothing first). How a
//! question is drawn, and where, is the window's.

use dm_core::{Entry, State};

use crate::i18n::{t, tf};

/// The question asked before Remove (B17's neighbour: every way of asking,
/// the Delete key, the toolbar, the menus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveAsk {
    /// The title in English, which the window's tests and the control port
    /// find it by.
    pub title_english: &'static str,
    /// The title, in the language in force.
    pub title: &'static str,
    /// What removing does to them.
    pub sentence: &'static str,
    /// Where finished files are beside unfinished rows: what happens to
    /// the files.
    pub files_sentence: Option<&'static str>,
    /// Which one, by name; or how many.
    pub named: String,
    /// The choice to delete the finished files too, where there are any.
    pub toggle: Option<&'static str>,
    /// The button that removes.
    pub go: &'static str,
    /// The button that keeps them: the default, and Enter's.
    pub keep: &'static str,
}

/// What the rows asked about are: `rows` the rows still listed, `files`
/// how many finished files are still on the disk, `deleting_files` whether
/// the question is Delete file's.
pub fn remove(
    rows: &[&Entry],
    files: usize,
    remove_files: bool,
    deleting_files: bool,
    named: Option<String>,
) -> RemoveAsk {
    // A cancelled row has nothing left to lose: its parts went with the
    // cancel.
    let unfinished = rows
        .iter()
        .any(|r| !matches!(r.state, State::Completed | State::Cancelled));
    // Whether there is anything to throw away: a row added for later and
    // never started has fetched nothing.
    let fetched = rows.iter().any(|r| {
        !matches!(r.state, State::Completed | State::Cancelled) && r.progress.downloaded > 0
    });
    let count = rows.len();
    // Not "unfinished" of a set with finished files in it.
    let title_english = if deleting_files {
        "Delete file"
    } else if unfinished && count == 1 {
        "Remove an unfinished download"
    } else if unfinished && files == 0 {
        "Remove unfinished downloads"
    } else {
        "Remove from the list"
    };
    let title = match title_english {
        "Remove an unfinished download" => t("Remove an unfinished download"),
        "Remove unfinished downloads" => t("Remove unfinished downloads"),
        "Delete file" => t("Delete file"),
        _ => t("Remove from the list"),
    };
    // Whole sentences, one for each number, since languages do not build
    // them from the same pieces.
    let mixed = unfinished && files > 0;
    let sentence = match (unfinished, count) {
        (true, _) if mixed => t("Some downloads aren't finished. They lose what was downloaded."),
        (true, 1) if !fetched => t("Nothing was downloaded yet. Remove it from the list?"),
        (true, _) if !fetched => t("Nothing was downloaded yet. Remove them from the list?"),
        (true, 1) => t("This download isn't finished. Removing it discards what was downloaded."),
        (true, _) => {
            t("Some downloads aren't finished. Removing them discards what was downloaded.")
        }
        (false, 1) if remove_files => t("Its file is deleted too."),
        (false, _) if remove_files => t("Their files are deleted too."),
        // No file on the disk (cancelled, or since deleted): none to stay.
        (false, 1) if files == 0 => t("This removes it from the list."),
        (false, _) if files == 0 => t("This removes them from the list."),
        (false, 1) => t("Its file is kept."),
        (false, _) => t("Their files are kept."),
    };
    let files_sentence = mixed.then(|| match (remove_files, files) {
        (true, 1) => t("The finished file is deleted too."),
        (true, _) => t("The finished files are deleted too."),
        (false, 1) => t("The finished file is kept."),
        (false, _) => t("The finished files are kept."),
    });
    // Which one, by name: "this download" was all it said; and how many,
    // where it is more than one.
    let named = match named {
        Some(name) if count == 1 => name,
        _ => tf("Downloads: {}", &[&count]),
    };
    let (go, keep) = match (unfinished, count) {
        (true, 1) => (t("Yes, remove"), t("Keep it")),
        (true, _) => (t("Yes, remove"), t("Keep them")),
        (false, 1) => (t("Remove it"), t("Keep it")),
        (false, _) => (t("Remove them"), t("Keep them")),
    };
    // A finished file can go with its row, if asked for; never by default.
    let toggle = (files > 0).then(|| {
        if files == 1 {
            t("Also delete the file")
        } else {
            t("Also delete the files")
        }
    });
    let go = if toggle.is_some() && remove_files {
        t("Yes, delete")
    } else {
        go
    };
    RemoveAsk {
        title_english,
        title,
        sentence,
        files_sentence,
        named,
        toggle,
        go,
        keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: State, downloaded: u64) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "http://e.com/f",
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": downloaded, "total": null, "speed": 0.0, "eta": null},
            "filename": "f",
        }))
        .unwrap()
    }

    #[test]
    fn an_unfinished_download_is_asked_about_by_name_keep_first() {
        let one = row(State::Paused, 10);
        let ask = remove(&[&one], 0, false, false, Some("film.mp4".into()));
        assert_eq!(ask.title_english, "Remove an unfinished download");
        assert!(ask.sentence.starts_with("This download isn't finished"));
        assert_eq!(ask.named, "film.mp4");
        assert_eq!((ask.go, ask.keep), ("Yes, remove", "Keep it"));
        assert_eq!(ask.toggle, None);
    }

    #[test]
    fn finished_files_offer_to_go_too_and_say_what_happens_to_them() {
        let (a, b) = (row(State::Completed, 5), row(State::Completed, 5));
        let kept = remove(&[&a, &b], 2, false, false, None);
        assert_eq!(kept.title_english, "Remove from the list");
        assert_eq!(kept.sentence, "Their files are kept.");
        assert_eq!(kept.toggle, Some("Also delete the files"));
        assert_eq!(kept.named, "Downloads: 2");
        assert_eq!(kept.go, "Remove them");
        let deleted = remove(&[&a, &b], 2, true, false, None);
        assert_eq!(deleted.sentence, "Their files are deleted too.");
        assert_eq!(deleted.go, "Yes, delete");
        let fresh = row(State::Queued, 0);
        let mixed = remove(&[&a, &fresh], 1, false, false, None);
        assert_eq!(mixed.files_sentence, Some("The finished file is kept."));
        assert_eq!(
            remove(&[&a], 1, false, true, None).title_english,
            "Delete file"
        );
    }
}
