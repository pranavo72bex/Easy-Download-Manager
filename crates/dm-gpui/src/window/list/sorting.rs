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
    /// back in the order the downloads were added; kept in the settings, so
    /// the list comes back sorted so after a restart.
    pub fn sort_by(&mut self, column: Column, cx: &mut Context<Self>) {
        self.list.sort = dm_desktop_base::columns::next_sort(self.list.sort, column);
        self.reshow();
        // The window's look alone (`keep_look`): kept for the next start with the file written
        // off the window's thread. Applied as a whole change of the settings it cost each click
        // about 50 ms on the window's thread, a sort taking 0.069 s at 100 rows where 0.017 s
        // had (`ui-bench.py`, 2026-11-16).
        let sort = self.list.sort;
        self.backend
            .keep_look(|settings| dm_desktop_base::columns::keep_sort(settings, sort));
        cx.notify();
    }

    /// The side panel and toolbar kept as they are now shown, for the next launch.
    pub(super) fn keep_shown(&mut self, cx: &mut Context<Self>) {
        let (sidebar, toolbar) = (self.layout.show_sidebar, self.layout.show_toolbar);
        self.backend.keep_look(|settings| {
            settings.show_categories = sidebar;
            settings.show_toolbar = toolbar;
        });
        cx.notify();
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
