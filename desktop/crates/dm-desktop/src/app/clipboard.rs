// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl DmApp {
    /// Turns the clipboard watch on or off (`links::Clipboard::turn`).
    pub fn set_clipboard_watch(&mut self, on: bool) {
        let platform = &self.backend.platform;
        self.clipboard.turn(on, || platform.clipboard());
    }

    /// Looks at the clipboard once, now: what it offers is the
    /// application's (`links::Clipboard::look`); the window opens what is
    /// taken at once.
    pub fn check_clipboard(&mut self) {
        if !self.clipboard.on {
            return;
        }
        let text = self.backend.platform.clipboard();
        let looked = self
            .clipboard
            .look(text, &self.rows, self.applied.start_copied);
        if let dm_shell::app::links::Looked::Take(links) = looked {
            if links.len() == 1 {
                self.dialogs.open_file_info(links);
            } else {
                self.queued.push(Command::AddFromClipboard);
            }
        }
    }

    /// The address on the clipboard that dm offers to download, if any.
    #[must_use]
    pub fn clipboard_offer(&self) -> Option<&str> {
        self.clipboard.offered.as_deref()
    }
}
