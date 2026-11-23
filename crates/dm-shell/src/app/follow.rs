// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A playlist, a channel's uploads or a feed (RSS, a podcast) followed (gaps
//! row W10): kept in the settings beside the Grabber's saved searches, read
//! again every so many hours on the same minute's look, and only the videos
//! or files it did not list before (in neither the list nor what it has
//! seen) added for later to the queue chosen. Reading it is
//! `lookups::read_playlist_later`'s; adding and saying so are the
//! presentation's.

use std::sync::Arc;

use dm_core::youtube::Playlist;
use dm_core::Entry;
use serde::{Deserialize, Serialize};

use crate::app::search::{listed, remembered, sort_finds};
use crate::lists::{bare, MOST_HOURS};

/// A playlist or feed followed: where it is, how often it is read again,
/// the queue its new items go to, and what it has listed before.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Follow {
    /// The playlist's or feed's address, without a login.
    pub address: String,
    /// Its title, as it was read: what the side panel shows.
    pub title: String,
    /// Hours between readings; 0 is only by hand.
    pub every_hours: u64,
    /// The queue its new items are added to; empty is the main queue.
    pub queue: String,
    /// The video quality its new videos are taken at; empty is the
    /// setting's.
    pub quality: String,
    /// When it was last read, in seconds since 1970; 0 is never.
    pub last_run: u64,
    /// The items it has listed before, by their bare address: not added
    /// again after the list is cleared.
    pub seen: Vec<String>,
    /// A torrent feed's word filter (gaps row C10): only new items whose
    /// titles have every one of these words, in any case, are added; empty
    /// adds every new item.
    pub words: String,
}

impl Follow {
    /// Whether it is due to be read again at `now` (seconds since 1970), as
    /// a saved search is (`lists::Search::due`).
    pub fn due(&self, now: u64) -> bool {
        if self.every_hours == 0 {
            return false;
        }
        if self.last_run > now {
            return true;
        }
        now - self.last_run >= self.every_hours.min(MOST_HOURS) * 3600
    }

    /// What its side panel row is called: its title, else its address.
    pub fn name(&self) -> &str {
        if self.title.trim().is_empty() {
            &self.address
        } else {
            &self.title
        }
    }

    /// Whether an item called `title` passes its word filter: every word,
    /// in any case, somewhere in the title.
    pub fn wants(&self, title: &str) -> bool {
        let title = title.to_lowercase();
        self.words
            .split_whitespace()
            .all(|word| title.contains(&word.to_lowercase()))
    }

    /// The queue its items go to.
    pub fn queue_or_main(&self) -> &str {
        if self.queue.is_empty() {
            dm_core::MAIN_QUEUE
        } else {
            &self.queue
        }
    }
}

/// Whether `list` holds a torrent: a `.torrent` file's address or a magnet
/// link, as a torrent feed's items are (gaps row C10).
pub fn has_torrents(list: &Playlist) -> bool {
    list.entries.iter().any(|e| dm_core::is_torrent(&e.url()))
}

/// Every item's address in `list`, in its order.
fn items(list: &Playlist) -> Vec<String> {
    list.entries.iter().map(|e| e.url()).collect()
}

/// `list`, read at `address`, followed from `now` (seconds): read again every
/// `every_hours` hours, its new items to `queue` at `quality`. Every item it
/// lists now counts as seen, ticked or not: the dialog took those it wanted.
pub fn following(
    list: &Playlist,
    address: &str,
    every_hours: u64,
    queue: &str,
    quality: &str,
    now: u64,
) -> Follow {
    Follow {
        address: bare(address),
        title: list.title.clone(),
        every_hours: every_hours.min(MOST_HOURS),
        queue: queue.to_string(),
        quality: quality.to_string(),
        last_run: now,
        seen: remembered(&[], items(list).iter().map(|u| bare(u))),
        words: String::new(),
    }
}

/// The settings with `follow` kept, in place of one at the same address.
pub fn followed(settings: &crate::Settings, follow: Follow) -> crate::Settings {
    let mut wanted = settings.clone();
    match wanted
        .follows
        .iter_mut()
        .find(|f| f.address == follow.address)
    {
        Some(kept) => *kept = follow,
        None => wanted.follows.push(follow),
    }
    wanted
}

/// The settings with the one at `address` no longer followed.
pub fn unfollowed(settings: &crate::Settings, address: &str) -> crate::Settings {
    let mut wanted = settings.clone();
    wanted.follows.retain(|f| f.address != address);
    wanted
}

/// Those due to be read again at `epoch` (seconds), and the settings with
/// each one's last reading stamped, to be saved before they are read; `None`
/// when none is due.
pub fn due_now(settings: &crate::Settings, epoch: u64) -> Option<(Vec<Follow>, crate::Settings)> {
    let due: Vec<Follow> = settings
        .follows
        .iter()
        .filter(|f| f.due(epoch))
        .cloned()
        .collect();
    if due.is_empty() {
        return None;
    }
    let mut wanted = settings.clone();
    for follow in &mut wanted.follows {
        if due.iter().any(|d| d.address == follow.address) {
            follow.last_run = epoch;
        }
    }
    Some((due, wanted))
}

/// What a reading of `follow` brings in: each item it did not list before
/// (in neither the `rows`, cancelled ones too, nor what it has seen) whose
/// title passes its word filter, with the options it is added with: for
/// later, to its queue, at its quality.
pub fn new_items(
    follow: &Follow,
    list: &Playlist,
    rows: &[Arc<Entry>],
) -> Vec<(String, dm_core::AddOptions)> {
    let with = dm_core::AddOptions {
        later: true,
        queue: follow.queue_or_main().to_string(),
        quality: Some(follow.quality.clone()).filter(|q| !q.is_empty()),
        referrer: follow.address.clone(),
        ..Default::default()
    };
    let wanted: Vec<String> = list
        .entries
        .iter()
        .filter(|e| follow.wants(&e.title))
        .map(|e| e.url())
        .collect();
    sort_finds(&wanted, rows, &follow.seen)
        .new
        .into_iter()
        .map(|url| (url, with.clone()))
        .collect()
}

/// The settings with what the one at `address` has now seen added: each
/// item of `list` that the `rows` hold (one that could not be added is
/// tried again next time). `None` when it is no longer followed.
pub fn seen_saved(
    settings: &crate::Settings,
    address: &str,
    list: &Playlist,
    rows: &[Arc<Entry>],
) -> Option<crate::Settings> {
    let now = listed(rows);
    let more: Vec<String> = items(list)
        .iter()
        .map(|u| bare(u))
        .filter(|u| now.contains(u))
        .collect();
    let mut wanted = settings.clone();
    let follow = wanted.follows.iter_mut().find(|f| f.address == address)?;
    follow.seen = remembered(&follow.seen, more);
    if !list.title.trim().is_empty() {
        follow.title.clone_from(&list.title);
    }
    Some(wanted)
}

/// Reads the one at `address` in the background, as the Playlist dialog
/// reads it, and gives `done` its items, or why not.
pub fn read_later(
    backend: &crate::app::backend::Backend,
    address: String,
    done: impl FnOnce(Result<Playlist, String>) + Send + 'static,
) {
    crate::app::lookups::read_playlist_later(backend, address, done);
}

#[cfg(test)]
mod tests;
