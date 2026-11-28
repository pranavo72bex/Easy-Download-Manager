// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the app is fetching, for the browser's toolbar popup (gaps row U18).
//!
//! The browser's native host is a process of its own, started per message, and the control port
//! is open only where a person asked for it, so the host has no way to ask the window. The window
//! writes what is running into `going.json` in EDM's home as it reads the queue, at most once a
//! second while anything runs and once when it stops, and the host reads the file when the popup
//! asks (`downloads.list`). What it wrote is stamped, so an EDM that quit mid-download is not taken
//! for one still fetching.
//!
//! Beside what runs, the downloads stopped or finished last (gaps row C12), each with its row, so
//! the popup can ask for one to be opened, shown, resumed or stopped (`downloads.act`): the host
//! leaves that in the inbox ([`ask_window`]) and the window carries it out with its own commands
//! ([`asked_of_window`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use edm_core::{Entry, Id, State};
use edm_protocol::{act_does, recent_state, Act, Going, LastFailure, Recent, MOST_RECENT};
use serde::{Deserialize, Serialize};

use super::commands::Command;
use super::snapshot::Changes;

/// The most downloads the popup lists; the rest are counted.
pub const MOST: usize = 5;

/// How often the file is written again while downloads run.
const EVERY: Duration = Duration::from_secs(1);

/// How old the file may be and still say what runs: past this, the window has not read the queue
/// for longer than it ever waits while downloads run, so it is gone or stuck.
pub const STALE_MS: u64 = 5_000;

/// What `going.json` holds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Published {
    /// When it was written, in milliseconds since 1970.
    pub at_ms: u64,
    /// The downloads running, the fastest first, [`MOST`] at most.
    pub downloads: Vec<Going>,
    /// How many run in all.
    pub running: usize,
    /// The last download that failed since this EDM started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<LastFailure>,
    /// The downloads stopped or finished last, the newest first ([`recent`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<Recent>,
}

/// The file's format number (`edm_core::saved`): 1, [`Published`]'s fields beside `format`.
const FORMAT: u32 = 1;

/// Where it is kept.
pub fn file(home: &Path) -> PathBuf {
    home.join(edm_core::paths::GOING)
}

/// The name a row is shown by.
fn name_of(row: &Entry) -> String {
    if row.filename.is_empty() {
        super::names::provisional(&row.url)
    } else {
        row.filename.clone()
    }
}

/// The downloads running among `rows`, the fastest first, [`MOST`] at most, and how many run.
pub fn running(rows: &[Arc<Entry>]) -> (Vec<Going>, usize) {
    running_of(rows.iter())
}

/// [`running`] of the rows `rows`, in the list's order.
fn running_of<'a>(rows: impl Iterator<Item = &'a Arc<Entry>>) -> (Vec<Going>, usize) {
    let mut going: Vec<Going> = rows
        .filter(|r| r.state == State::Running)
        .map(|r| Going {
            name: name_of(r),
            percent: r
                .progress
                .total
                .filter(|&t| t > 0)
                .map(|t| (r.progress.downloaded.min(t).saturating_mul(100) / t) as u8),
            speed: r.progress.speed.max(0.0) as u64,
            id: Some(r.id),
        })
        .collect();
    let count = going.len();
    going.sort_by(|a, b| b.speed.cmp(&a.speed).then_with(|| a.name.cmp(&b.name)));
    going.truncate(MOST);
    (going, count)
}

/// How a row shows under the popup's "Recent", where it does: finished, stopped by the person, or
/// failed. A row waiting or running is not recent, and a cancelled one has nothing to be done to.
fn shown_as(state: &State) -> Option<&'static str> {
    match state {
        State::Completed => Some(recent_state::FINISHED),
        State::Paused => Some(recent_state::STOPPED),
        State::Failed(_) => Some(recent_state::FAILED),
        State::Queued | State::Running | State::Cancelled => None,
    }
}

