use super::*;

impl DmApp {
    pub fn shows(&self, row: &Entry) -> bool {
        self.within(&self.place, row) && self.matches_filter(row)
    }

    /// Whether every word typed over the list is in the row's name, its
    /// address or its note, whatever their case.
    pub(super) fn matches_filter(&self, row: &Entry) -> bool {
        if self.filter.trim().is_empty() {
            return true;
        }
        // The words made again only when the filter has changed.
        if self.filter_words.borrow().0 != self.filter {
            let words = self
                .filter
                .split_whitespace()
                .map(str::to_lowercase)
                .collect();
            *self.filter_words.borrow_mut() = (self.filter.clone(), words);
        }
        let cached = self.filter_words.borrow();
        let found = |said: &str| cached.1.iter().all(|w| said.contains(w.as_str()));
        match self.haystacks.get(&row.id) {
            Some(said) => found(said),
            // A row the last refresh has not seen yet.
            None => found(&haystack(row)),
        }
    }

    /// Whether `row` belongs in `place`.
    pub(super) fn within(&self, place: &Place, row: &Entry) -> bool {
        match place {
            Place::All => true,
            // A row whose kind is not known yet, and in no category of a
            // person's own, is Other: queued `.bin` rows were in no place
            // and the counts did not add up.
            Place::Kind(c) => match self.placed(row) {
                (Some(kind), _) => kind == *c,
                (None, custom) => *c == Category::Other && custom.is_none(),
            },
            Place::Custom(name) => self.placed(row).1.as_deref() == Some(name.as_str()),
            // What is under way: running, waiting its turn, or held by the
            // schedule or the download limit. Not a failed row or one a
            // person stopped, which the status bar does not count as
            // downloading either.
            Place::Unfinished => {
                matches!(row.state, State::Running | State::Queued)
                    || (row.state == State::Paused && row.paused_by_schedule)
            }
            Place::Finished => row.state == State::Completed,
            Place::Queue(name) => &row.queue == name,
        }
    }

    /// The category a person made that `row` belongs to, by the rule its
    /// file was (or will be) placed by.
    pub fn custom_of(&self, row: &Entry) -> Option<String> {
        let categories = self.applied.categories();
        let name = name_of(row);
        categories
            .custom_for(&name, &dm_core::category::host_of(&row.url))
            .map(|c| c.name.clone())
    }

    /// Each row's kind and category of a person's own, worked out once a
    /// refresh: the places' counts asked for them per row, per place, per
    /// frame, and 500 rows went past the frame budget (CI, 909e714).
    pub(super) fn place_rows(&mut self) {
        let categories = self.applied.categories();
        let mut placed = HashMap::with_capacity(self.rows.len());
        let mut haystacks = HashMap::with_capacity(self.rows.len());
        for row in &self.rows {
            // Each row's name worked out once: from its address, for a row
            // not yet named, it is a parse and a decode, and it was done
            // three times a row (2.8 µs a row, 14 ms at 5,000 rows).
            let name = name_of(row);
            // The site only where a category of a person's own could ask:
            // without one, every address was parsed again for nothing.
            let custom = if categories.custom.is_empty() {
                None
            } else {
                categories
                    .custom_for(&name, &dm_core::category::host_of(&row.url))
                    .map(|c| c.name.clone())
            };
            let kind = dm_core::category::by_name(&name).or(row.category);
            haystacks.insert(row.id, haystack_named(&name, row));
            placed.insert(row.id, (kind, custom));
        }
        self.placed = placed;
        self.haystacks = haystacks;
    }

    /// A row's kind and own category, from [`Self::place_rows`] when it has
    /// been placed.
    pub(super) fn placed(&self, row: &Entry) -> (Option<Category>, Option<String>) {
        match self.placed.get(&row.id) {
            Some(placed) => placed.clone(),
            None => (kind_of(row), self.custom_of(row)),
        }
    }

    pub fn count(&self, place: &Place) -> usize {
        self.rows.iter().filter(|r| self.within(place, r)).count()
    }

    /// The rows the table shows, in the order it shows them.
    pub fn visible(&self) -> Vec<Entry> {
        let rows: Vec<Entry> = self
            .rows
            .iter()
            .filter(|r| self.shows(r))
            .cloned()
            .collect();
        self.shaped(rows)
    }

    /// `rows` as the table shows them: with the size a dialog learned until
    /// the row knows its own, and in the order it is sorted.
    fn shaped(&self, mut rows: Vec<Entry>) -> Vec<Entry> {
        // What a dialog learned of a row's size, until the row knows it.
        for row in &mut rows {
            if row.progress.total.is_none() {
                if let Some(size) = self.sizes_told.get(&row.id) {
                    row.progress.total = Some(*size);
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
        let rows: Vec<Entry> = self
            .rows
            .iter()
            .filter(|r| self.selected.contains(&r.id) && self.shows(r))
            .cloned()
            .collect();
        self.shaped(rows)
    }

    /// The queue a queue command means: the one shown, or the main one.
    pub fn current_queue(&self) -> String {
        match &self.place {
            Place::Queue(name) => name.clone(),
            _ => MAIN_QUEUE.to_string(),
        }
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
