// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the browser's toolbar popup asked of a download (gaps row C12), carried out by whatever
//! drives dm as the window's own command for that row: Open and Open folder as the effects every
//! driver carries out on its platform ([`Effect::Open`], [`Effect::Reveal`]); Resume download,
//! Stop download and Delete Completed through the application's verbs
//! (`dm_shell::app::commands::verb`).

use std::sync::Arc;

use dm_core::Entry;
use dm_shell::app::backend::Backend;
use dm_shell::app::commands::{verb, Command};

use super::{Driver, Effect, Session};

impl Session {
    /// The popup's asks waiting in the inbox (`app::going::asked_of_window`), each taken once,
    /// where the driver reads the inbox.
    pub(super) fn browser_asks(
        &mut self,
        backend: &Backend,
        rows: &[Arc<Entry>],
        driver: Driver,
        effects: &mut Vec<Effect>,
    ) {
        if !driver.reads_inbox {
            return;
        }
        for act in dm_shell::app::going::asked_of_window(&dm_shell::settings::home()) {
            effects.extend(carry_asked(backend, rows, &act));
        }
    }
}

/// Carries out the popup's `act` over the last read's `rows`: what is the driver's to do (a file
/// opened or shown), or nothing where the verb was done here, or the row is gone, or the command
/// does not apply to it now (`app::going::command_for`).
pub fn carry_asked(
    backend: &Backend,
    rows: &[Arc<Entry>],
    act: &dm_protocol::Act,
) -> Option<Effect> {
    let (command, row) = dm_shell::app::going::command_for(act, rows)?;
    match (command, row) {
        (Command::OpenFile, Some(row)) => Some(Effect::Open(row.path)),
        (Command::OpenFolder, Some(row)) => Some(Effect::Reveal(row.path)),
        (command, row) => {
            let chosen: Vec<Entry> = row.into_iter().collect();
            verb(command, backend, &chosen, "");
            None
        }
    }
}
