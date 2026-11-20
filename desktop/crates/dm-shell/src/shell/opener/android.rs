// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Android (gaps row N2): a library has no program to open a file with. The phone opens a file
//! and shows its folder through an intent, which only the app can send: `dm-android`'s
//! `Platform` asks the app (`AndroidSystem::open`, `reveal`). Asked here, each says so.

use std::path::Path;

/// What is said when a file is asked to be opened from the engine rather than the app.
const THE_APPS: &str = "on a phone the file is opened by dm's app, not by its engine";

/// Not here: the app shows a file's folder.
pub(crate) fn reveal(_: &Path, _: &str) -> Result<(), String> {
    Err(THE_APPS.into())
}

/// Not here: the app opens a file.
pub(crate) fn open(_: &Path, _: &str) -> Result<(), String> {
    Err(THE_APPS.into())
}

/// No short forms here.
pub(crate) fn short_form(_: &str) -> Option<String> {
    None
}
