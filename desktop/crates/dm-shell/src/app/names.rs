// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a row is called and where its file will go, as every presentation
//! shows it: the name it is shown by until the server names it, its kind,
//! the folder it is bound for and the free name it will get there. Only
//! shown, never written into the row: what a command reads of a row is what
//! the person chose.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_core::{Category, Entry, Id, State};

/// Where a URL came from, which is the second thing anybody wants to know
/// about a download. A magnet has no host — it is a swarm, not a server.
pub fn host(url: &str) -> String {
    if url.starts_with("magnet:") {
        return "torrent".into();
    }
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_string())
        })
        .unwrap_or_default()
}

/// What to call a row the probe has not reached yet: the last path segment,
/// which is the engine's own fallback, or a magnet's `dn`.
pub fn provisional(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    if parsed.scheme() == "magnet" {
        return parsed
            .query_pairs()
            .find(|(k, _)| k == "dn")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_else(|| "Magnet link".into());
    }
    let last = parsed
        .path_segments()
        .and_then(|mut s| s.next_back().map(str::to_owned))
        .unwrap_or_default();
    let last = percent_decode(&last);
    if last.is_empty() {
        return host(url);
    }
    // A page made by a script, named by its query (`watch?v=…`), is shown
    // by its short address until it is named: a failed YouTube video's row
    // said only "watch".
    match parsed.query().filter(|q| !q.is_empty()) {
        Some(query) if !last.contains('.') => {
            let host = parsed.host_str().unwrap_or_default();
            let host = host.strip_prefix("www.").unwrap_or(host);
            let path = parsed.path().trim_start_matches('/');
            let shown = format!("{host}/{path}?{query}");
            match shown.char_indices().nth(60) {
                Some((at, _)) => format!("{}…", &shown[..at]),
                None => shown,
            }
        }
        _ => last,
    }
}

fn percent_decode(text: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", text.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| text.to_string())
}

/// The name a row is shown by: the file's, or a guess at it until the
/// server says.
pub fn name_of(row: &Entry) -> String {
    if !row.filename.is_empty() {
        return row.filename.clone();
    }
    // The name chosen in Download details, until the file is saved under
    // it: a row added for later showed its address's name.
    match row.save_name.as_deref().map(str::trim) {
        Some(chosen) if !chosen.is_empty() => chosen.to_string(),
        _ => provisional(&row.url),
    }
}

/// A row's kind: what its name says, else what dm learned from the server.
/// Queued rows had none, so Videos listed one of a hundred queued videos
/// and their icons were blank; and with what the server said
/// first, a file renamed to "report (1).pdf" stayed under Other.
/// One named "get.php" is still known by what the server sent.
pub fn kind_of(row: &Entry) -> Option<Category> {
    dm_core::by_name(&name_of(row)).or(row.category)
}

/// Where a row not saved anywhere yet will go, for the Folder column
/// to show until it is: the download folder, or the kind's folder under
/// it where Options › Folders sorts by kind — as the queue places it.
/// Nothing where that waits on the server: a name that does not say its
/// kind (`download.php`, `.bin`) is placed by the type the server gives.
/// Only shown, never written into the row: what a command
/// reads of a row is what the person chose.
pub fn planned_folder(
    row: &Entry,
    download_dir: &Path,
    categories: &mut dm_core::Categories,
) -> Option<String> {
    // Not for one finished or cancelled; a failed one still goes there
    // once retried, and lost its Folder.
    if !row.path.is_empty()
        || row.save_dir.is_some()
        || matches!(row.state, State::Completed | State::Cancelled)
    {
        return None;
    }
    // The folder it began in, once it has begun.
    let root = if row.began_in.is_empty() {
        download_dir.to_path_buf()
    } else {
        std::path::PathBuf::from(&row.began_in)
    };
    if !categories.enabled {
        return Some(root.display().to_string());
    }
    let name = if row.filename.is_empty() {
        provisional(&row.url)
    } else {
        row.filename.clone()
    };
    categories.site = dm_core::host_of(&row.url);
    let kind = match kind_of(row).or_else(|| dm_core::by_name(&name)) {
        Some(kind) => kind,
        None if categories.custom_for(&name, &categories.site).is_some() => {
            dm_core::Category::Other
        }
        None => return None,
    };
    Some(categories.place(&root, kind, &name).display().to_string())
}

/// For every row not yet placed, how many rows before it are bound for the
/// same folder under the same name: what `name_it_will_get_after` is told.
/// One pass, in the order the rows were added.
pub fn earlier_of_the_same_name(rows: &[Arc<Entry>]) -> HashMap<Id, usize> {
    let mut order: Vec<&Entry> = rows.iter().map(|r| &**r).collect();
    order.sort_by_key(|r| r.id);
    let mut seen: HashMap<(Option<String>, String), usize> = HashMap::new();
    let mut before = HashMap::new();
    for r in order {
        if r.state.settled() || !r.path.is_empty() {
            continue;
        }
        let count = seen.entry((r.save_dir.clone(), name_of(r))).or_insert(0);
        before.insert(r.id, *count);
        *count += 1;
    }
    before
}

