// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Linux (and any system but macOS and Windows): the login entry is
//! `autostart`'s, not `auto-launch`'s.

/// The program the login item `auto-launch` wrote starts: none here.
pub fn login_item_program() -> Option<std::path::PathBuf> {
    None
}

/// Whether the person turned dm's login item off outside dm: there is no
/// such switch here.
pub fn login_item_turned_off_outside() -> bool {
    false
}
