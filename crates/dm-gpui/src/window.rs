// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The main window: the list and its table, the bars around it, its menus, and what each wake does (gaps row A13).

pub(crate) mod a11y;
pub(crate) mod actions;
pub(crate) mod bars;
pub(crate) mod frame;
pub(crate) mod list;
pub(crate) mod menus;
pub(crate) mod mirror;
pub(crate) mod narrow;
pub(crate) mod placemenu;
pub(crate) mod reach;
pub(crate) mod rowmenu;
pub(crate) mod safe;
pub(crate) mod screen;
pub(crate) mod sidebar;
pub(crate) mod statusbar;
pub(crate) mod table;
pub(crate) mod theme;
pub(crate) mod tick;
pub(crate) mod titlebar;
pub(crate) mod toolbar;
pub(crate) mod viewmenu;
pub(crate) mod zoom;

/// Moves nothing that is not needed where the system asks for less motion (gaps row K7): asked
/// at start and again whenever the window comes in front or the look changes, so a change made
/// while dm runs is followed (it was read once, at start).
pub(crate) fn follow_motion(cx: &mut gpui::App) {
    let still = dm_desktop_base::motion::reduced();
    cx.set_reduce_motion(still);
    table::set_still(still);
}
