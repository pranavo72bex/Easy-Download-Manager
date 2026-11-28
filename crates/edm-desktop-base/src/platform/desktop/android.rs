// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Android (gaps row N17): what `Desktop` asks of the shell on a desktop, asked of the phone.
//! A file is opened and its folder shown by an intent only the app can send, so each goes
//! through the services the app handed in (`phone`); the engine's own opener only says it
//! cannot (`edm_shell`'s `platform/opener/android.rs`). There is no login item.

use std::path::Path;

/// What is said when a login item is asked for on a phone.
const NO_LOGIN_ITEM: &str = "a phone starts no app at login: Android has no login item";

/// Opens `path` in the app the phone picks for it.
pub(super) fn open_file(path: &str) -> Result<(), String> {
    crate::phone::open(path)
}

/// Shows the folder `path` is in.
pub(super) fn reveal_in_folder(path: &str) -> Result<(), String> {
    crate::phone::reveal(path)
}

/// Open with…: the phone's own chooser of the apps that open the file (no app is picked
/// beforehand on a phone, gaps row N22).
pub(super) fn open_with(path: &str, _app: &Path) -> Result<(), String> {
    crate::phone::open_with(path)
}

/// The login item: none to remove, and one asked for refused in words.
pub(super) fn login_item(wanted: bool) -> Result<(), String> {
    if wanted {
        Err(NO_LOGIN_ITEM.into())
    } else {
        Ok(())
    }
}
