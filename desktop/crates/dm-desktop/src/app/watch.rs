// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The watched folder (Options › Downloads): a link saved where dm is told
//! to look is added, then the file is moved into a `done` folder inside it,
//! so it is not added twice and nothing is deleted.
//!
//! Files are read as a drop is (`dropped_links`): the first 64 KB of a text
//! kind, or a `.torrent` by its path. A file is never run, and one still
//! being written is left for the next look.

use super::*;

/// How often the folder is read (`dm_shell::app::watch`).
pub(super) use dm_shell::app::watch::WATCH_EVERY;

impl DmApp {
    /// Reads the watched folder when a look is due (the application reads it
    /// and moves what it read aside) and adds each file's links as the
    /// window's own adds go in.
    pub(super) fn take_watched(&mut self) {
        let folder = self.applied.watch_folder.clone();
        for links in self.watched.look(&folder, &self.rows, &self.backend) {
            let _ = self.add(&links, Default::default());
        }
    }
}
