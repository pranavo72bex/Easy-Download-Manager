// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Mica, Windows 11's backdrop: the desktop's colours faintly through the
//! window's frame. Opt-in (Options › General, from dm's next start), as it
//! needs a see-through window, which an old GPU driver can draw black
//! (docs/decisions.md). Both windows, egui's and GPUI's, ask here.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this run's window was made see-through for Mica.
static IN_FORCE: AtomicBool = AtomicBool::new(false);

/// The first Windows build with `DWMWA_SYSTEMBACKDROP_TYPE`: 11 22H2.
pub const MICA_BUILD: u32 = 22621;

/// Whether this computer can draw Mica: Windows 11 22H2 or later.
pub fn possible() -> bool {
    #[cfg(windows)]
    {
        crate::autostart::registry::machine_string(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "CurrentBuildNumber",
        )
        .and_then(|b| b.trim().parse::<u32>().ok())
        .is_some_and(|build| build >= MICA_BUILD)
    }
    #[cfg(not(windows))]
    false
}

/// Puts Mica in force for this run where it is `wanted` and possible, and
/// says whether it is: the window is then to be made see-through, and its
/// frame drawn clear.
pub fn use_it(wanted: bool) -> bool {
    let on = wanted && possible();
    IN_FORCE.store(on, Ordering::Relaxed);
    on
}

/// Whether this run's window has Mica behind it.
pub fn in_force() -> bool {
    IN_FORCE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off where it is not wanted, and off wherever it cannot be drawn
    /// (every system but Windows 11 22H2 and later).
    #[test]
    fn mica_is_in_force_only_where_wanted_and_possible() {
        assert!(!use_it(false));
        assert!(!in_force());
        assert_eq!(use_it(true), possible());
        assert_eq!(in_force(), possible());
        use_it(false);
    }
}
