// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A saved site search's finds, sorted by the application: which files are new
//! (in neither the list nor what the search has seen), and what it remembers
//! as seen, so a file is offered once however often the search runs. Running
//! the search, adding what it found and saying so are the presentation's.

use std::collections::HashSet;
use std::sync::Arc;

use dm_core::Entry;

use crate::lists::{bare, MOST_SEEN};

/// A search's files, sorted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Finds {
    /// Files in neither the list nor what the search has seen: to add.
    pub new: Vec<String>,
    /// Files already in the list, by their bare address: to remember as seen.
    pub listed: Vec<String>,
}

/// Sorts a search's `found` files against the `rows` (every row, cancelled
/// ones too: a file the person removed is not offered again) and what the
/// search has `seen`.
pub fn sort_finds(found: &[String], rows: &[Arc<Entry>], seen: &[String]) -> Finds {
    let seen: HashSet<String> = seen.iter().cloned().collect();
    let listed_now = listed(rows);
    Finds {
        new: found
            .iter()
            .filter(|f| {
                let f = bare(f);
                !listed_now.contains(&f) && !seen.contains(&f)
            })
            .cloned()
            .collect(),
        listed: found
            .iter()
            .map(|f| bare(f))
            .filter(|f| listed_now.contains(f))
            .collect(),
    }
}

/// Every row's bare address.
pub fn listed(rows: &[Arc<Entry>]) -> HashSet<String> {
    rows.iter().map(|r| bare(&r.url)).collect()
}

/// `seen` with each of `more` added once, keeping the newest `MOST_SEEN`.
pub fn remembered(seen: &[String], more: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = seen.to_vec();
    for f in more {
        if !seen.contains(&f) {
            seen.push(f);
        }
    }
    let over = seen.len().saturating_sub(MOST_SEEN);
    seen.drain(..over);
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(url: &str) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": 1, "url": url,
                "state": serde_json::to_value(dm_core::State::Cancelled).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "f",
            }))
            .unwrap(),
        )
    }

    /// A file in the list (even a cancelled row) or seen before is not new; a
    /// listed one is remembered as seen.
    #[test]
    fn a_find_is_new_only_if_neither_listed_nor_seen() {
        let found = vec![
            "https://h.example/a".to_string(),
            "https://h.example/b".to_string(),
            "https://h.example/c".to_string(),
        ];
        let finds = sort_finds(
            &found,
            &[row("https://h.example/a")],
            &["https://h.example/b".to_string()],
        );
        assert_eq!(finds.new, vec!["https://h.example/c".to_string()]);
        assert_eq!(finds.listed, vec!["https://h.example/a".to_string()]);
    }

    /// Each remembered once, and no more than `MOST_SEEN`, the oldest going.
    #[test]
    fn what_is_seen_is_kept_once_and_capped() {
        let seen = remembered(&["a".to_string()], ["a".to_string(), "b".to_string()]);
        assert_eq!(seen, vec!["a".to_string(), "b".to_string()]);
        let many: Vec<String> = (0..MOST_SEEN + 3).map(|n| n.to_string()).collect();
        let kept = remembered(&[], many);
        assert_eq!(kept.len(), MOST_SEEN);
        assert_eq!(kept[0], "3");
    }
}
