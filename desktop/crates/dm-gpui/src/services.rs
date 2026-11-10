// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the GPUI window supplies to the desktop's services
//! (`dm_desktop_base::desktop::WindowServices`): the clipboard (arboard),
//! the system's own open and save panels for the application's choosers
//! (rfd: on macOS the NSOpenPanel and NSSavePanel), and notifications
//! (`notify.rs`). The window's own dialogs use GPUI's path prompts.

use std::path::PathBuf;

use dm_desktop_base::desktop::WindowServices;
use dm_desktop_base::quiet::Note;

/// The GPUI window's services.
pub struct GpuiServices;

impl WindowServices for GpuiServices {
    fn clipboard(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn choose_app(&self) -> Option<PathBuf> {
        let dialog = rfd::FileDialog::new().set_title("Open with");
        #[cfg(target_os = "macos")]
        let dialog = dialog
            .set_directory("/Applications")
            .add_filter("Applications", &["app"]);
        #[cfg(target_os = "windows")]
        let dialog = dialog.add_filter("Apps", &["exe"]);
        dialog.pick_file()
    }

    fn choose_folder(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    fn choose_file(&self, save: bool, suggested: &str) -> Option<PathBuf> {
        // What is asked for decides the filter: a settings file is JSON.
        let dialog = if suggested.ends_with(".json") {
            rfd::FileDialog::new().add_filter("dm settings", &["json"])
        } else {
            rfd::FileDialog::new().add_filter("Download lists", &["dmlist", "txt"])
        };
        if save {
            dialog.set_file_name(suggested).save_file()
        } else {
            dialog.pick_file()
        }
    }

    /// The system's own notification centre (`crate::notify`).
    fn say(&self, note: Note) {
        #[cfg(not(windows))]
        crate::notify::say(note);
        #[cfg(windows)]
        let _ = note;
    }
}
