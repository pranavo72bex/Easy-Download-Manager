// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl DmApp {
    /// Whether the list shows `row` in the place chosen and under the filter typed.
    #[must_use]
    pub fn shows(&self, row: &Entry) -> bool {
        self.within(&self.place, row) && self.matches_filter(row)
    }

    /// Whether every word typed over the list is in the row's name, its
    /// address or its note, whatever their case.
    pub(super) fn matches_filter(&self, row: &Entry) -> bool {
        self.places.matches_filter(&self.filter, row)
    }

    /// Whether `row` belongs in `place`.
    pub(super) fn within(&self, place: &Place, row: &Entry) -> bool {
        self.places.within(place, row, &self.applied.categories())
    }

    /// The category a person made that `row` belongs to, by the rule its
    /// file was (or will be) placed by.
    pub fn custom_of(&self, row: &Entry) -> Option<String> {
        dm_shell::app::places::custom_of(row, &self.applied.categories())
    }

    /// Each row's kind and category of a person's own, worked out once a
    /// read (`dm_shell::app::places::Places::place_rows`).
    pub(super) fn place_rows(&mut self) {
        self.rows_gen += 1;
        self.places
            .place_rows(&self.rows, &self.applied.categories());
    }

    /// How many rows are in `place`: counted once for each read.
    pub fn count(&self, place: &Place) -> usize {
        self.places
            .count(place, &self.rows, &self.applied.categories())
    }

    /// The rows the table shows, in the order it shows them, as a list of
    /// the caller's own. To read them, [`Self::visible_shared`] shares the
    /// one the table keeps rather than copying it.
    pub fn visible(&self) -> Vec<Arc<Entry>> {
        self.visible_shared().as_ref().clone()
    }

    /// [`Self::visible`], kept from one frame to the next for as long as
    /// nothing it is made from has changed: the list read again, the place,
    /// the filter, the sort or a size a dialog told. The table asks every
    /// frame, and a copy and a sort of every row cost a few milliseconds a
    /// frame at 5,000 rows, in every frame the pointer moved.
    pub fn visible_shared(&self) -> SharedRows {
        let key = VisibleKey {
            list: self.rows_gen,
            place: self.place.clone(),
            filter: self.filter.clone(),
            sort: self.sort,
            sizes_told: self.sizes_told.len(),
        };
        if let Some((made_from, rows)) = &*self.visible_cache.borrow() {
            if *made_from == key {
                return std::rc::Rc::clone(rows);
            }
        }
        let rows: Vec<Arc<Entry>> = self
            .rows
            .iter()
            .filter(|r| self.shows(r))
            .cloned()
            .collect();
        let rows = std::rc::Rc::new(self.shaped(rows));
        *self.visible_cache.borrow_mut() = Some((key, std::rc::Rc::clone(&rows)));
        rows
    }

    /// `rows` as the table shows them: with the size a dialog learned until
    /// the row knows its own, and in the order it is sorted.
    fn shaped(&self, mut rows: Vec<Arc<Entry>>) -> Vec<Arc<Entry>> {
        // What a dialog learned of a row's size, until the row knows it:
        // that row alone is copied, the rest stay shared.
        for row in &mut rows {
            if row.progress.total.is_none() {
                if let Some(size) = self.sizes_told.get(&row.id) {
                    Arc::make_mut(row).progress.total = Some(*size);
                }
            }
        }
        if let Some((column, ascending)) = self.sort {
            rows.sort_by(|a, b| {
                let order = compare(column, a, b);
                if ascending {
                    order
                } else {
                    order.reverse()
                }
            });
        }
        rows
    }

    /// The rows a command acts on: the selection, in table order.
    ///
    /// Only the selected rows are copied and sorted, not every row shown: a
    /// stable sort of some of the rows is those rows in the order a stable
    /// sort of all of them puts them in. The toolbar asks for this several
    /// times a frame, and with nothing selected it asks for nothing at all
    /// (it was a copy and a sort of the whole list, 5,000 rows, each time).
    pub fn targets(&self) -> Vec<Entry> {
        if self.selected.is_empty() {
            return Vec::new();
        }
        let rows: Vec<Arc<Entry>> = self
            .rows
            .iter()
            .filter(|r| self.selected.contains(&r.id) && self.shows(r))
            .cloned()
            .collect();
        self.shaped(rows)
            .into_iter()
            .map(Arc::unwrap_or_clone)
            .collect()
    }

    /// The queue a queue command means: the one shown, or the main one.
    pub fn current_queue(&self) -> String {
        match &self.place {
            Place::Queue(name) => name.clone(),
            _ => MAIN_QUEUE.to_string(),
        }
    }

    /// The queue Start Queue or Stop Queue acts on: the one a drop-down
    /// chose for it, else the one the list shows. Taken, so the choice is
    /// for that one command.
    pub fn chosen_queue(&mut self) -> String {
        self.queue_chosen
            .take()
            .unwrap_or_else(|| self.current_queue())
    }

    /* --------------------------------------------------------- commands */

    /// Each column on or off — View › Columns, and a right click on a
    /// heading. The file name always shows.
    ///
    /// And moved, so the columns can be put in any order:
    /// each but the file name can go a place earlier or later. Kept in the
    /// settings, so the list looks the same at the next launch.
    pub fn columns_menu(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let order = self.column_order.clone();
        for (at, column) in order.iter().copied().enumerate().skip(1) {
            ui.horizontal(|ui| {
                let mut on = !self.hidden_columns.contains(&column);
                if crate::dialogs::check(ui, &mut on, column.title()).clicked() {
                    if on {
                        self.hidden_columns.remove(&column);
                    } else {
                        self.hidden_columns.insert(column);
                    }
                    changed = true;
                }
                // The file name stays first, so nothing moves before it.
                if let Some(to) = move_buttons(ui, column.title(), at > 1, at + 1 < order.len()) {
                    self.column_order.swap(at, to(at));
                    changed = true;
                }
            });
        }
        if changed {
            self.save_columns();
            // The widths go with the places, not the columns: Status took
            // Size's 76 points and read "Compl…". Fitted
            // afresh to what is shown now.
            self.columns_changed = true;
        }
    }

    /// Writes the columns' order and which are off into the settings.
    pub(super) fn save_columns(&mut self) {
        let mut wanted = self.applied.clone();
        wanted.column_order = self
            .column_order
            .iter()
            .map(|c| c.key().to_string())
            .collect();
        let mut hidden: Vec<String> = self
            .hidden_columns
            .iter()
            .map(|c| c.key().to_string())
            .collect();
        hidden.sort();
        wanted.columns_hidden = hidden;
        if let Err(e) = self.apply_settings(wanted) {
            self.said = e.into();
        }
    }

    /// Sorts by a column; asked for again, the other way round; a third
    /// time, back in the queue's order. Kept in the settings, so the list
    /// is sorted the same way at the next launch.
    pub fn sort_by(&mut self, column: Column) {
        // Up, down, then the queue's own order again: a list once sorted
        // could not go back to it, where rows are dragged into another
        // order.
        self.sort = match self.sort {
            Some((c, true)) if c == column => Some((c, false)),
            Some((c, false)) if c == column => None,
            _ => Some((column, true)),
        };
        let mut wanted = self.applied.clone();
        match self.sort {
            Some((column, ascending)) => {
                wanted.sort_column = column.key().to_string();
                wanted.sort_ascending = ascending;
            }
            None => wanted.sort_column = String::new(),
        }
        if let Err(e) = self.apply_settings(wanted) {
            self.said = e.into();
        }
    }
}
