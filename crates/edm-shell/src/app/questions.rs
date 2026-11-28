// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The questions the rows raise, as reference A asks them (maintainer;
//! contract rows B39 to B41): a name the server sent that is not
//! the address's, a web page where a file was asked for, a 404. Found by
//! comparing each read with the last, and kept until the presentation is free
//! to ask, one at a time. How each is asked is the presentation's.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use edm_core::{Entry, FailKind, Id};

use super::names::address_file_name;

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
    /// The same file as one already here, stopped before its body (gaps row W18): show that
    /// one, or download it again.
    SameFile(Id),
}

impl Question {
    /// The row it is about.
    pub fn id(&self) -> Id {
        match self {
            Question::Name { id, .. } | Question::Page(id) | Question::SameFile(id) => *id,
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
            let failed = row.failed_as();
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
            // Named only to be asked whether it is the same file: that question alone.
            let same = failed == Some(FailKind::SameFile);
            if was_unnamed && !same && !row.filename.is_empty() && !chosen && typed_in && fresh < 2
            {
                let address_name = address_file_name(&row.url);
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
            if let (true, Some(kind)) = (newly_failed, failed) {
                if kind == FailKind::WebPage && !row.accept_page {
                    self.waiting.push_back(Question::Page(row.id));
                } else if kind == FailKind::SameFile && !row.accept_same {
                    self.waiting.push_back(Question::SameFile(row.id));
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

/// `name` without the " (2)" EDM puts before the extension to keep two copies
/// of one file apart: that number is EDM's, not something the server sent.
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
mod tests;
