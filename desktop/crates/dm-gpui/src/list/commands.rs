// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Which commands can be run now, for the rows chosen: what the toolbar, the menus and the row
//! menu grey, and why a greyed button is greyed.

use super::*;

impl Downloads {
    /// Whether `command` can be run now: the shell carries it out, and it
    /// applies to the rows chosen (`commands::applies`), as the egui
    /// window greys its buttons and menu items.
    pub fn enabled(&self, command: dm_shell::app::commands::Command) -> bool {
        if !crate::menus::carried_out(command) {
            return false;
        }
        let chosen = self.chosen();
        dm_shell::app::commands::applies(
            command,
            &chosen,
            &self.rows,
            false,
            &dm_desktop_base::rows::Looks,
        )
    }

    /// Why `command` is greyed, in the language in force, where it is: none is chosen and one would do, the
    /// rows chosen are not ones it acts on, or nothing in the list is (gaps row U5).
    pub(crate) fn why_off(
        &self,
        command: dm_shell::app::commands::Command,
    ) -> Option<&'static str> {
        use dm_shell::app::commands::applies;
        if self.enabled(command) || !crate::menus::carried_out(command) {
            return None;
        }
        let every: Vec<&Entry> = self.rows.iter().map(|r| &**r).collect();
        let some_row_would = applies(
            command,
            &every,
            &self.rows,
            false,
            &dm_desktop_base::rows::Looks,
        );
        use dm_desktop_base::i18n::t;
        Some(match (self.chosen().is_empty(), some_row_would) {
            (true, true) => t("Choose a download first"),
            (false, true) => t("Not for the downloads chosen"),
            (_, false) => t("Nothing in the list for it now"),
        })
    }

    /// Every command that cannot be run now (`enabled`), the rows chosen read once for them all:
    /// what the menus grey (gaps row U5).
    pub(crate) fn commands_off(&self) -> Vec<dm_shell::app::commands::Command> {
        let chosen = self.chosen();
        dm_shell::app::commands::Command::ALL
            .iter()
            .copied()
            .filter(|&command| {
                !crate::menus::carried_out(command)
                    || !dm_shell::app::commands::applies(
                        command,
                        &chosen,
                        &self.rows,
                        false,
                        &dm_desktop_base::rows::Looks,
                    )
            })
            .collect()
    }
}
