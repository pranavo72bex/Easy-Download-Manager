// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the app is fetching, for the browser's toolbar popup (gaps row U18).
//!
//! The browser's native host is a process of its own, started per message, and the control port
//! is open only where a person asked for it, so the host has no way to ask the window. The window
//! writes what is running into `going.json` in dm's home as it reads the queue, at most once a
//! second while anything runs and once when it stops, and the host reads the file when the popup
//! asks (`downloads.list`). What it wrote is stamped, so a dm that quit mid-download is not taken
//! for one still fetching.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_core::{Entry, Id, State};
use dm_protocol::{Going, LastFailure};
use serde::{Deserialize, Serialize};

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
    /// The last download that failed since this dm started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<LastFailure>,
}

/// Where it is kept.
pub fn file(home: &Path) -> PathBuf {
    home.join("going.json")
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
    let mut going: Vec<Going> = rows
        .iter()
        .filter(|r| r.state == State::Running)
        .map(|r| Going {
            name: name_of(r),
            percent: r
                .progress
                .total
                .filter(|&t| t > 0)
                .map(|t| (r.progress.downloaded.min(t).saturating_mul(100) / t) as u8),
            speed: r.progress.speed.max(0.0) as u64,
        })
        .collect();
    let count = going.len();
    going.sort_by(|a, b| b.speed.cmp(&a.speed).then_with(|| a.name.cmp(&b.name)));
    going.truncate(MOST);
    (going, count)
}

/// Writes `going.json` as the queue is read, no more often than it needs.
#[derive(Debug, Default)]
pub struct Publisher {
    /// What was written last, and when.
    written: Option<(Published, Instant)>,
    /// The last failure seen.
    failed: Option<LastFailure>,
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
        let (downloads, running) = running(rows);
        let wanted = Published {
            at_ms: now_ms,
            downloads,
            running,
            failed: self.failed.clone(),
        };
        let due = match &self.written {
            None => true,
            Some((was, at)) => {
                let new_failure = was.failed != wanted.failed;
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
    /// talked to this dm: nobody else reads the file, so it costs nobody else a write. Asked only
    /// when a write is due. A write that fails costs the popup its list, nothing else.
    pub fn publish(
        &mut self,
        home: &Path,
        rows: &[Arc<Entry>],
        failed: &[Id],
        browser: impl FnOnce() -> bool,
    ) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        if let Some(wanted) = self.next(rows, failed, Instant::now(), now_ms) {
            if browser() {
                let _ = write(home, &wanted);
            }
        }
    }
}

/// Writes `published` whole: into a file beside it, then moved over, so a reader never finds
/// half of it.
fn write(home: &Path, published: &Published) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(published).map_err(std::io::Error::other)?;
    let path = file(home);
    let beside = path.with_extension("json.new");
    std::fs::write(&beside, bytes)?;
    std::fs::rename(&beside, &path)
}

/// What the popup is told (`downloads.list`): the file as written, with no download running
/// where it is older than [`STALE_MS`] or no dm runs. Nothing written reads as nothing running.
pub fn read(home: &Path, now_ms: u64, app_running: bool) -> Published {
    let mut published: Published = std::fs::read(file(home))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    if !app_running || now_ms.saturating_sub(published.at_ms) > STALE_MS {
        published.downloads.clear();
        published.running = 0;
    }
    published
}

#[cfg(test)]
mod tests;
