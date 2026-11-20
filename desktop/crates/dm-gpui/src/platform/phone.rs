// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Android (gaps row N17): the window's notifier and clipboard, through what the app handed
//! in (`dm_desktop_base::phone`), in `notify.rs`'s and `services.rs`'s place. A notification
//! is the phone's own, its click and Retry heard by the app.

use dm_desktop_base::quiet::Note;

/// Nothing to claim: the app's notifications are posted under its own name.
pub fn claim() {}

/// Says `note` in the phone's notification shade.
pub fn say(note: Note) {
    dm_desktop_base::phone::notify(note);
}

/// The phone's clipboard text.
pub fn clipboard() -> Option<String> {
    dm_desktop_base::phone::clipboard()
}

/// Puts `text` on the phone's clipboard.
pub fn set_clipboard(text: &str) {
    dm_desktop_base::phone::set_clipboard(text);
}
