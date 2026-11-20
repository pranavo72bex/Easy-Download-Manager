// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A row of the list as the phone draws it: the engine's `Entry`, in types Kotlin has.

use dm_core::{Entry, State, Swarm};

/// Where a download has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    /// Waiting for its turn.
    Queued,
    /// Being downloaded now.
    Running,
    /// Stopped by the person, to be resumed.
    Paused,
    /// Finished and in its place.
    Completed,
    /// Stopped by a failure, said in `Row::failure`.
    Failed,
    /// Stopped by the person for good.
    Cancelled,
}

/// A torrent's swarm now: the details screen's Peers line (gaps row N8).
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentSwarm {
    /// Peers connected now.
    pub connected: u32,
    /// Peers known this run.
    pub known: u32,
    /// Bytes sent to peers this run.
    pub uploaded: u64,
    /// Bytes a second sent to peers now.
    pub upload_speed: f64,
}

/// One download, as the list and its details screen show it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The row's number, never reused.
    pub id: u64,
    /// The address it is downloaded from.
    pub url: String,
    /// The file's name, or the address's last part until the server has named it.
    pub name: String,
    /// Where it has got to.
    pub state: RowState,
    /// Why it failed, in dm's words, where it did.
    pub failure: Option<String>,
    /// Bytes on disk so far.
    pub downloaded: u64,
    /// The file's size, where known.
    pub total: Option<u64>,
    /// `total` is an estimate (a stream's), shown as "≈".
    pub estimated: bool,
    /// Bytes a second, smoothed.
    pub speed: f64,
    /// Seconds left, where there is something to base it on.
    pub eta_seconds: Option<u64>,
    /// Where the finished file was written.
    pub path: String,
    /// What it was sorted as, once known (`videos`, `audio`, …, as dm saves it).
    pub category: Option<String>,
    /// A magnet or a `.torrent`.
    pub torrent: bool,
    /// The swarm, for a torrent the engine is running.
    pub swarm: Option<TorrentSwarm>,
}

impl Row {
    /// `entry` as the phone shows it, with `swarm` where it is a running torrent.
    pub(crate) fn of(entry: &Entry, swarm: Option<&Swarm>) -> Row {
        let (state, failure) = match &entry.state {
            State::Queued => (RowState::Queued, None),
            State::Running => (RowState::Running, None),
            State::Paused => (RowState::Paused, None),
            State::Completed => (RowState::Completed, None),
            State::Failed(why) => (RowState::Failed, Some(why.clone())),
            State::Cancelled => (RowState::Cancelled, None),
        };
        Row {
            id: entry.id,
            url: entry.url.clone(),
            name: name_of(entry),
            state,
            failure,
            downloaded: entry.progress.downloaded,
            total: entry.progress.total,
            estimated: entry.progress.estimated,
            speed: entry.progress.speed,
            eta_seconds: entry.progress.eta.map(|eta| eta.as_secs()),
            path: entry.path.clone(),
            category: entry.category.and_then(|c| {
                serde_json::to_value(c)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
            }),
            torrent: dm_core::is_torrent(&entry.url),
            swarm: swarm.map(|s| TorrentSwarm {
                connected: s.connected,
                known: s.known,
                uploaded: s.uploaded,
                upload_speed: s.upload_speed,
            }),
        }
    }
}

/// The file's name, else the address's last part, else the address.
fn name_of(entry: &Entry) -> String {
    if !entry.filename.is_empty() {
        return entry.filename.clone();
    }
    let path = entry.url.split(['?', '#']).next().unwrap_or(&entry.url);
    path.rsplit('/')
        .find(|part| !part.is_empty() && !part.contains(':'))
        .map_or_else(|| entry.url.clone(), str::to_owned)
}