/// The downloads stopped or finished last among `rows`, [`MOST_RECENT`] at most: the stopped ones
/// first (they wait on the person, and keep no time they stopped at), the latest added first; then
/// the finished and failed ones, the latest to end first.
pub fn recent(rows: &[Arc<Entry>]) -> Vec<Recent> {
    let mut found: Vec<(&Arc<Entry>, &'static str)> = rows
        .iter()
        .filter_map(|r| shown_as(&r.state).map(|s| (r, s)))
        .collect();
    let newest = |r: &Entry| (r.finished_at.unwrap_or(u64::MAX), r.id);
    let by_newest =
        |a: &(&Arc<Entry>, &str), b: &(&Arc<Entry>, &str)| newest(b.0).cmp(&newest(a.0));
    if found.len() > MOST_RECENT {
        found.select_nth_unstable_by(MOST_RECENT, by_newest);
        found.truncate(MOST_RECENT);
    }
    found.sort_by(by_newest);
    found
        .into_iter()
        .map(|(r, state)| Recent {
            id: r.id,
            name: name_of(r),
            state: state.to_string(),
        })
        .collect()
}

/// Whether the changes of a read (`changes`) can have changed [`recent`]: every row, or a changed
/// row that is not running. A read of a running download's figures alone cannot, and is not made
/// to go through every row (gaps row 181).
fn recent_may_change(rows: &[Arc<Entry>], changes: &Changes) -> bool {
    match changes.at() {
        None => true,
        Some(at) => at
            .iter()
            .any(|&i| rows.get(i).is_none_or(|r| r.state != State::Running)),
    }
}

/// Writes `going.json` as the queue is read, no more often than it needs.
#[derive(Debug, Default)]
pub struct Publisher {
    /// What was written last, and when.
    written: Option<(Published, Instant)>,
    /// The last failure seen.
    failed: Option<LastFailure>,
    /// Where the running rows were at the last read (`publish`), followed
    /// by the rows that changed rather than looked for among every row
    /// (gaps row 181).
    running_at: BTreeSet<usize>,
    /// The recent downloads at the last read that could change them.
    recent: Vec<Recent>,
}

impl Publisher {
    /// What to write after a read of `rows`, in which `failed` failed since the read before;
    /// `None` where the file already says it. Written while anything runs every [`EVERY`], so
    /// its stamp stays fresh; once when the last stops; and at once when a download fails.
    pub fn next(
        &mut self,
        rows: &[Arc<Entry>],
        failed: &[Id],
        now: Instant,
        now_ms: u64,
    ) -> Option<Published> {
        let going = running(rows);
        self.recent = recent(rows);
        self.next_with(rows, going, failed, now, now_ms)
    }

    /// [`Self::next`], the downloads running among `rows` worked out already.
    fn next_with(
        &mut self,
        rows: &[Arc<Entry>],
        (downloads, running): (Vec<Going>, usize),
        failed: &[Id],
        now: Instant,
        now_ms: u64,
    ) -> Option<Published> {
        if let Some(row) = failed
            .iter()
            .rev()
            .find_map(|id| rows.iter().find(|r| r.id == *id))
        {
            if let State::Failed(why) = &row.state {
                self.failed = Some(LastFailure {
                    name: name_of(row),
                    why: why.clone(),
                });
            }
        }
        let wanted = Published {
            at_ms: now_ms,
            downloads,
            running,
            failed: self.failed.clone(),
            recent: self.recent.clone(),
        };
        let due = match &self.written {
            None => true,
            Some((was, at)) => {
                let new_failure = was.failed != wanted.failed || was.recent != wanted.recent;
                if wanted.running > 0 {
                    // Running: kept fresh every second, at once when the first starts or one fails.
                    now.duration_since(*at) >= EVERY || was.running == 0 || new_failure
                } else {
                    // Nothing running: written once as the last stops; the file then says so,
                    // however old it grows.
                    was.running > 0 || new_failure
                }
            }
        };
        if !due {
            return None;
        }
        self.written = Some((wanted.clone(), now));
        Some(wanted)
    }

    /// [`Publisher::next`], written into `home` where `browser` says a browser's extension has
    /// talked to this EDM: nobody else reads the file, so it costs nobody else a write. Asked only
    /// when a write is due. A write that fails costs the popup its list, nothing else. Called
    /// at every read, with how its rows differ from the last read's (`changes`): the running
    /// rows are followed by the rows that changed.
    pub fn publish(
        &mut self,
        home: &Path,
        rows: &[Arc<Entry>],
        changes: &Changes,
        failed: &[Id],
        browser: impl FnOnce() -> bool,
    ) {
        match changes.at() {
            Some(at) => {
                for &i in at {
                    if rows.get(i).is_some_and(|r| r.state == State::Running) {
                        self.running_at.insert(i);
                    } else {
                        self.running_at.remove(&i);
                    }
                }
            }
            None => {
                self.running_at = rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.state == State::Running)
                    .map(|(i, _)| i)
                    .collect();
            }
        }
        let going = running_of(self.running_at.iter().filter_map(|&i| rows.get(i)));
        if recent_may_change(rows, changes) {
            self.recent = recent(rows);
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        if let Some(wanted) = self.next_with(rows, going, failed, Instant::now(), now_ms) {
            if browser() {
                let _ = write(home, &wanted);
            }
        }
    }
}

/// Writes `published` whole: into a file beside it, then moved over, so a reader never finds
/// half of it.
fn write(home: &Path, published: &Published) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(&edm_core::saved::Stamped {
        format: FORMAT,
        body: published,
    })
    .map_err(std::io::Error::other)?;
    let path = file(home);
    let beside = path.with_extension("json.new");
    std::fs::write(&beside, bytes)?;
    std::fs::rename(&beside, &path)
}

