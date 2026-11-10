// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the GPUI window supplies to the desktop's services
//! (`dm_desktop_base::desktop::WindowServices`). The shell supplies none
//! yet: each is the system's own in its slice (the clipboard and the
//! choosers through GPUI's, macOS's open and save panels among them, in G7
//! slices 3 and 4; notifications in slice 8), so the shell answers as a
//! person who cancelled would.

use std::path::PathBuf;

use dm_desktop_base::desktop::WindowServices;
use dm_desktop_base::quiet::Note;

/// The GPUI window's services: none yet (see the module).
pub struct GpuiServices;

impl WindowServices for GpuiServices {
    fn clipboard(&self) -> Option<String> {
        None
    }

    fn choose_app(&self) -> Option<PathBuf> {
        None
    }

    fn choose_folder(&self) -> Option<PathBuf> {
        None
    }

    fn choose_file(&self, _save: bool, _suggested: &str) -> Option<PathBuf> {
        None
    }

    fn say(&self, note: Note) {
        eprintln!("dm-gpui: {}: {}", note.title, note.body);
    }
}
