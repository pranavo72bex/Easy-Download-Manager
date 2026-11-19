// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list's keys (gaps row U6): the arrows, Home, End, Page Up
//! and Page Down move the choice, Shift extending it from where it started
//! (`dm_desktop_base::selection::stepped`); Enter shows the chosen downloads' progress; Shift+F10
//! and the Menu key open the row menu at the keyboard's place.

use dm_desktop_base::selection::{stepped, Step};

use super::*;

/// How long letters typed over the list are taken as one name to find (`type_to_find`): what
/// comes after a pause this long starts again.
const TYPED_FOR: std::time::Duration = std::time::Duration::from_secs(1);

impl Downloads {
    /// A right-click on the row at `position`, at `at`: the row is chosen
    /// if it was not (a right-click on a chosen row keeps the choice), and
    /// the row menu opens there. The keyboard's place moves to it:
    /// the arrows and Shift go on from the row clicked.
    pub fn open_menu(&mut self, id: dm_core::Id, at: gpui::Point<gpui::Pixels>) {
        if self.position_of(id).is_some() {
            if !self.list.selected.contains(&id) {
                self.list.selected = [id].into();
                self.list.anchor = Some(id);
            }
            self.list.cursor = Some(id);
            self.menus.menu = Some(at);
            self.menus.menu_at = None;
        }
    }

    /// A click on the row at `position` in the list, with ⌘ (`toggle`) or
    /// ⇧ (`range`) held, as a macOS list takes it.
    ///
    /// The row is the one clicked, `id`, wherever it is now: a sort or a
    /// new row between the frame and the click moved it, and the click
    /// acted on whatever row had taken its place (gaps row 182).
    pub fn click_row(&mut self, id: dm_core::Id, toggle: bool, range: bool) {
        let Some(position) = self.position_of(id) else {
            return;
        };
        let shown: Vec<dm_core::Id> = if range {
            (0..self.list.shown.len())
                .filter_map(|p| self.id_at(p))
                .collect()
        } else {
            Vec::new()
        };
        let anchor = self.list.anchor.and_then(|a| self.position_of(a));
        let (selected, anchor) = clicked(
            std::mem::take(&mut self.list.selected),
            anchor,
            &shown,
            id,
            position,
            toggle,
            range,
        );
        self.list.selected = selected;
        self.list.cursor = Some(id);
        // A ⇧-click keeps the anchor it ran from.
        if !range {
            self.list.anchor = anchor;
        }
    }

    /// Closes the row menu.
    pub fn close_menu(&mut self) {
        self.menus.menu = None;
        self.menus.menu_at = None;
    }

    /// Whether the row menu is open.
    pub(crate) fn menu_open(&self) -> bool {
        self.menus.menu.is_some()
    }

    /// Moves the choice one row up (`-1`) or down (`1`), as the arrow keys
    /// do, keeping the row on screen.
    pub fn step(&mut self, by: isize) {
        let step = if by < 0 {
            dm_desktop_base::selection::Step::Up
        } else {
            dm_desktop_base::selection::Step::Down
        };
        self.step_by(step, false);
    }

    /// The id of the row at `position` in the list as shown.
    pub(crate) fn id_at(&self, position: usize) -> Option<dm_core::Id> {
        self.list
            .shown
            .get(position)
            .and_then(|&r| self.rows.get(r))
            .map(|r| r.id)
    }

    /// Where the row `id` is in the list as shown.
    pub(crate) fn position_of(&self, id: dm_core::Id) -> Option<usize> {
        self.list
            .positions
            .get(&id)
            .copied()
            .filter(|&at| self.id_at(at) == Some(id))
    }

    /// Moves the keyboard's place by `step`, choosing the row moved to or, with `shift`, the run
    /// from where it started; the row moved to kept on screen.
    pub fn step_by(&mut self, step: Step, shift: bool) {
        let ids: Vec<dm_core::Id> = (0..self.list.shown.len())
            .filter_map(|p| self.id_at(p))
            .collect();
        // The place is the last row moved to while it is still chosen; else the first chosen.
        let cursor = self
            .list
            .cursor
            .filter(|c| self.list.selected.contains(c))
            .or(self.list.anchor.filter(|a| self.list.selected.contains(a)));
        let (selected, anchor, cursor) = stepped(
            &ids,
            &self.list.selected,
            self.list.anchor,
            cursor,
            step,
            shift,
        );
        self.list.selected = selected;
        self.list.anchor = anchor;
        self.list.cursor = cursor;
        if let Some(at) = cursor.and_then(|c| self.position_of(c)) {
            self.list
                .scroll
                .scroll_to_item(at, gpui::ScrollStrategy::Nearest);
        }
    }

    /// How many rows Page Up and Page Down move: a screenful, less one so the row the page
    /// started from stays in view.
    pub(crate) fn page(&self) -> usize {
        let height = f32::from(self.list.scroll.0.borrow().base_handle.bounds().size.height);
        ((height / ROW_HEIGHT) as usize).saturating_sub(1).max(1)
    }

    /// A letter typed over the list: the next row (from the keyboard's place) whose name starts
    /// with what has been typed in the last second, chosen, as Finder's list finds one.
    pub(crate) fn type_to_find(&mut self, typed: &str) {
        let now = std::time::Instant::now();
        let fresh = self
            .list
            .typed
            .1
            .is_none_or(|at| now.duration_since(at) > TYPED_FOR);
        if fresh {
            self.list.typed.0.clear();
        }
        self.list.typed.0.push_str(&typed.to_lowercase());
        self.list.typed.1 = Some(now);
        let wanted = self.list.typed.0.clone();
        let count = self.list.shown.len();
        let from = self
            .list
            .cursor
            .and_then(|c| self.position_of(c))
            // A new word starts after the row found last; one being typed on looks there too.
            .map_or(0, |at| {
                if wanted.chars().count() == 1 {
                    at + 1
                } else {
                    at
                }
            });
        let found = (0..count).map(|n| (from + n) % count.max(1)).find(|&at| {
            self.list
                .shown
                .get(at)
                .and_then(|&r| self.rows.get(r))
                .is_some_and(|row| self.name_shown(row).to_lowercase().starts_with(&wanted))
        });
        if let Some(id) = found.and_then(|at| self.id_at(at)) {
            self.list.selected = [id].into();
            self.list.anchor = Some(id);
            self.list.cursor = Some(id);
            if let Some(at) = found {
                self.list
                    .scroll
                    .scroll_to_item(at, gpui::ScrollStrategy::Nearest);
            }
        }
    }

    /// The row menu at the keyboard's place, as a right-click on that row opens it: Shift+F10
    /// and the Menu key.
    pub(crate) fn row_menu_by_key(&mut self) {
        let Some(at) = self
            .list
            .cursor
            .or(self.list.anchor)
            .and_then(|c| self.position_of(c))
        else {
            return;
        };
        let top = f32::from(self.list.scroll.0.borrow().base_handle.bounds().origin.y);
        let offset = f32::from(self.list.scroll.0.borrow().base_handle.offset().y);
        let y = top + offset + (at as f32 + 0.5) * ROW_HEIGHT;
        let x = f32::from(self.list.scroll.0.borrow().base_handle.bounds().origin.x) + 40.0;
        self.menus.menu = Some(gpui::point(gpui::px(x), gpui::px(y)));
        // Opened by a key, the keyboard is on its first entry that applies.
        self.menus.menu_at = None;
        self.step_menu(1);
    }
}
