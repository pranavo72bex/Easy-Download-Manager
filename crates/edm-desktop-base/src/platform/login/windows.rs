// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows: the current user's Run key `auto-launch` writes, and Task
//! Manager's Startup tab.

/// The program the login item `auto-launch` wrote starts: on Windows the
/// Run key's value. `None` when there is none or it cannot be read, which
/// is not taken as a reason to rewrite it.
pub fn login_item_program() -> Option<std::path::PathBuf> {
    use crate::autostart::{registry, run_value_program, RUN_KEY};
    registry::string(RUN_KEY, "EDM").and_then(|value| run_value_program(&value))
}

/// Whether the person turned EDM's login item off outside EDM: on Windows,
/// in Task Manager's Startup tab.
pub fn login_item_turned_off_outside() -> bool {
    use crate::autostart::{registry, turned_off_in_task_manager, STARTUP_APPROVED_KEY};
    registry::binary(STARTUP_APPROVED_KEY, "EDM")
        .is_some_and(|approved| turned_off_in_task_manager(&approved))
}
