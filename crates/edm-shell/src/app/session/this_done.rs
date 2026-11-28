// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What EDM does when one download is done, chosen in its own progress window.

use std::sync::Arc;

use edm_core::{Entry, State};

use super::Session;
use crate::app::power::WhenDone;

/// What EDM does when one download is done, chosen in its own progress window (gaps row U36):
/// nothing, open the file or its folder, or what Downloads › When done does for every download.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThisDone {
    /// Nothing.
    #[default]
    Nothing,
    /// The file opened.
    Open,
    /// Its folder opened, the file chosen there.
    OpenFolder,
    /// Exit, sleep, hibernate or turn off, through When done's own countdown.
    Then(WhenDone),
}

impl ThisDone {
    /// Every choice, in the order the progress window lists them.
    pub const ALL: [ThisDone; 7] = [
        ThisDone::Nothing,
        ThisDone::Open,
        ThisDone::OpenFolder,
        ThisDone::Then(WhenDone::Quit),
        ThisDone::Then(WhenDone::Sleep),
        ThisDone::Then(WhenDone::Hibernate),
        ThisDone::Then(WhenDone::ShutDown),
    ];

    /// Its words in English, as the menus name the same things.
    pub fn english(self) -> &'static str {
        match self {
            ThisDone::Nothing | ThisDone::Then(WhenDone::Nothing) => "Do nothing",
            ThisDone::Open => "Open",
            ThisDone::OpenFolder => "Open folder",
            ThisDone::Then(WhenDone::Quit) => "Exit EDM",
            ThisDone::Then(WhenDone::Sleep) => "Sleep",
            ThisDone::Then(WhenDone::Hibernate) => "Hibernate",
            ThisDone::Then(WhenDone::ShutDown) => "Turn off computer",
        }
    }
}

impl Session {
    /// The downloads whose own "When this download is done" is due: each finished one, with its
    /// choice, taken once. A choice for a download that failed, was cancelled or left the list
    /// is forgotten.
    pub fn this_done_due(&mut self, rows: &[Arc<Entry>]) -> Vec<(Entry, ThisDone)> {
        let mut due = Vec::new();
        self.this_done
            .retain(|id, choice| match rows.iter().find(|r| r.id == *id) {
                Some(row) if row.state == State::Completed => {
                    due.push(((**row).clone(), *choice));
                    false
                }
                Some(row) => !matches!(row.state, State::Failed(_) | State::Cancelled),
                None => false,
            });
        due
    }
}

#[cfg(test)]
mod this_done_tests {
    use super::*;
    use edm_core::Id;

    fn row(id: Id, state: State) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": id, "url": "https://e.com/f.bin",
                "state": serde_json::to_value(&state).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "f.bin",
            }))
            .unwrap(),
        )
    }

    /// A download's own choice is due once, when it finishes; a failed or gone one's is
    /// forgotten, a running one's kept (gaps row U36).
    #[test]
    fn a_downloads_own_choice_is_due_once_when_it_finishes() {
        let mut session = Session::default();
        session.this_done.insert(1, ThisDone::Open);
        session.this_done.insert(2, ThisDone::Then(WhenDone::Sleep));
        session.this_done.insert(3, ThisDone::OpenFolder);
        session.this_done.insert(4, ThisDone::Open);
        let rows = vec![
            row(1, State::Running),
            row(2, State::Failed("x".into())),
            row(3, State::Completed),
        ];
        let due = session.this_done_due(&rows);
        assert_eq!(due.len(), 1);
        assert_eq!((due[0].0.id, due[0].1), (3, ThisDone::OpenFolder));
        assert_eq!(
            session.this_done.keys().copied().collect::<Vec<_>>(),
            vec![1]
        );
        let rows = vec![row(1, State::Completed)];
        assert_eq!(session.this_done_due(&rows).len(), 1);
        assert!(session.this_done_due(&rows).is_empty(), "once");
    }
}
