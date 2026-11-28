// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The console of whatever started EDM (`modes::attach_console`): a Windows release build is a
//! GUI program with none of its own. Elsewhere a program always writes to its terminal, so
//! there is nothing to do.

/// Attaches EDM to the console of whatever started it, so `println!` reaches the terminal;
/// with none (started from Explorer) nothing changes.
#[cfg(windows)]
pub(crate) fn attach() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: no pointers; it fails harmlessly when there is no parent
    // console or one is already attached.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Nothing to attach to: a program here writes to its terminal already.
#[cfg(not(windows))]
pub(crate) fn attach() {}
