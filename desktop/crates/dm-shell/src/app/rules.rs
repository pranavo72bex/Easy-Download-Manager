// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Decisions the application makes about a row, once, for every place that
//! acts on them: the menu, the progress window, Add URL, the control port and
//! any other presentation. Each place keeps its own way of saying what it
//! decided (a question, a sentence, an error), never its own copy of the rule.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_row_resumes_a_failed_one_retries_and_a_held_one_is_asked_about() {
        assert_eq!(go_on(&State::Paused, false), GoOn::Resume);
        assert_eq!(go_on(&State::Paused, true), GoOn::Held);
        assert_eq!(go_on(&State::Failed("x".into()), false), GoOn::Retry);
        // Held says nothing of a row that is not stopped.
        assert_eq!(go_on(&State::Failed("x".into()), true), GoOn::Retry);
        assert_eq!(go_on(&State::Completed, false), GoOn::Settled);
        assert_eq!(go_on(&State::Cancelled, true), GoOn::Settled);
        assert_eq!(go_on(&State::Queued, false), GoOn::NotStopped);
        assert_eq!(go_on(&State::Running, false), GoOn::NotStopped);
    }

    /// `url` with a login in it, set through its parts rather than written
    /// out: a login spelled in the source reads to a secret scanner as a leak.
    fn with_login(url: &str, user: &str, word: &str) -> String {
        let mut parsed = url::Url::parse(url).unwrap();
        parsed.set_username(user).unwrap();
        parsed.set_password(Some(word)).unwrap();
        parsed.to_string()
    }

    /// A row as the queue file keeps one: the fields it needs, the rest
    /// their defaults.
    fn row(id: Id, url: &str, state: State, path: &str) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": id,
                "url": url,
                "state": serde_json::to_value(&state).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "f",
                "path": path,
            }))
            .unwrap(),
        )
    }

    /// The same address with or without its login is one row; a cancelled
    /// row is no longer in the list.
    #[test]
    fn an_address_is_listed_by_its_bare_form_and_not_once_cancelled() {
        let rows = vec![
            row(1, "https://a.example/f.zip", State::Cancelled, ""),
            row(
                2,
                &with_login("https://b.example/g.zip", "user", "pw"),
                State::Paused,
                "",
            ),
        ];
        assert_eq!(already_listed(&rows, "https://a.example/f.zip"), None);
        assert_eq!(already_listed(&rows, "https://b.example/g.zip"), Some(2));
    }

    /// A failed row, or a finished one whose file is gone, is not worth
    /// keeping; a finished one whose file is there, or a stopped one, is.
    #[test]
    fn only_a_row_still_worth_having_is_a_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let kept = dir.path().join("kept.bin");
        std::fs::write(&kept, b"x").unwrap();
        let gone = dir.path().join("gone.bin").display().to_string();
        let rows = vec![
            row(1, "https://h.example/failed", State::Failed("x".into()), ""),
            row(2, "https://h.example/gone", State::Completed, &gone),
            row(
                3,
                "https://h.example/kept",
                State::Completed,
                &kept.display().to_string(),
            ),
            row(4, "https://h.example/stopped", State::Paused, ""),
        ];
        assert_eq!(worth_keeping(&rows, "https://h.example/failed"), None);
        assert_eq!(worth_keeping(&rows, "https://h.example/gone"), None);
        assert_eq!(worth_keeping(&rows, "https://h.example/kept"), Some(3));
        assert_eq!(worth_keeping(&rows, "https://h.example/stopped"), Some(4));
    }

    #[test]
    fn a_kept_choice_is_read_back_from_its_word() {
        for dup in [Dup::Show, Dup::Again, Dup::Replace] {
            assert_eq!(Dup::from_word(dup.word()), Some(dup));
        }
        assert_eq!(Dup::from_word("ask"), None);
    }
}
