// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list's keys, as the egui window has them (gaps row U6): the arrows, Home, End, Page Up
//! and Page Down move the choice, Shift extending it from where it started
//! (`dm_desktop_base::selection::stepped`); Enter shows the chosen downloads' progress; Shift+F10
//! and the Menu key open the row menu at the keyboard's place.

use dm_desktop_base::selection::{stepped, Step};

use super::*;

impl Downloads {
    /// Moves the keyboard's place by `step`, choosing the row moved to or, with `shift`, the run
    /// from where it started; the row moved to kept on screen.
    pub fn step_by(&mut self, step: Step, shift: bool) {
        let ids: Vec<dm_core::Id> = (0..self.shown.len())
            .filter_map(|p| self.id_at(p))
            .collect();
        // The place is the last row moved to while it is still chosen; else the first chosen.
        let cursor = self
            .cursor
            .filter(|c| self.selected.contains(c))
            .or(self.anchor.filter(|a| self.selected.contains(a)));
        let (selected, anchor, cursor) =
            stepped(&ids, &self.selected, self.anchor, cursor, step, shift);
        self.selected = selected;
        self.anchor = anchor;
        self.cursor = cursor;
        if let Some(at) = cursor.and_then(|c| self.position_of(c)) {
            self.scroll
                .scroll_to_item(at, gpui::ScrollStrategy::Nearest);
        }
    }

    /// How many rows Page Up and Page Down move: a screenful, less one so the row the page
    /// started from stays in view.
    pub(crate) fn page(&self) -> usize {
        let height = f32::from(self.scroll.0.borrow().base_handle.bounds().size.height);
        ((height / ROW_HEIGHT) as usize).saturating_sub(1).max(1)
    }

    /// The row menu at the keyboard's place, as a right-click on that row opens it: Shift+F10
    /// and the Menu key.
    pub(crate) fn row_menu_by_key(&mut self) {
        let Some(at) = self
            .cursor
            .or(self.anchor)
            .and_then(|c| self.position_of(c))
        else {
            return;
        };
        let top = f32::from(self.scroll.0.borrow().base_handle.bounds().origin.y);
        let offset = f32::from(self.scroll.0.borrow().base_handle.offset().y);
        let y = top + offset + (at as f32 + 0.5) * ROW_HEIGHT;
        let x = f32::from(self.scroll.0.borrow().base_handle.bounds().origin.x) + 40.0;
        self.menu = Some(gpui::point(gpui::px(x), gpui::px(y)));
    }
}