/// What the popup is told (`downloads.list`): the file as written, with no download running
/// where it is older than [`STALE_MS`] or no EDM runs. Nothing written reads as nothing running.
pub fn read(home: &Path, now_ms: u64, app_running: bool) -> Published {
    let mut published: Published = std::fs::read(file(home))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    if !app_running || now_ms.saturating_sub(published.at_ms) > STALE_MS {
        published.downloads.clear();
        published.running = 0;
    }
    // Nothing can be done to them with no EDM to do it; the file, written when nothing more ran,
    // still says them however old it is.
    if !app_running {
        published.recent.clear();
    }
    published
}

/// The start of the name of an inbox file that carries one of the popup's asks
/// ([`ask_window`]).
pub const ASKED_PREFIX: &str = "browser.";

/// Leaves the popup's `act` in the running EDM's inbox for its window to carry out
/// ([`asked_of_window`]): what the host answers `downloads.act` with. Why not, in words for the
/// popup, where it is not one this EDM knows or the inbox cannot be written.
pub fn ask_window(home: &Path, act: &Act) -> Result<(), String> {
    if !act.is_known() {
        return Err(format!("EDM does not know \"{}\"", act.does));
    }
    let name = match act.id {
        Some(id) => format!("{ASKED_PREFIX}{}.{id}", act.does),
        None => format!("{ASKED_PREFIX}{}", act.does),
    };
    // The bell is the caller's to ring (`host::World::ring`).
    let dir = crate::host::inbox(home);
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join(name), b""))
        .map_err(|e| format!("EDM could not be asked: {e}"))
}

/// The ask an inbox file's name carries ([`ask_window`]), or `None` for any other file.
pub fn asked_by(name: &str) -> Option<Act> {
    let rest = name.strip_prefix(ASKED_PREFIX)?;
    let (does, id) = match rest.split_once('.') {
        Some((does, id)) => (does, Some(id.parse().ok()?)),
        None => (rest, None),
    };
    let act = Act {
        does: does.to_string(),
        id,
    };
    act.is_known().then_some(act)
}

/// Whether any of the popup's asks waits in `home`'s inbox, for the inbox's watcher to wake the
/// window by.
pub fn window_asked(home: &Path) -> bool {
    std::fs::read_dir(crate::host::inbox(home)).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(ASKED_PREFIX))
        })
    })
}

/// The popup's asks waiting in `home`'s inbox, each taken once, in the order left. A file that is
/// not one is left where it is.
pub fn asked_of_window(home: &Path) -> Vec<Act> {
    let Ok(entries) = std::fs::read_dir(crate::host::inbox(home)) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, Act)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(ASKED_PREFIX) {
            continue;
        }
        let at = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        // Taken before it is done, so an ask that upsets EDM is not done again at every look.
        if std::fs::remove_file(&path).is_err() {
            continue;
        }
        if let Some(act) = asked_by(name) {
            found.push((at, act));
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, act)| act).collect()
}

/// What the window knows of a row, as far as the popup's asks need it: whether its file is gone.
struct OnDisk;

impl super::commands::Looks for OnDisk {
    fn retrying(&self, _: Id) -> bool {
        false
    }
    fn gone(&self, row: &Entry) -> bool {
        super::rules::file_gone(row, &Default::default())
    }
    fn plays(&self, _: &Entry) -> bool {
        false
    }
}

/// The window's own command a popup's ask is, with the row it is for among `rows` (none for
/// Delete Completed): `None` where the row is gone or the command does not apply to it now, as
/// the window greys it out (`commands::applies`), so it is not done.
pub fn command_for(act: &Act, rows: &[Arc<Entry>]) -> Option<(Command, Option<Entry>)> {
    let command = match act.does.as_str() {
        act_does::OPEN => Command::OpenFile,
        act_does::FOLDER => Command::OpenFolder,
        act_does::RESUME => Command::DownloadNow,
        act_does::STOP => Command::StopDownload,
        act_does::CLEAR_FINISHED => Command::DeleteCompleted,
        _ => return None,
    };
    let row = match act.id {
        Some(id) => Some(rows.iter().find(|r| r.id == id)?),
        None => None,
    };
    let chosen: Vec<&Entry> = row.iter().map(|r| r.as_ref()).collect();
    super::commands::applies(command, &chosen, rows, false, &OnDisk)
        .then(|| (command, row.map(|r| Entry::clone(r))))
}

#[cfg(test)]
mod tests;
