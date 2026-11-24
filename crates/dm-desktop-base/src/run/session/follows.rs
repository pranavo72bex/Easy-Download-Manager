// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Playlists, channels' uploads and feeds followed (gaps row W10), in the tick every driver
//! runs: each one whose time has come read again in the background, looked at once a minute,
//! and each reading that has come back handed to the driver ([`Effect::Followed`]), which adds
//! what is new its own way and says so (the window in the language in force; with no window,
//! in English). What is new, and what is kept, is the application's (`dm_shell::app::follow`).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dm_core::youtube::Playlist;
use dm_shell::app::backend::Backend;
use dm_shell::app::follow::{self, Follow};

use crate::i18n::tf;

use super::{Effect, Session};

/// How often the followed ones are looked at for one that is due.
const FOLLOWS_EVERY: Duration = Duration::from_secs(60);

/// A reading that has come back: the followed one's address, and its items or why not.
type Reading = (String, Result<Playlist, String>);

/// What the tick keeps of the followed ones between ticks.
#[derive(Debug, Default)]
pub(super) struct Follows {
    /// The readings that have come back, in the order they did, for the next tick.
    results: Arc<Mutex<Vec<Reading>>>,
    /// When the followed ones were last looked at for one that is due.
    checked: Option<Instant>,
}

impl Session {
    /// Reads `follow` now, in the background, with the login Options › Sign-ins keeps for its
    /// site: Check now, and each one due. What it brought comes with a later tick
    /// ([`Effect::Followed`]), the driver woken for it.
    pub fn check_follow(&self, backend: &Backend, follow: &Follow) {
        let results = Arc::clone(&self.follows.results);
        let address = follow.address.clone();
        let reading = backend.settings().with_login(&address);
        follow::read_later(backend, reading, move |read| {
            dm_core::lock(&results).push((address, read));
            dm_shell::app::changes::changed();
        });
    }

    /// The readings that came back since the last tick, each for the driver to take in, and,
    /// once a minute, each followed one whose time has come read again, its reading stamped in
    /// the settings first.
    pub(super) fn follows(&mut self, backend: &Backend, effects: &mut Vec<Effect>) {
        let done: Vec<Reading> = std::mem::take(&mut *dm_core::lock(&self.follows.results));
        effects.extend(
            done.into_iter()
                .map(|(address, read)| Effect::Followed { address, read }),
        );
        let now = Instant::now();
        if self
            .follows
            .checked
            .is_some_and(|at| now.duration_since(at) < FOLLOWS_EVERY)
        {
            return;
        }
        self.follows.checked = Some(now);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let Some((due, wanted)) = follow::due_now(&backend.settings(), epoch) else {
            return;
        };
        if let Err(e) = backend.set_settings(wanted) {
            // Still read, and not again for a minute; but said, since an unsaved last reading
            // reads it again at every launch.
            effects.push(Effect::Problem(tf(
                "what is followed couldn't be saved: {}",
                &[&e],
            )));
        }
        for one in &due {
            self.check_follow(backend, one);
        }
    }
}

#[cfg(test)]
mod tests;
