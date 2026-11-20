// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Whether something shows dm's tray item (`tray::host_showing`, `tray::keep_registered`): on
//! Linux a StatusNotifier watcher on the session bus (`linux.rs`), which may come later than
//! dm or never; elsewhere the system always has a tray.

#[cfg(target_os = "linux")]
mod linux;

/// Whether something is showing tray icons now.
pub(crate) fn host_showing() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::shown(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Keeps dm's tray item registered with whatever StatusNotifier watcher there is, for as long
/// as dm runs. Nothing to do elsewhere.
pub(crate) fn keep_registered() {
    #[cfg(target_os = "linux")]
    linux::keep_registered();
}
