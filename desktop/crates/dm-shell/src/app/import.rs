// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A list of downloads taken in (Import): how much of it is already listed,
//! which a presentation asks about first, as Add URL asks; the rows it adds;
//! and the lines that held no link, which are said rather than passed over.
//! Reading the file, the question and the words are the presentation's.

use std::collections::HashSet;
use std::sync::Arc;

use dm_core::{Entry, Id, State};

use crate::app::backend::Backend;

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    /// How many downloads were added.
    pub added: usize,
    /// The rows it added, which a presentation treats as its own (a list is
    /// not a run of progress windows).
    pub new: Vec<Id>,
    /// How many lines of a plain list held no address dm can read: said,
    /// not passed over ("127.0.0.1:8000/notes.txt" went without a word).
    pub unread: usize,
}

/// The addresses of `rows` as `rules::already_listed` compares them
/// (`lists::bare`), a cancelled row's left out: made once for a list, not
/// gone through for each of its links (10,000 links at 10,000 rows was a
/// hundred million comparisons).
fn listed_addresses(rows: &[Arc<Entry>]) -> HashSet<String> {
    rows.iter()
        .filter(|r| r.state != State::Cancelled)
        .map(|r| crate::lists::bare(&r.url))
        .collect()
}

/// How many of the addresses list `text` holds are already rows of `rows`
/// (as `rules::already_listed` finds them), and how many it holds in all.
pub fn already_listed(rows: &[Arc<Entry>], text: &str) -> (usize, usize) {
    let listed = Backend::listed(text);
    let known = listed_addresses(rows);
    let already = listed
        .iter()
        .filter(|url| known.contains(&crate::lists::bare(url)))
        .count();
    (already, listed.len())
}

/// Adds the downloads list `text` holds; with `skip_listed`, not those whose
/// address is already a row of `rows`.
///
/// The rows it added are those the queue gave it ids for, not the rows new
/// since it began: a browser's hand-off added meanwhile is not the list's.
pub fn import(backend: &Backend, rows: &[Arc<Entry>], text: &str, skip_listed: bool) -> Imported {
    let skipped: HashSet<String> = if skip_listed {
        let known = listed_addresses(rows);
        Backend::listed(text)
            .into_iter()
            .filter(|url| known.contains(&crate::lists::bare(url)))
            .collect()
    } else {
        HashSet::new()
    };
    let new = backend.import_where(text, |url| !skipped.contains(url));
    Imported {
        added: new.len(),
        new,
        unread: unread_lines(text),
    }
}

/// The lines of a plain list (not dm's own JSON) with no link in them: a
/// link starts with a scheme (`http://`, `ftp://`…) or is a magnet. Blank
/// lines and `#` comments are not counted.
pub fn unread_lines(text: &str) -> usize {
    if text.trim_start().starts_with('{') {
        return 0;
    }
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| !l.contains("://") && !l.to_ascii_lowercase().contains("magnet:?"))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_with_no_link_are_counted_and_comments_are_not() {
        let text = "# mine\nhttp://127.0.0.1:9/a.bin\n\n127.0.0.1:8000/notes.txt\nMAGNET:?xt=urn:btih:abc\nnot a link\n";
        assert_eq!(unread_lines(text), 2);
        assert_eq!(unread_lines("{\"downloads\": []}"), 0);
    }

    #[test]
    fn a_list_is_added_and_what_is_listed_already_can_be_skipped() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = crate::app::backend::testing::backend(&runtime, dir.path());
        // Held for later, so nothing is fetched.
        let listed = backend
            .add_with(
                "http://127.0.0.1:9/a.bin",
                dm_core::AddOptions {
                    later: true,
                    ..dm_core::AddOptions::default()
                },
            )
            .unwrap();
        let rows: Vec<Arc<Entry>> = backend.list().into_iter().map(Arc::new).collect();
        let text = "http://127.0.0.1:9/a.bin\nhttp://127.0.0.1:9/b.bin\nno link here\n";
        assert_eq!(already_listed(&rows, text), (1, 2));
        let done = import(&backend, &rows, text, true);
        assert_eq!((done.added, done.new.len(), done.unread), (1, 1, 1));
        assert!(!done.new.contains(&listed));
    }
}
