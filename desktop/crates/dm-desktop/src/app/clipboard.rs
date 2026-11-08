// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl DmApp {
    /// Turns the clipboard watch on or off.
    pub fn set_clipboard_watch(&mut self, on: bool) {
        self.clipboard.on = on;
        if !on {
            self.clipboard.offered = None;
            self.clipboard.offered_all.clear();
            return;
        }
        // Seeded with whatever is there now, so turning it on does not offer
        // a link copied long ago for another reason.
        self.clipboard.last = self.backend.platform.clipboard();
        self.clipboard.polled = Some(Instant::now());
    }

    /// Looks at the clipboard once, now.
    pub fn check_clipboard(&mut self) {
        if !self.clipboard.on {
            return;
        }
        self.clipboard.polled = Some(Instant::now());
        let text = self.backend.platform.clipboard();
        if text == self.clipboard.last {
            return;
        }
        self.clipboard.last.clone_from(&text);
        // What it offers is the application's rule (`dm_shell::app::links`).
        let links = dm_shell::app::links::clipboard_offer(
            text.as_deref(),
            &self.clipboard.dismissed,
            &self.rows,
        );
        // Taken at once where the person asked for that: one link's
        // Download details, or the list of several, with no bar first.
        if self.applied.start_copied && !links.is_empty() {
            self.clipboard.dismissed.extend(links.iter().cloned());
            self.clipboard.offered = None;
            self.clipboard.offered_all.clear();
            if links.len() == 1 {
                self.dialogs.open_file_info(links);
            } else {
                self.queued.push(Command::AddFromClipboard);
            }
            return;
        }
        self.clipboard.offered = links.first().cloned();
        self.clipboard.offered_all = links;
    }

    /// The address on the clipboard that dm offers to download, if any.
    #[must_use]
    pub fn clipboard_offer(&self) -> Option<&str> {
        self.clipboard.offered.as_deref()
    }
}
