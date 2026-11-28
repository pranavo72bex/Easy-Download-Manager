// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
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

use edm_core::{Category, Entry, Id, State};

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
///
/// Kept by address once worked out, on the thread that asked: the window
/// names every row from it at a read, twice, and parsing ten thousand
/// addresses each time was most of what a read cost after Stop on ten
/// thousand rows (gaps row 183).
pub fn provisional(url: &str) -> String {
    /// How many names are kept before they are all let go.
    const KEPT: usize = 50_000;
    thread_local! {
        static NAMED: std::cell::RefCell<std::collections::HashMap<String, String>> =
            std::cell::RefCell::default();
    }
    NAMED.with(|named| {
        if let Some(name) = named.borrow().get(url) {
            return name.clone();
        }
        let name = provisional_of(url);
        let mut named = named.borrow_mut();
        if named.len() >= KEPT {
            named.clear();
        }
        named.insert(url.to_string(), name.clone());
        name
    })
}

/// [`provisional`], worked out.
fn provisional_of(url: &str) -> String {
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

/// The name the address itself gives the file, as the engine names it when
/// the server sends none: its last path segment, decoded (a magnet's `dn`).
/// What B39 sets against the server's name, and what No saves it as; not
/// `provisional`, which shows a script's page by its short address
/// (`example.com/dl?id=7`), a row's label rather than a file's name.
pub fn address_file_name(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    if parsed.scheme() == "magnet" {
        return provisional(url);
    }
    let last = parsed
        .path_segments()
        .and_then(|mut s| s.next_back().map(str::to_owned))
        .unwrap_or_default();
    let last = percent_decode(&last);
    if last.is_empty() {
        host(url)
    } else {
        last
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
    name_seen(row).into_owned()
}

/// [`name_of`], borrowed from the row where it is the row's own.
fn name_seen(row: &Entry) -> std::borrow::Cow<'_, str> {
    if !row.filename.is_empty() {
        return std::borrow::Cow::Borrowed(&row.filename);
    }
    // The name chosen in Download details, until the file is saved under
    // it: a row added for later showed its address's name.
    match row.save_name.as_deref().map(str::trim) {
        Some(chosen) if !chosen.is_empty() => std::borrow::Cow::Borrowed(chosen),
        _ => std::borrow::Cow::Owned(provisional(&row.url)),
    }
}

/// A row's kind: what its name says, else what EDM learned from the server.
/// Queued rows had none, so Videos listed one of a hundred queued videos
/// and their icons were blank; and with what the server said
/// first, a file renamed to "report (1).pdf" stayed under Other.
/// One named "get.php" is still known by what the server sent.
pub fn kind_of(row: &Entry) -> Option<Category> {
    edm_core::by_name(&name_of(row)).or(row.category)
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
    categories: &mut edm_core::Categories,
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
    categories.site = edm_core::host_of(&row.url);
    let kind = match kind_of(row).or_else(|| edm_core::by_name(&name)) {
        Some(kind) => kind,
        None if categories.custom_for(&name, &categories.site).is_some() => {
            edm_core::Category::Other
        }
        None => return None,
    };
    Some(categories.place(&root, kind, &name).display().to_string())
}

/// For every row not yet placed, how many rows before it are bound for the
/// same folder under the same name: what `name_it_will_get_after` is told.
/// One pass, in the order the rows were added.
pub fn earlier_of_the_same_name(rows: &[Arc<Entry>]) -> HashMap<Id, usize> {
    earlier_and_alike(rows, &[]).0
}

/// [`earlier_of_the_same_name`], and in the same pass where in `rows` (in order) the rows not
/// yet placed are that are bound for one of the folders `named` under its name (each a row's
/// `save_dir` and [`name_of`]): the rows whose names a change of a row of that name and folder
/// can change, where every row had been named again (gaps row 181).
pub fn earlier_and_alike(
    rows: &[Arc<Entry>],
    named: &[(Option<String>, String)],
) -> (HashMap<Id, usize>, Vec<usize>) {
    // Only the rows not yet placed, sorted only where the list is not already in the order
    // they were added, and each folder and name borrowed from its row: at 10,000 waiting rows
    // every row was sorted and its folder and name copied at each change of the list (gaps
    // row 210).
    let mut order: Vec<(usize, &Entry)> = rows
        .iter()
        .enumerate()
        .map(|(at, r)| (at, &**r))
        .filter(|(_, r)| !r.state.settled() && r.path.is_empty())
        .collect();
    if !order.is_sorted_by_key(|(_, r)| r.id) {
        order.sort_by_key(|(_, r)| r.id);
    }
    let mut seen: HashMap<(Option<&str>, std::borrow::Cow<'_, str>), usize> =
        HashMap::with_capacity(order.len());
    let mut before = HashMap::with_capacity(order.len());
    let mut alike = Vec::new();
    for (at, r) in order {
        let name = name_seen(r);
        // A few names, those of the rows that changed: looked through as they are.
        if named
            .iter()
            .any(|(folder, n)| folder.as_deref() == r.save_dir.as_deref() && *n == *name)
        {
            alike.push(at);
        }
        let count = seen.entry((r.save_dir.as_deref(), name)).or_insert(0);
        before.insert(r.id, *count);
        *count += 1;
    }
    alike.sort_unstable();
    (before, alike)
}

/// How long the free name a row's folder was asked for is believed before the folder is asked
/// again (`name_it_will_get_after`).
const FREE_NAME_FOR: Duration = Duration::from_secs(2);

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
    categories: &mut edm_core::Categories,
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
        static LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(2000) };
    }
    FREE.with(|free| {
        let mut free = free.borrow_mut();
        if let Some((at, said)) = free.get(&key) {
            if at.elapsed() < FREE_NAME_FOR {
                return said.clone();
            }
        }
        // Kept small by dropping what has expired, not by emptying it:
        // emptied past 2,000, a list of 10,000 rows waiting asked the disk
        // for every one of them at every read (gaps row 181). Looked over
        // again only once it has doubled, so the look costs nothing a row.
        if free.len() > LIMIT.get() {
            free.retain(|_, (at, _)| at.elapsed() < FREE_NAME_FOR);
            LIMIT.set((free.len() * 2).max(2000));
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
        let said = nth_free_in(std::path::Path::new(&folder), &name, before);
        free.insert(key, (Instant::now(), said.clone()));
        said
    })
}

