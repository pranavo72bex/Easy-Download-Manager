// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Playlists, channels' uploads and feeds followed, with no window (gaps row W10): the
//! application's tick reads each again when its time comes (`Session::check_follow`), and what
//! a reading brought is taken in here as the window takes it in, said in English.

use edm_core::youtube::Playlist;
use edm_shell::app::follow;

use super::{english, Headless};

impl Headless {
    /// What a reading of the one at `address` brought: the items it did not list before added
    /// for later to its queue, said; what it has seen kept.
    pub(super) fn take_followed(&mut self, address: &str, read: Result<Playlist, String>) {
        let settings = self.backend.settings();
        // No longer followed: what it brought is let go.
        let Some(kept) = settings.follows.iter().find(|f| f.address == address) else {
            return;
        };
        let name = kept.name().to_string();
        let list = match read {
            Ok(list) => list,
            Err(e) => {
                self.problems
                    .push(format!("the followed {name} couldn't be read: {e}"));
                return;
            }
        };
        let new = follow::new_items(kept, &list, &self.rows);
        if new.is_empty() {
            self.said = english("{} has nothing new.", 1, &[&name]);
        } else {
            let queue = kept.queue_or_main().to_string();
            match self.add_items(new) {
                Ok(n) => {
                    self.said = english(
                        "Added {} download to {}.||Added {} downloads to {}.",
                        n as u64,
                        &[&n, &queue],
                    );
                }
                Err(e) => self
                    .problems
                    .push(format!("the items {name} gained could not be added: {e}")),
            }
        }
        let settings = self.backend.settings();
        if let Some(wanted) = follow::seen_saved(&settings, address, &list, &self.rows) {
            if let Err(e) = self.backend.set_settings(wanted) {
                self.problems
                    .push(format!("what is followed could not be saved: {e}"));
            }
        }
    }
}