/// The name a row's file will have in its folder: for one not yet placed,
/// with the ` (1)` it will get where its folder already has that name (a
/// file added again said `big.bin` until it finished as `big (1).bin`). The
/// folder is asked at most every two seconds per row. `known_before`, where
/// known, is how many earlier rows of the same name bound for the same
/// folder take a free name first (`earlier_of_the_same_name`), as a list of
/// every row's name needs it once for all and not by a search of every row
/// for each.
pub fn name_it_will_get_after(
    rows: &[Arc<Entry>],
    row: &Entry,
    download_dir: &Path,
    categories: &mut dm_core::Categories,
    known_before: Option<usize>,
) -> String {
    let name = name_of(row);
    if !row.path.is_empty() || matches!(row.state, State::Completed | State::Cancelled) {
        return name;
    }
    let folder = row
        .save_dir
        .clone()
        .filter(|d| !d.trim().is_empty())
        .or_else(|| planned_folder(row, download_dir, categories))
        .unwrap_or_else(|| download_dir.display().to_string());
    let key = (row.id, std::path::Path::new(&folder).join(&name));
    thread_local! {
        static FREE: std::cell::RefCell<HashMap<(Id, std::path::PathBuf), (Instant, String)>> =
            std::cell::RefCell::new(HashMap::new());
    }
    FREE.with(|free| {
        let mut free = free.borrow_mut();
        if let Some((at, said)) = free.get(&key) {
            if at.elapsed() < Duration::from_secs(2) {
                return said.clone();
            }
        }
        if free.len() > 2000 {
            free.clear();
        }
        // Each earlier row of the same name bound for the same folder,
        // and not yet placed, takes a free name first: three added for
        // later all read "slow (1).bin".
        let before = known_before.unwrap_or_else(|| {
            rows.iter()
                .filter(|r| {
                    r.id < row.id
                        && !r.state.settled()
                        && r.path.is_empty()
                        && r.save_dir == row.save_dir
                        && name_of(r) == name
                })
                .count()
        });
        let said = dm_core::nth_free_name(std::path::Path::new(&folder), &name, before);
        free.insert(key, (Instant::now(), said.clone()));
        said
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, url: &str, state: State) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": id,
                "url": url,
                "state": serde_json::to_value(state).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "",
            }))
            .unwrap(),
        )
    }

    #[test]
    fn rows_bound_for_one_name_each_take_the_next_free_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.bin"), b"there").unwrap();
        let rows = vec![
            row(1, "https://e.com/big.bin", State::Queued),
            row(2, "https://e.com/big.bin", State::Queued),
            row(3, "https://e.com/other.zip", State::Queued),
        ];
        let before = earlier_of_the_same_name(&rows);
        assert_eq!((before[&1], before[&2], before[&3]), (0, 1, 0));
        let mut categories = dm_core::Categories {
            enabled: false,
            ..Default::default()
        };
        let named = |r: &Entry, known| {
            name_it_will_get_after(&rows, r, dir.path(), &mut categories.clone(), known)
        };
        assert_eq!(named(&rows[0], None), "big (1).bin");
        assert_eq!(named(&rows[1], Some(before[&2])), "big (2).bin");
        assert_eq!(named(&rows[2], None), "other.zip");
        assert_eq!(
            planned_folder(&rows[0], dir.path(), &mut categories),
            Some(dir.path().display().to_string())
        );
    }

    #[test]
    fn a_settled_row_keeps_its_own_name_and_has_no_planned_folder() {
        let done = row(4, "https://e.com/done.iso", State::Completed);
        let mut categories = dm_core::Categories::default();
        assert_eq!(
            planned_folder(&done, Path::new("/nowhere"), &mut categories),
            None
        );
        // Not yet named by the server: its address's name, and its kind
        // from that.
        assert_eq!(name_of(&done), "done.iso");
        let film = row(5, "https://e.com/v/film.mp4?t=1", State::Queued);
        assert_eq!(name_of(&film), "film.mp4");
        assert_eq!(kind_of(&film), Some(Category::Videos));
    }

    /// A page named by a long query is cut to sixty characters; a name of no
    /// known kind goes to a category a person made that takes it, and to no
    /// planned folder otherwise.
    #[test]
    fn a_long_query_name_is_cut_and_a_persons_category_places_an_unknown_kind() {
        let long = format!("https://www.e.com/watch?v={}", "x".repeat(80));
        let shown = provisional(&long);
        assert!(shown.starts_with("e.com/watch?v=xx"), "{shown}");
        assert!(shown.ends_with('…'), "{shown}");
        assert_eq!(shown.chars().count(), 61);
        assert_eq!(
            provisional("https://e.com/watch?v=abc"),
            "e.com/watch?v=abc"
        );

        let dir = tempfile::tempdir().unwrap();
        let odd = row(6, "https://e.com/notes.zzq", State::Queued);
        let mut categories = dm_core::Categories {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(planned_folder(&odd, dir.path(), &mut categories), None);
        categories.custom = vec![dm_core::Custom {
            name: "Odd".into(),
            extensions: vec!["zzq".into()],
            ..Default::default()
        }];
        assert_eq!(
            planned_folder(&odd, dir.path(), &mut categories),
            Some(dir.path().join("Odd").display().to_string())
        );
    }
}