/// [`edm_core::nth_free_name`], with a folder asked about many names at once listed once
/// instead (Windows). Naming 10,300 waiting rows looked at the disk 10,300 times on Windows,
/// 0.78 s of a 0.93 s read, where the same on macOS is a few milliseconds (gaps rows 210, S16).
fn nth_free_in(folder: &Path, name: &str, nth: usize) -> String {
    #[cfg(windows)]
    if let Some(listed) = listing::of(folder) {
        // NTFS compares names without case and keeps them as written, so a name the disk has
        // is in the listing in some case; one found there is still asked of the disk.
        return edm_core::nth_free_name_with(folder, name, nth, |path| {
            path.file_name()
                .is_some_and(|n| listed.contains(&n.to_string_lossy().to_lowercase()))
        });
    }
    edm_core::nth_free_name(folder, name, nth)
}

/// Each folder's names, listed once it has been asked about many names in a short while.
#[cfg(windows)]
mod listing {
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    /// How long a listing, or a count of names asked, stands: a pass naming every row ends
    /// within it, and a file arriving is seen no later than a name kept for two seconds is.
    const FOR: Duration = Duration::from_millis(500);
    /// Names asked of a folder within [`FOR`] before it is listed: a few rows named every
    /// two seconds look at the disk as before, a folder of many files is not listed for them.
    const AFTER: usize = 64;

    struct Folder {
        since: Instant,
        asked: usize,
        /// `None` until listed; a folder that could not be read is kept as `Some(None)`.
        listed: Option<Option<Rc<HashSet<String>>>>,
    }

    thread_local! {
        static FOLDERS: std::cell::RefCell<HashMap<PathBuf, Folder>> =
            std::cell::RefCell::new(HashMap::new());
    }

    /// `folder`'s names in lower case, where it has been asked about more than [`AFTER`] names
    /// within [`FOR`] and could be read; `None` while each name is to be asked of the disk.
    pub(super) fn of(folder: &Path) -> Option<Rc<HashSet<String>>> {
        FOLDERS.with(|folders| {
            let mut folders = folders.borrow_mut();
            if folders.len() > 256 {
                folders.retain(|_, f| f.since.elapsed() < FOR);
            }
            let entry = folders.entry(folder.to_path_buf()).or_insert(Folder {
                since: Instant::now(),
                asked: 0,
                listed: None,
            });
            if entry.since.elapsed() >= FOR {
                *entry = Folder {
                    since: Instant::now(),
                    asked: 0,
                    listed: None,
                };
            }
            entry.asked += 1;
            if entry.listed.is_none() && entry.asked > AFTER {
                // A folder not made yet holds nothing; one that cannot be read otherwise is
                // asked name by name.
                entry.listed = Some(match std::fs::read_dir(folder) {
                    Ok(dir) => Some(Rc::new(
                        dir.flatten()
                            .map(|e| e.file_name().to_string_lossy().to_lowercase())
                            .collect(),
                    )),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        Some(Rc::new(HashSet::new()))
                    }
                    Err(_) => None,
                });
            }
            entry.listed.clone().flatten()
        })
    }
}

/// Renames asked for before a row's file is whole ("use the address's
/// name"): a finished row is renamed at once, one still under way when it
/// finishes, and one that failed or was cancelled (or removed) never.
#[derive(Debug, Default)]
pub struct PendingRenames(HashMap<Id, String>);

impl PendingRenames {
    /// `id`, in `state` (`None`: no such row), is to be called `name`: the
    /// name to rename it to now, or `None` (kept for later, or dropped).
    pub fn ask(&mut self, id: Id, state: Option<&State>, name: &str) -> Option<String> {
        match state {
            Some(State::Completed) => Some(name.to_string()),
            Some(State::Failed(_) | State::Cancelled) | None => None,
            Some(_) => {
                self.0.insert(id, name.to_string());
                None
            }
        }
    }

    /// The renames due now, by each row's state (`state_of`): those
    /// finished, taken out; those that failed, were cancelled or are gone,
    /// dropped.
    pub fn due(&mut self, state_of: impl Fn(Id) -> Option<State>) -> Vec<(Id, String)> {
        let mut now = Vec::new();
        self.0.retain(|id, name| match state_of(*id) {
            Some(State::Completed) => {
                now.push((*id, std::mem::take(name)));
                false
            }
            Some(State::Failed(_) | State::Cancelled) | None => false,
            Some(_) => true,
        });
        now.sort_by_key(|(id, _)| *id);
        now
    }
}

/// A category's name as the settings and the control protocol write it.
#[must_use]
pub fn category_word(c: Category) -> String {
    serde_json::to_value(c)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A category's folder under `root`, by the category's word (a kind, as
/// [`category_word`] writes it) or a person's own category's name.
pub fn category_folder(
    word: &str,
    root: &Path,
    categories: &edm_core::Categories,
) -> std::path::PathBuf {
    match Category::ALL.iter().find(|c| category_word(**c) == word) {
        Some(kind) => categories.destination(root, *kind),
        None => root.join(word),
    }
}

#[cfg(test)]
mod tests;
