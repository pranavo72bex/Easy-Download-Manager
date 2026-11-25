// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Which window is in front: Properties, the holder of a dialog, the main window; and files opened
//! and shown.

use super::*;

impl Downloads {
    /// Where an opener says what went wrong, from its thread.
    pub(crate) fn opener_said(&self) -> std::sync::Arc<std::sync::Mutex<Option<String>>> {
        self.threads.opener_said.clone()
    }

    /// Opens a file with its usual app, off the window's thread.
    pub fn open_path(&self, path: &str) {
        let said = self.threads.opener_said.clone();
        self.backend
            .platform
            .open_later(path, Box::new(move |why| *dm_core::lock(&said) = Some(why)));
    }

    /// Shows a file in the file manager, off the window's thread.
    pub fn reveal_path(&self, path: &str) {
        let said = self.threads.opener_said.clone();
        self.backend
            .platform
            .reveal_later(path, Box::new(move |why| *dm_core::lock(&said) = Some(why)));
    }

    /// Open with…: the app chosen in the system's own panel, the window
    /// going on meanwhile; then `after`, chosen or not.
    pub fn open_path_with(
        &mut self,
        path: &str,
        after: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        // A phone has no app picker of dm's: its own chooser lists the apps (gaps row N22).
        if crate::platform::ON_A_PHONE {
            if let Err(e) = self
                .backend
                .platform
                .open_with(path, std::path::Path::new(""))
            {
                self.say(e);
            }
            after(self, cx);
            cx.notify();
            return;
        }
        let chosen = crate::platform::services::ask(crate::platform::services::Asked::app());
        let path = path.to_string();
        cx.spawn(async move |this, cx| {
            let app = chosen.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(app) = app {
                    if let Err(e) = this.backend.platform.open_with(&path, &app) {
                        this.say(e);
                    }
                }
                after(this, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// What an opener said went wrong, from its thread.
    pub(crate) fn say_opener(&mut self) {
        let said = dm_core::lock(&self.threads.opener_said).take();
        if let Some(why) = said {
            self.say(why);
        }
    }

    /// An archive's password asked for, where Extract here or Extract to found none of the
    /// saved ones opens it (gaps row C2b).
    pub(crate) fn ask_archive_password(&mut self, cx: &mut gpui::Context<Self>) {
        let asked = dm_core::lock(&self.threads.password_asked).take();
        if let Some(asked) = asked {
            crate::dialogs::password::open(cx.entity().downgrade(), asked, cx);
        }
    }

    /// Whether a dialog stands in front of the list: a question, or a
    /// dialog's window. Progress windows and "Download complete" stand
    /// beside it, not in front.
    pub(crate) fn dialog_in_front(&self, cx: &gpui::App) -> bool {
        self.questions.sheet_open
            || crate::window::screen::Screens::all(cx)
                .iter()
                .any(|(w, _, dialog)| {
                    *dialog
                        && !self.windows.progress_windows.values().any(|p| p == w)
                        && self.windows.complete_window.as_ref() != Some(w)
                })
    }

    /// Row `id`'s File properties window, where it is still open.
    pub(crate) fn properties_window(
        &self,
        id: dm_core::Id,
        cx: &gpui::App,
    ) -> Option<gpui::AnyWindowHandle> {
        let open = cx.windows();
        self.windows
            .properties_windows
            .get(&id)
            .filter(|w| open.contains(w))
            .copied()
    }

    /// How many File properties windows are open.
    pub(crate) fn properties_open(&self, cx: &gpui::App) -> usize {
        let open = cx.windows();
        self.windows
            .properties_windows
            .values()
            .filter(|w| open.contains(w))
            .count()
    }

    /// Row `id`'s File properties window opened; those since closed are
    /// forgotten.
    pub(crate) fn properties_opened(&mut self, id: dm_core::Id, window: gpui::AnyWindowHandle) {
        self.windows.properties_windows.insert(id, window);
    }

    /// The dialog that keeps the clicks to itself and so holds this window
    /// (B42): Options, while open; on Linux an open question too, which GPUI
    /// holds a parent for only as a `Dialog`, which a question is not there
    /// (`question::KIND`). Elsewhere a question is a sheet or a `Dialog`, and
    /// the system holds the window for it.
    pub(crate) fn holder(&self, cx: &gpui::App) -> Option<gpui::AnyWindowHandle> {
        let open = cx.windows();
        let question = self
            .questions
            .question_window
            .filter(|_| cfg!(not(any(target_os = "macos", windows))));
        [self.windows.options_window, question]
            .into_iter()
            .flatten()
            .find(|w| open.contains(w))
    }

    /// Where a dialog holds the window, it comes forward; true if one does.
    pub(crate) fn bring_holder(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(holder) = self.holder(cx) else {
            return false;
        };
        cx.defer(move |cx| {
            let _ = holder.update(cx, |_, window, _| window.activate_window());
        });
        true
    }

    /// The main window.
    pub(crate) fn main_window(&self) -> gpui::AnyWindowHandle {
        self.questions.main
    }

    /// The main window back in front, from the tray or wherever it is, after
    /// the update that asked.
    pub(crate) fn show_main(&mut self, cx: &mut Context<Self>) {
        self.system.hidden = false;
        let main = self.questions.main;
        cx.defer(move |cx| {
            let _ = main.update(cx, |_, window, _| {
                crate::platform::syswindow::show(window);
                window.activate_window();
            });
            // A dialog open over it stays in front, as a person left it:
            // brought forward after the main window, not buried under it.
            if let Some((dialog, _, _)) = crate::window::screen::Screens::all(cx)
                .into_iter()
                .find(|(_, _, dialog)| *dialog)
            {
                let _ = dialog.update(cx, |_, window, _| window.activate_window());
            }
            cx.activate(true);
        });
    }

    /// Whether a second launch, the browser or the Dock asked for the
    /// window since the last look.
    pub(crate) fn take_show_asked(&self) -> bool {
        self.port
            .show_asked
            .swap(false, std::sync::atomic::Ordering::Relaxed)
    }
}
