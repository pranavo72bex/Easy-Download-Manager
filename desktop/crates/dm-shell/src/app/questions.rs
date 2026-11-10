// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The questions the rows raise, as reference A asks them (maintainer,
//! 2026-11-01; contract rows B39 to B41): a name the server sent that is not
//! the address's, a web page where a file was asked for, a 404. Found by
//! comparing each read with the last, and kept until the presentation is free
//! to ask, one at a time. How each is asked is the presentation's.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use dm_core::{Entry, Id, State};

use super::names::provisional;

/// A question a row raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// The server sent another name than the address's: which to save as.
    Name {
        /// The row.
        id: Id,
        /// The name the address gave.
        address_name: String,
        /// The name the server sent.
        server_name: String,
    },
    /// A web page answered for a file: save the page, or not.
    Page(Id),
    /// A server said the file is not there.
    NotFound(Id),
}

impl Question {
    /// The row it is about.
    pub fn id(&self) -> Id {
        match self {
            Question::Name { id, .. } | Question::Page(id) | Question::NotFound(id) => *id,
        }
    }
}

/// The questions waiting, and what the last read held.
#[derive(Debug, Default)]
pub struct Questions {
    /// Waiting for the presentation to be free, in the order they came.
    waiting: VecDeque<Question>,
    /// Each row's name, and whether it had failed, at the last read: what is
    /// new since is what is asked about.
    seen: HashMap<Id, (String, bool)>,
    /// Whether the first read is over: what it held is not asked about (rows
    /// of an earlier run).
    primed: bool,
}

impl Questions {
    /// Looks at the rows as just read. Only what is new since the last read
    /// is asked about, and the first read only notes what is there.
    pub fn watch(&mut self, rows: &[Arc<Entry>]) {
        let first = !self.primed;
        self.primed = true;
        let mut seen = std::mem::take(&mut self.seen);
        // Several rows new at once are a batch (Download all links), which
        // is not asked about names one by one.
        let fresh = rows.iter().filter(|r| !seen.contains_key(&r.id)).count();
        for row in rows {
            let failed = match &row.state {
                State::Failed(why) => Some(why.as_str()),
                _ => None,
            };
            let before = seen.insert(row.id, (row.filename.clone(), failed.is_some()));
            if first {
                continue;
            }
            let was_unnamed = before.as_ref().is_none_or(|(name, _)| name.is_empty());
            let chosen = row
                .save_name
                .as_deref()
                .is_some_and(|name| !name.trim().is_empty());
            // A download the browser handed over comes with the name the browser
            // gives it (and has a page it came from): not asked about. Nor one a
            // person has said yes to saving as an HTML page, which is named so.
            let typed_in = row.referrer.is_empty() && !row.accept_page;
            if was_unnamed && !row.filename.is_empty() && !chosen && typed_in && fresh < 2 {
                let address_name = provisional(&row.url);
                if row.filename != address_name
                    && without_copy_number(&row.filename) != address_name
                {
                    self.waiting.push_back(Question::Name {
                        id: row.id,
                        address_name,
                        server_name: row.filename.clone(),
                    });
                }
            }
            let newly_failed = before.as_ref().is_none_or(|(_, was)| !*was);
            if let (true, Some(why)) = (newly_failed, failed) {
                if why.contains("answered with a web page rather than the file") && !row.accept_page
                {
                    self.waiting.push_back(Question::Page(row.id));
                } else if why.contains("answered 404") {
                    self.waiting.push_back(Question::NotFound(row.id));
                }
            }
        }
        let ids: HashSet<Id> = rows.iter().map(|r| r.id).collect();
        seen.retain(|id, _| ids.contains(id));
        self.seen = seen;
    }

    /// The next question to ask, in the order they came, skipping those
    /// whose row is gone (`exists` says which are not).
    pub fn next(&mut self, exists: impl Fn(Id) -> bool) -> Option<Question> {
        while let Some(question) = self.waiting.pop_front() {
            if exists(question.id()) {
                return Some(question);
            }
        }
        None
    }
}

