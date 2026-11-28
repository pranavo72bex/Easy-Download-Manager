// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A menu item found by its path, for the control port's `ui.menu`.

use super::{window_menus, Ticks};

/// The action of the menu item at `path` (a menu's name, then its items' words, "…" or not), as
/// the window's menus hold it now: what the control port's `ui.menu` runs, where a script cannot
/// reach the menus by their drawn titles (macOS, whose menu bar is the system's). None for a
/// path that is no item, or names one greyed.
pub(crate) fn action_at(ticks: &Ticks, path: &[String]) -> Option<Box<dyn gpui::Action>> {
    let menus: Vec<gpui::OwnedMenu> = window_menus(ticks).into_iter().map(|m| m.owned()).collect();
    let same =
        |name: &str, wanted: &str| name.trim_end_matches('…') == wanted.trim_end_matches('…');
    let (first, rest) = path.split_first()?;
    let mut items = &menus.iter().find(|m| same(&m.name, first))?.items;
    for (i, name) in rest.iter().enumerate() {
        let last = i + 1 == rest.len();
        let mut next = None;
        for item in items {
            match item {
                gpui::OwnedMenuItem::Submenu(sub) if !last && same(&sub.name, name) => {
                    next = Some(&sub.items);
                    break;
                }
                gpui::OwnedMenuItem::Action {
                    name: words,
                    action,
                    disabled,
                    ..
                } if last && same(words, name) => {
                    return (!*disabled).then(|| action.boxed_clone());
                }
                _ => {}
            }
        }
        items = next?;
    }
    None
}
