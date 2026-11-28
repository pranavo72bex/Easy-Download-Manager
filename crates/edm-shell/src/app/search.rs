// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A saved site search's finds, sorted by the application: which files are new
//! (in neither the list nor what the search has seen), and what it remembers
//! as seen, so a file is offered once however often the search runs; and the
//! crawl itself, in the background ([`crawl_later`]). Adding what it found
//! and saying so are the presentation's.

use std::collections::HashSet;
use std::sync::Arc;

use edm_core::Entry;

use crate::lists::{bare, MOST_SEEN};

/// Crawls `search` in the background, with the application's client and the
/// queue's options (its headers, its agent), and gives `done` what it found,
/// or why not. The presentation is not held while pages are read (a site
/// search can take minutes).
pub fn crawl_later(
    backend: &crate::app::backend::Backend,
    search: crate::lists::Search,
    done: impl FnOnce(Result<crate::lists::Crawled, String>) + Send + 'static,
) {
    crawl_watched_later(backend, search, Arc::default(), done);
}

/// [`crawl_later`], telling `watch` how far it has come and stopped by it (gaps row U25).
pub fn crawl_watched_later(
    backend: &crate::app::backend::Backend,
    search: crate::lists::Search,
    watch: Arc<crate::lists::CrawlWatch>,
    done: impl FnOnce(Result<crate::lists::Crawled, String>) + Send + 'static,
) {
    let (client, queue) = (backend.client(), backend.queue.clone());
    backend.runtime.spawn(async move {
        let options = queue.options().await;
        let found = crate::lists::crawl_watched(&client, &search, Some(&options), &watch).await;
        done(found.map_err(String::from));
    });
}

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

/// The saved searches whose time has come at `epoch` (seconds), and the
/// settings with each one's last run stamped, to be saved before they run
/// (an unsaved last run runs the search again at every launch). `None` when
/// none is due.
pub fn due_now(
    settings: &crate::Settings,
    epoch: u64,
) -> Option<(Vec<crate::lists::Search>, crate::Settings)> {
    let due: Vec<crate::lists::Search> = settings
        .site_searches
        .iter()
        .filter(|s| s.due(epoch))
        .cloned()
        .collect();
    if due.is_empty() {
        return None;
    }
    let mut wanted = settings.clone();
    for search in &mut wanted.site_searches {
        if due.iter().any(|d| d.name == search.name) {
            search.last_run = epoch;
        }
    }
    Some((due, wanted))
}

/// What a search found, grouped by the folder under `download_dir` that
/// mirrors each file's address (the search's "Keep the site’s folders"):
/// each group's folder (`None`: the download folder itself) and its files,
/// in the folders' order.
pub fn by_site_folder(
    urls: &[String],
    download_dir: &std::path::Path,
) -> Vec<(Option<String>, Vec<String>)> {
    let mut groups: std::collections::BTreeMap<Option<String>, Vec<String>> =
        std::collections::BTreeMap::new();
    for url in urls {
        groups
            .entry(crate::lists::layout_folder(url))
            .or_default()
            .push(url.clone());
    }
    groups
        .into_iter()
        .map(|(folder, files)| {
            let dir = folder.map(|f| download_dir.join(f).to_string_lossy().into_owned());
            (dir, files)
        })
        .collect()
}

/// The settings with what the search `name` has now seen (`more`) added to
/// what it had, as [`remembered`] keeps it; `None` when there is no such
/// search.
pub fn seen_saved(
    settings: &crate::Settings,
    name: &str,
    more: impl IntoIterator<Item = String>,
) -> Option<crate::Settings> {
    let mut wanted = settings.clone();
    let search = wanted.site_searches.iter_mut().find(|s| s.name == name)?;
    search.seen = remembered(&search.seen, more);
    Some(wanted)
}

#[cfg(test)]
mod tests;