/// `name` without the " (2)" dm puts before the extension to keep two copies
/// of one file apart: that number is dm's, not something the server sent.
pub fn without_copy_number(name: &str) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    if let Some(open) = stem.rfind(" (") {
        if let Some(digits) = stem[open + 2..].strip_suffix(')') {
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                return format!("{}{}", &stem[..open], ext);
            }
        }
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: Id, url: &str, name: &str, state: State) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": id,
                "url": url,
                "state": serde_json::to_value(state).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": name,
            }))
            .unwrap(),
        )
    }

    #[test]
    fn the_number_dm_adds_to_a_second_copy_is_not_the_servers() {
        assert_eq!(without_copy_number("again (2).bin"), "again.bin");
        assert_eq!(without_copy_number("again (12)"), "again");
        assert_eq!(
            without_copy_number("Report (final).pdf"),
            "Report (final).pdf"
        );
        assert_eq!(without_copy_number("a.b (1).tar.gz"), "a.b (1).tar.gz");
        assert_eq!(without_copy_number("plain.bin"), "plain.bin");
    }

    #[test]
    fn what_the_first_read_holds_is_not_asked_about() {
        let mut questions = Questions::default();
        questions.watch(&[
            row(1, "http://e.com/a.bin", "other.bin", State::Running),
            row(
                2,
                "http://e.com/b.bin",
                "b.bin",
                State::Failed("answered 404".into()),
            ),
        ]);
        assert_eq!(questions.next(|_| true), None);
    }

    #[test]
    fn a_server_name_a_page_and_a_404_are_asked_once_each_in_order() {
        let mut questions = Questions::default();
        questions.watch(&[]);
        // Named by the server once it answered.
        questions.watch(&[row(1, "http://e.com/get?id=7", "", State::Queued)]);
        questions.watch(&[row(
            1,
            "http://e.com/get?id=7",
            "report.pdf",
            State::Running,
        )]);
        let page = "the server answered with a web page rather than the file";
        questions.watch(&[
            row(1, "http://e.com/get?id=7", "report.pdf", State::Running),
            row(2, "http://e.com/f.zip", "f.zip", State::Failed(page.into())),
        ]);
        questions.watch(&[
            row(1, "http://e.com/get?id=7", "report.pdf", State::Running),
            row(2, "http://e.com/f.zip", "f.zip", State::Failed(page.into())),
            row(
                3,
                "http://e.com/g.bin",
                "g.bin",
                State::Failed("answered 404".into()),
            ),
        ]);
        assert_eq!(
            questions.next(|_| true),
            Some(Question::Name {
                id: 1,
                address_name: provisional("http://e.com/get?id=7"),
                server_name: "report.pdf".into(),
            })
        );
        assert_eq!(questions.next(|_| true), Some(Question::Page(2)));
        assert_eq!(questions.next(|_| true), Some(Question::NotFound(3)));
        assert_eq!(questions.next(|_| true), None);
        // The same rows again raise nothing new.
        questions.watch(&[
            row(2, "http://e.com/f.zip", "f.zip", State::Failed(page.into())),
            row(
                3,
                "http://e.com/g.bin",
                "g.bin",
                State::Failed("answered 404".into()),
            ),
        ]);
        assert_eq!(questions.next(|_| true), None);
    }

    #[test]
    fn a_batch_a_copy_number_and_a_gone_row_raise_no_question() {
        let mut questions = Questions::default();
        questions.watch(&[]);
        // Two new at once: Download all links, not asked about names.
        questions.watch(&[
            row(1, "http://e.com/1", "one.bin", State::Running),
            row(2, "http://e.com/2", "two.bin", State::Running),
        ]);
        // The address's own name with dm's copy number.
        questions.watch(&[
            row(1, "http://e.com/1", "one.bin", State::Running),
            row(2, "http://e.com/2", "two.bin", State::Running),
            row(3, "http://e.com/f.bin", "f (2).bin", State::Running),
        ]);
        assert_eq!(questions.next(|_| true), None);
        questions.watch(&[row(
            4,
            "http://e.com/x",
            "",
            State::Failed("answered 404".into()),
        )]);
        assert_eq!(questions.next(|id| id != 4), None, "its row is gone");
    }
}
