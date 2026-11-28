// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! macOS: the launch agent `auto-launch` writes.

/// The program the login item `auto-launch` wrote starts: on macOS its
/// launch agent's plist. `None` when there is none or it cannot be read,
/// which is not taken as a reason to rewrite it.
pub fn login_item_program() -> Option<std::path::PathBuf> {
    let plist = edm_core::folders::launch_agents()?.join("EDM.plist");
    let text = std::fs::read_to_string(plist).ok()?;
    crate::autostart::plist_program(&text)
}

/// Whether the person turned EDM's login item off outside EDM: macOS has no
/// such switch.
pub fn login_item_turned_off_outside() -> bool {
    false
}
