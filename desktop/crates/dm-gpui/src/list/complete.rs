// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The downloads' progress windows, and "Download complete" when they finish.

use gpui::Context;

use super::Downloads;

impl Downloads {
    /// Whether a person closed the progress window of `id`.
    pub(crate) fn progress_closed_has(&self, id: dm_core::Id) -> bool {
        self.progress_closed.contains(&id)
    }

    /// Closes the progress window of `id`, where one is open.
    pub(super) fn close_progress(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        if let Some(handle) = self.progress_windows.remove(&id) {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// Progress windows a person closed: forgotten, and not opened again by
    /// themselves.
    pub(crate) fn prune_progress(&mut self, cx: &mut Context<Self>) {
        let open = cx.windows();
        let gone: Vec<dm_core::Id> = self
            .progress_windows
            .iter()
            .filter(|(_, handle)| !open.contains(handle))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.progress_windows.remove(&id);
            self.progress_closed.insert(id);
        }
        if self.complete_window.is_some_and(|w| !open.contains(&w)) {
            self.complete_window = None;
            self.complete.clear();
        }
    }

    /// Downloads that finished: "Download complete" says so, where that is asked for, in the
    /// progress window of one that has it open, else in a window listing the rest; a progress
    /// window closes where it is not.
    pub(crate) fn finished(&mut self, ids: &[dm_core::Id], cx: &mut Context<Self>) {
        let show = self.backend.settings().show_complete;
        for id in ids {
            // Its progress window open: that window becomes Download complete itself, with
            // Open and Open folder, rather than a second window opening over it (gaps row U11,
            // B63).
            if show && self.progress_windows.contains_key(id) {
                continue;
            }
            self.close_progress(*id, cx);
            if show {
                self.complete.insert(*id);
            }
        }
        // Not over "Exit dm?" while it asks, with Open folder where Exit was about to be
        // pressed: it opens once the question is answered (`ask_quit`).
        if show && !self.complete.is_empty() && self.complete_window.is_none() && !self.exit_asked {
            let me = cx.entity().downgrade();
            cx.defer(move |cx| {
                let handle = crate::complete::open(me.clone(), cx);
                let _ = me.update(cx, |view, _| view.complete_window = handle);
            });
        }
    }

    /// "Download complete" closed: a ticked "Don't show this window again"
    /// kept, and said.
    pub fn complete_closed(&mut self, never: bool, cx: &mut Context<Self>) {
        self.complete.clear();
        self.complete_window = None;
        if never {
            let mut wanted = self.backend.settings();
            wanted.show_complete = false;
            match self.backend.set_settings(wanted) {
                Ok(_) => self.say(
                    dm_desktop_base::i18n::t(
                        "This window won't open again. Turn it on in Options › General.",
                    )
                    .into(),
                ),
                Err(e) => self.say(e),
            }
        }
        cx.notify();
    }

    /// A file acted on in "Download complete": it leaves the list, and the
    /// window with the last.
    pub fn complete_done(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        self.complete.remove(&id);
        if self.complete.is_empty() {
            if let Some(handle) = self.complete_window.take() {
                // After the update that asked: its own button's click (Open,
                // Open folder) is still being handled inside that window,
                // which GPUI will not update again, and it stayed, empty.
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                });
            }
        }
        cx.notify();
    }
}
