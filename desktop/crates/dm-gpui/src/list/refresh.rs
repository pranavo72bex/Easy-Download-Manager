// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a wake reads and draws again, and what the list holds in place
//! (gaps rows 180 to 182): whether a wake draws, what a read may keep,
//! the rows in sight named and their files looked for off the window's thread, a
//! click by the row's download, and the row at the top of the view held.

use std::time::{Duration, Instant};

use dm_core::{Entry, State};
use gpui::{AppContext, Context};

use super::{Downloads, IDLE, ROW_HEIGHT};

impl Downloads {
    /// What the window shows beside the rows that a wake's work may change
    /// without saying so: compared before and after it (`needs_drawing`).
    pub(super) fn shown_beside(&self) -> (usize, Option<Instant>, usize, bool, bool, usize) {
        (
            self.problems.len(),
            self.said.as_ref().map(|(_, at)| *at),
            self.sheets.len(),
            self.menu.is_some(),
            self.hidden,
            self.selected.len(),
        )
    }

    /// Whether what the window shows moves with the clock alone: a running
    /// download's figures, a retry's countdown, When done's, a sentence in
    /// the status bar about to go, a file's days before the Trash.
    pub(super) fn moves_with_time(&self) -> bool {
        self.session.countdown.is_some()
            || dm_desktop_base::rows::retrying_any()
            || self
                .said
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() < crate::statusbar::SAID_FOR + IDLE)
            || self
                .rows
                .iter()
                .any(|r| r.state == State::Running || r.delete_after.is_some())
    }

    /// Keeps the words for when each row finished (`format::Whens`), as the
    /// queue is read: worked out in a frame, they asked the disk for the
    /// time zone (gaps row 180). Every row's where `all` (the rows were
    /// placed again) or the day, zone or `language` changed; otherwise those
    /// of the rows that changed, `finished_at`.
    pub(super) fn keep_whens(&mut self, language: &str, all: bool, finished_at: Vec<u64>) {
        if self.whens.renew(language) || all {
            self.whens
                .keep(self.rows.iter().filter_map(|row| row.finished_at));
        } else {
            self.whens.keep(finished_at);
        }
    }

    /// Looks for the files of the finished rows on screen and of those
    /// chosen, off the window's thread, at most once a second, and draws
    /// again where one is found gone or back: a row's "File missing", and
    /// the commands that need its file, read what was found
    /// (`rows::gone`), not the disk as each frame is drawn (gaps row 180).
    pub(crate) fn look_for_files(&mut self, cx: &mut Context<Self>) {
        if self.looking_for_files
            || self
                .files_looked
                .is_some_and(|at| at.elapsed() < LOOK_EVERY)
        {
            return;
        }
        let on_screen = self
            .shown
            .get(self.visible.clone())
            .unwrap_or_default()
            .iter()
            .filter_map(|&r| self.rows.get(r))
            .map(|r| &**r);
        let files: Vec<(dm_core::Id, String)> = on_screen
            .chain(self.chosen())
            .filter(|r| !r.path.is_empty())
            .map(|r| (r.id, r.path.clone()))
            .collect();
        self.files_looked = Some(Instant::now());
        if files.is_empty() {
            return;
        }
        self.looking_for_files = true;
        let found = cx.background_spawn(async move { dm_desktop_base::rows::look_for(files) });
        cx.spawn(async move |this, cx| {
            let found = found.await;
            let _ = this.update(cx, |this, cx| {
                this.looking_for_files = false;
                if dm_desktop_base::rows::set_looked(found) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Names the rows at `which` (places in `rows`) by the name each file
    /// will get, with the earlier rows of the same name the last whole read
    /// counted (`earlier`).
    pub(super) fn name_rows(&mut self, which: &[usize], settings: &dm_shell::Settings) {
        let mut categories = settings.categories();
        for &at in which {
            let Some(row) = self.rows.get(at) else {
                continue;
            };
            let name = dm_shell::app::names::name_it_will_get_after(
                &self.rows,
                row,
                &settings.download_dir,
                &mut categories,
                Some(self.earlier.get(&row.id).copied().unwrap_or(0)),
            );
            self.shown_names.insert(row.id, name);
        }
    }

    /// Where in `rows` the rows on screen and those chosen are.
    pub(super) fn rows_seen(&self) -> Vec<usize> {
        let mut seen: Vec<usize> = self
            .shown
            .get(self.visible.clone())
            .unwrap_or_default()
            .to_vec();
        seen.extend(
            self.selected
                .iter()
                .filter_map(|id| self.positions.get(id))
                .filter_map(|&at| self.shown.get(at).copied()),
        );
        seen.sort_unstable();
        seen.dedup();
        seen
    }

    /// Which rows the list shows, in order: those in the place chosen,
    /// sorted as chosen.
    pub(super) fn reshow(&mut self) {
        // The row at the top of the view, held there below.
        let top = self.top_row();
        let categories = self.drawn.categories();
        let mut shown: Vec<usize> = (0..self.rows.len())
            .filter(|&i| {
                self.places.within(&self.place, &self.rows[i], &categories)
                    && self.places.matches_filter(&self.filter_text, &self.rows[i])
            })
            .collect();
        if let Some((column, up)) = self.sort {
            let rows = &self.rows;
            shown.sort_by(|&a, &b| {
                let order = dm_desktop_base::columns::compare(column, &rows[a], &rows[b]);
                if up {
                    order
                } else {
                    order.reverse()
                }
            });
        }
        self.positions = shown
            .iter()
            .enumerate()
            .filter_map(|(at, &r)| self.rows.get(r).map(|row| (row.id, at)))
            .collect();
        self.shown = shown;
        // Rows added or taken away above the view, or a sort, no longer
        // move what is in it: the row that was at its top stays there
        // (gaps row 182).
        if let Some((id, was, height, offset)) = top {
            if let Some(&now) = self.positions.get(&id) {
                let held = holding_top(offset, height, was, now);
                if held != offset {
                    let state = self.scroll.0.borrow();
                    let x = state.base_handle.offset().x;
                    state.base_handle.set_offset(gpui::point(x, gpui::px(held)));
                }
            }
        }
    }

    /// The list laid out `range` on screen: its files are looked for at
    /// once where it moved.
    pub(crate) fn laid_out(&mut self, range: std::ops::Range<usize>, cx: &mut Context<Self>) {
        if self.visible != range {
            self.visible = range;
            self.files_looked = None;
            self.look_for_files(cx);
            // The rows come into sight named at the next read (`read`).
            self.named_at = None;
        }
    }

    /// The row at the top of the list's view, where it is in the list, the
    /// rows' height and how far the list is scrolled (both in points); none
    /// before the list is first laid out.
    pub(super) fn top_row(&self) -> Option<(dm_core::Id, usize, f32, f32)> {
        let state = self.scroll.0.borrow();
        // Not laid out yet: nothing on screen to hold.
        state.last_item_size?;
        let height = f32::from(crate::zoom::px(ROW_HEIGHT));
        let offset = f32::from(state.base_handle.offset().y);
        if height <= 0.0 {
            return None;
        }
        let at = (-offset / height).floor().max(0.0) as usize;
        Some((self.id_at(at)?, at, height, offset))
    }
}

/// Whether a wake draws the window again: where the read found a change,
/// a request was answered (what it did is to be seen), something shown
/// moves with the clock, or the wake's work changed what stands beside the
/// rows.
pub(super) fn needs_drawing(read_changed: bool, answered: bool, timed: bool, beside: bool) -> bool {
    read_changed || answered || timed || beside
}

/// The choice after a click on row `id`, at `position` in the list as
/// shown, with ⌘ (`toggle`) or ⇧ (`range`) held: from `anchor`'s position
/// to it with ⇧ (`shown`, every row's id in order), it added or taken away
/// with ⌘, else it alone. The choice, and its new anchor (none with ⇧,
/// which keeps the one it ran from).
pub(super) fn clicked(
    mut selected: std::collections::BTreeSet<dm_core::Id>,
    anchor: Option<usize>,
    shown: &[dm_core::Id],
    id: dm_core::Id,
    position: usize,
    toggle: bool,
    range: bool,
) -> (std::collections::BTreeSet<dm_core::Id>, Option<dm_core::Id>) {
    if range {
        let from = anchor.unwrap_or(position);
        let (low, high) = (from.min(position), from.max(position));
        let chosen = shown
            .get(low..=high)
            .unwrap_or_default()
            .iter()
            .copied()
            .collect();
        return (chosen, None);
    }
    if toggle {
        if !selected.remove(&id) {
            selected.insert(id);
        }
        return (selected, Some(id));
    }
    ([id].into(), Some(id))
}

/// Where the list is scrolled to (`offset`, 0 at its top and less below
/// it, as GPUI keeps it) so the row at the top of the view, which was at
/// `was` in the list and is now at `now`, stays where it was in the view;
/// each row `height` tall. A list at its very top stays there, rows added
/// above in sight, as a new download is looked for there.
pub(super) fn holding_top(offset: f32, height: f32, was: usize, now: usize) -> f32 {
    if offset >= 0.0 || height <= 0.0 {
        return offset;
    }
    offset - (now as f32 - was as f32) * height
}

/// Whether two reads of a row differ only in its figures (bytes, rate,
/// time left), which neither its name, its place, nor whether it is chosen
/// follows from.
pub(super) fn placed_alike(old: &Entry, new: &Entry) -> bool {
    old.state == new.state
        && old.path == new.path
        && old.filename == new.filename
        && old.url == new.url
        && old.save_dir == new.save_dir
        && old.save_name == new.save_name
        && old.category == new.category
        && old.queue == new.queue
}

/// How often, at most, the rows on screen and those chosen are named again
/// while only figures change (`Downloads::read`).
pub(super) const NAMES_EVERY: Duration = Duration::from_secs(2);

/// How often, at most, the files of the rows on screen are looked for
/// (`Downloads::look_for_files`).
pub(super) const LOOK_EVERY: Duration = Duration::from_secs(1);

#[cfg(test)]
mod tests {
    use super::{clicked, holding_top, needs_drawing};
    /// A click acts on the row clicked, though a sort moved it between the
    /// frame it was drawn in and the click: the frame put download 2 at
    /// the second place, and the sort put download 3 there (gaps row 182).
    #[test]
    fn a_click_acts_on_the_row_clicked_after_a_sort_moved_it() {
        // As drawn: 1, 2, 3; the click is on download 2.
        let clicked_id = [1u64, 2, 3][1];
        // Sorted again before the click is handled: 3 now at the second
        // place, 2 at the first.
        let now = [2u64, 3, 1];
        let at = now.iter().position(|&id| id == clicked_id).unwrap();
        let (chosen, anchor) =
            clicked(Default::default(), None, &now, clicked_id, at, false, false);
        assert_eq!(chosen.into_iter().collect::<Vec<_>>(), vec![2]);
        assert_eq!(anchor, Some(2));
        // ⇧ from the anchor, by where the rows are now.
        let (chosen, anchor) = clicked(Default::default(), Some(0), &now, 1, 2, false, true);
        assert_eq!(chosen.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(anchor, None, "a ⇧-click leaves the anchor where it was");
        // ⌘ adds, and takes away again.
        let (chosen, _) = clicked([2].into(), None, &now, 3, 1, true, false);
        assert_eq!(chosen.iter().copied().collect::<Vec<_>>(), vec![2, 3]);
        let (chosen, _) = clicked(chosen, None, &now, 3, 1, true, false);
        assert_eq!(chosen.into_iter().collect::<Vec<_>>(), vec![2]);
    }

    /// The row at the top of the view stays there while rows are added or
    /// taken away above it, half a row scrolled past as it was; a list at
    /// its top stays at its top (gaps row 182).
    #[test]
    fn the_top_row_holds_its_place_as_rows_come_and_go_above_it() {
        let h = 22.0;
        // Row 10 at the top, half of it scrolled past.
        let offset = -10.5 * h;
        // Two rows added above: it is the twelfth now.
        assert_eq!(holding_top(offset, h, 10, 12), -12.5 * h);
        // Five taken away above: the fifth.
        assert_eq!(holding_top(offset, h, 10, 5), -5.5 * h);
        // Nothing moved above it.
        assert_eq!(holding_top(offset, h, 10, 10), offset);
        // At the very top: the rows added are in sight.
        assert_eq!(holding_top(0.0, h, 0, 3), 0.0);
    }

    /// A wake that changed nothing shown draws nothing (gaps row 181).
    #[test]
    fn a_wake_that_changed_nothing_draws_nothing() {
        assert!(!needs_drawing(false, false, false, false));
        assert!(needs_drawing(true, false, false, false));
        assert!(needs_drawing(false, true, false, false));
        assert!(needs_drawing(false, false, true, false));
        assert!(needs_drawing(false, false, false, true));
    }
}
