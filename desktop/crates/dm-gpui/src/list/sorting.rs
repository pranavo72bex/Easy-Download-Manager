// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How the list is shown, kept for the next launch (gaps row U29): its sort, and View's
//! Categories and Toolbar.

use dm_desktop_base::columns::Column;
use gpui::Context;

use super::Downloads;

impl Downloads {
    /// Sorts by `column`: up, then the other way on a second click, then
    /// back in the order the downloads were added, as the egui window's
    /// header does; kept in the settings, so the list comes back sorted so
    /// after a restart.
    pub fn sort_by(&mut self, column: Column, cx: &mut Context<Self>) {
        self.sort = dm_desktop_base::columns::next_sort(self.sort, column);
        self.reshow();
        let mut wanted = self.backend.settings();
        dm_desktop_base::columns::keep_sort(&mut wanted, self.sort);
        if let Err(e) = self.apply_settings(wanted, cx) {
            self.say(e);
        }
    }

    /// The side panel and toolbar kept as they are now shown, for the next launch.
    pub(super) fn keep_shown(&mut self, cx: &mut Context<Self>) {
        let mut wanted = self.backend.settings();
        wanted.show_categories = self.show_sidebar;
        wanted.show_toolbar = self.show_toolbar;
        if let Err(e) = self.apply_settings(wanted, cx) {
            self.say(e);
        }
    }
}

/// The command that sorts by `column`, for View › Sort by's tick; none for
/// a column the menu does not sort by.
pub(super) fn sort_command(column: Column) -> Option<dm_shell::app::commands::Command> {
    use dm_shell::app::commands::Command;
    Some(match column {
        Column::Name => Command::SortByName,
        Column::Size => Command::SortBySize,
        Column::Status => Command::SortByStatus,
        Column::TimeLeft => Command::SortByTimeLeft,
        Column::Rate => Command::SortByRate,
        Column::LastTry => Command::SortByDate,
        Column::Description => Command::SortByDescription,
        _ => return None,
    })
}
