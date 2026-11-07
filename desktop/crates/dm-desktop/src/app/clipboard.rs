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
        let fresh = |url: &String| {
            !self.clipboard.dismissed.contains(url) && !self.rows.iter().any(|r| &r.url == url)
        };
        // Text that is links and nothing else, one or several; a sentence
        // with a link in it is not an offer.
        let words: Vec<&str> = text.as_deref().unwrap_or("").split_whitespace().collect();
        let links: Vec<String> =
            if !words.is_empty() && words.iter().all(|w| looks_downloadable(w).is_some()) {
                let mut links: Vec<String> = Vec::new();
                for w in words {
                    if !links.iter().any(|l| l == w) {
                        links.push(w.to_string());
                    }
                }
                links.into_iter().filter(fresh).collect()
            } else {
                text.as_deref()
                    .and_then(looks_downloadable)
                    .filter(fresh)
                    .into_iter()
                    .collect()
            };
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
