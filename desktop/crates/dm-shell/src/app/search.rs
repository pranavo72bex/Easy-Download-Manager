// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A saved site search's finds, sorted by the application: which files are new
//! (in neither the list nor what the search has seen), and what it remembers
//! as seen, so a file is offered once however often the search runs; and the
//! crawl itself, in the background ([`crawl_later`]). Adding what it found
//! and saying so are the presentation's.

use std::collections::HashSet;
use std::sync::Arc;

use dm_core::Entry;

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
        done(crate::lists::crawl_watched(&client, &search, Some(&options), &watch).await);
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
/// mirrors each file's address (the search's "Keep the site's folders"):
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

    fn search(name: &str, every_hours: u64, last_run: u64) -> crate::lists::Search {
        crate::lists::Search {
            name: name.into(),
            page: "https://example.com/files/".into(),
            every_hours,
            last_run,
            ..Default::default()
        }
    }

    #[test]
    fn the_searches_due_are_given_with_their_last_run_stamped() {
        let hour = 3600;
        let settings = crate::Settings {
            site_searches: vec![
                search("hourly", 1, 0),
                search("daily", 24, 10 * hour),
                search("by hand", 0, 0),
            ],
            ..crate::Settings::default()
        };
        let (due, wanted) = due_now(&settings, 12 * hour).unwrap();
        assert_eq!(
            due.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["hourly"]
        );
        let runs: Vec<u64> = wanted.site_searches.iter().map(|s| s.last_run).collect();
        assert_eq!(runs, [12 * hour, 10 * hour, 0]);
        assert!(
            due_now(&wanted, 12 * hour + 60).is_none(),
            "not again within the hour"
        );
    }

    #[test]
    fn finds_are_grouped_by_the_folder_their_address_mirrors() {
        let urls = [
            "https://example.com/a/b/one.zip".to_string(),
            "https://example.com/two.zip".to_string(),
            "https://example.com/a/b/three.zip".to_string(),
        ];
        let groups = by_site_folder(&urls, std::path::Path::new("/dl"));
        let folder = crate::lists::layout_folder(&urls[0]).map(|f| {
            std::path::Path::new("/dl")
                .join(f)
                .to_string_lossy()
                .into_owned()
        });
        let top = crate::lists::layout_folder(&urls[1]).map(|f| {
            std::path::Path::new("/dl")
                .join(f)
                .to_string_lossy()
                .into_owned()
        });
        let both = groups.iter().find(|(dir, _)| *dir == folder).unwrap();
        assert_eq!(both.1, [urls[0].clone(), urls[2].clone()]);
        assert!(groups
            .iter()
            .any(|(dir, files)| *dir == top && files == &[urls[1].clone()]));
        assert_eq!(groups.iter().map(|(_, f)| f.len()).sum::<usize>(), 3);
    }

    #[test]
    fn what_a_search_has_seen_is_kept_with_it() {
        let settings = crate::Settings {
            site_searches: vec![search("s", 0, 0)],
            ..crate::Settings::default()
        };
        let wanted = seen_saved(&settings, "s", ["https://e.com/f.zip".to_string()]).unwrap();
        assert_eq!(wanted.site_searches[0].seen, ["https://e.com/f.zip"]);
        assert!(seen_saved(&settings, "none such", Vec::new()).is_none());
    }
}
