// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The places a list of downloads is seen through (All Downloads,
//! Unfinished, Finished, each kind of file, a person's own categories, each
//! queue): which rows each holds, how many, and the filter typed over the
//! list. Worked out once a read and only for rows that changed, so a list
//! of 10,000 rows costs a frame nothing. How a place is drawn is the
//! presentation's.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dm_core::{Categories, Category, Entry, Id, State};

use super::names::name_of;

/// A place in the category tree: a filter over the one list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// Every download.
    All,
    /// One built-in category.
    Kind(Category),
    /// A category a person made, by name.
    Custom(String),
    /// The downloads not yet finished.
    Unfinished,
    /// The finished downloads.
    Finished,
    /// The downloads in the queue of this name.
    Queue(String),
}

impl Place {
    /// The place a word names, as the control port's `ui.place` gives it:
    /// `all`, `unfinished`, `finished`, `queue:<name>`, `custom:<name>` or
    /// `kind:<word>`. A queue or category that does not exist is refused: a
    /// place nothing is filed under showed an empty list as if it were one.
    pub fn from_word(
        word: &str,
        queues: &[String],
        customs: &[String],
    ) -> Result<Place, crate::Error> {
        Ok(match word {
            "all" => Place::All,
            "unfinished" => Place::Unfinished,
            "finished" => Place::Finished,
            other => {
                if let Some(name) = other.strip_prefix("queue:") {
                    if !queues.iter().any(|q| q == name) {
                        return Err(format!("no queue is called {name}").into());
                    }
                    Place::Queue(name.to_string())
                } else if let Some(name) = other.strip_prefix("custom:") {
                    if !customs.iter().any(|c| c == name) {
                        return Err(format!("no category is called {name}").into());
                    }
                    Place::Custom(name.to_string())
                } else if let Some(word) = other.strip_prefix("kind:") {
                    let kind = Category::ALL
                        .into_iter()
                        .find(|c| super::names::category_word(*c) == word)
                        .ok_or_else(|| format!("no category is called {word}"))?;
                    Place::Kind(kind)
                } else {
                    return Err(format!("no place is called {other}").into());
                }
            }
        })
    }
}

/// What the filter looks in: a row's name, address and note, lowercased.
pub fn haystack_named(name: &str, row: &Entry) -> String {
    format!("{name} {} {}", row.url, row.description).to_lowercase()
}

/// The category a person made that `row` belongs to, by the rule its file
/// was (or will be) placed by.
pub fn custom_of(row: &Entry, categories: &Categories) -> Option<String> {
    categories
        .custom_for(&name_of(row), &dm_core::host_of(&row.url))
        .map(|c| c.name.clone())
}

/// Each row's kind, category of a person's own and filter text, kept from
/// read to read.
#[derive(Debug, Default)]
pub struct Places {
    /// Each row's kind and category of a person's own.
    placed: HashMap<Id, (Option<Category>, Option<String>)>,
    /// Each row's name, address and note, lowercased once a read, for the
    /// filter: lowercasing them for every row on every frame made a frame
    /// with a filter typed take 36 ms at 5,000 rows.
    haystacks: HashMap<Id, String>,
    /// What each row's place and filter text were made from, so a read
    /// makes them again only for a row that changed. With the row they were
    /// made from: the same allocation again means the row did not change,
    /// and nothing of it is hashed.
    fingerprints: HashMap<Id, (u64, Arc<Entry>)>,
    /// The categories of a person's own that `placed` was made under.
    rules: String,
    /// Counts up with each read; the counts are kept for as long as it
    /// stands still.
    generation: u64,
    /// How many rows each place holds, for the generation counted at.
    counts: RefCell<(u64, HashMap<String, usize>)>,
    /// The filter's words, lowercased, and the filter they were made from.
    filter_words: RefCell<(String, Vec<String>)>,
}

impl Places {
    /// Places the rows of a new read: each row's kind and category of a
    /// person's own, worked out once (the places' counts asked for them per
    /// row, per place, per frame, and 500 rows went past the frame budget).
    pub fn place_rows(&mut self, rows: &[Arc<Entry>], categories: &Categories) {
        // Under other categories of a person's own, every row is placed
        // again; under the same, only those whose name, address, note or
        // kind changed (otherwise a read of the list four times a second
        // would parse and lowercase every row's, 5,000 of them).
        let rules = format!("{:?}", categories.custom);
        if rules != self.rules {
            self.rules = rules;
            self.fingerprints.clear();
        }
        self.generation += 1;
        for row in rows {
            self.place_one(row, categories);
        }
        // Rows that are gone.
        if self.placed.len() != rows.len() {
            let ids: HashSet<Id> = rows.iter().map(|r| r.id).collect();
            self.placed.retain(|id, _| ids.contains(id));
            self.haystacks.retain(|id, _| ids.contains(id));
            self.fingerprints.retain(|id, _| ids.contains(id));
        }
    }

    /// Places again only the rows of `rows` at the places `at`, the others as the last read
    /// placed them: the same rows in the same order, a few changed in place (a state, a name).
    /// At 10,000 rows, a download that finished placed every row again (gaps row 181). Under
    /// other categories of a person's own, every row is placed again ([`Self::place_rows`]).
    pub fn place_at(&mut self, rows: &[Arc<Entry>], at: &[usize], categories: &Categories) {
        if format!("{:?}", categories.custom) != self.rules {
            self.place_rows(rows, categories);
            return;
        }
        self.generation += 1;
        for row in at.iter().filter_map(|&i| rows.get(i)) {
            self.place_one(row, categories);
        }
    }

    /// Places `row`, where it is not the row placed last time and its name, address, note or
    /// kind changed.
    fn place_one(&mut self, row: &Arc<Entry>, categories: &Categories) {
        use std::hash::{Hash, Hasher};
        if self
            .fingerprints
            .get(&row.id)
            .is_some_and(|(_, seen)| Arc::ptr_eq(seen, row))
            && self.placed.contains_key(&row.id)
        {
            return;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (
            &row.url,
            &row.filename,
            &row.save_name,
            &row.description,
            row.category.map_or(255u8, |c| c as u8),
        )
            .hash(&mut hasher);
        let fingerprint = hasher.finish();
        if self.fingerprints.get(&row.id).map(|(f, _)| *f) == Some(fingerprint)
            && self.placed.contains_key(&row.id)
        {
            self.fingerprints
                .insert(row.id, (fingerprint, Arc::clone(row)));
            return;
        }
        // Each row's name worked out once: from its address, for a row
        // not yet named, it is a parse and a decode.
        let name = name_of(row);
        // The site only where a category of a person's own could ask:
        // without one, every address was parsed again for nothing.
        let custom = if categories.custom.is_empty() {
            None
        } else {
            categories
                .custom_for(&name, &dm_core::host_of(&row.url))
                .map(|c| c.name.clone())
        };
        let kind = dm_core::by_name(&name).or(row.category);
        self.haystacks.insert(row.id, haystack_named(&name, row));
        self.placed.insert(row.id, (kind, custom));
        self.fingerprints
            .insert(row.id, (fingerprint, Arc::clone(row)));
    }

    /// A row's kind and own category, from [`Self::place_rows`] where it
    /// has been placed.
    pub fn placed(
        &self,
        row: &Entry,
        categories: &Categories,
    ) -> (Option<Category>, Option<String>) {
        match self.placed.get(&row.id) {
            Some(placed) => placed.clone(),
            None => (super::names::kind_of(row), custom_of(row, categories)),
        }
    }

    /// Whether `row` belongs in `place`.
    pub fn within(&self, place: &Place, row: &Entry, categories: &Categories) -> bool {
        match place {
            Place::All => true,
            // A row whose kind is not known yet, and in no category of a
            // person's own, is Other: queued `.bin` rows were in no place
            // and the counts did not add up.
            Place::Kind(c) => match self.placed(row, categories) {
                (Some(kind), _) => kind == *c,
                (None, custom) => *c == Category::Other && custom.is_none(),
            },
            Place::Custom(name) => self.placed(row, categories).1.as_deref() == Some(name.as_str()),
            // Every row not whole yet: running, waiting, stopped by a person
            // or failed. So Unfinished and Finished add up to All: with only
            // running and waiting rows, a list of 45 with 9 failed and 20
            // stopped said Unfinished 0 (gaps row U1).
            Place::Unfinished => row.state != State::Completed,
            Place::Finished => row.state == State::Completed,
            Place::Queue(name) => &row.queue == name,
        }
    }

    /// Whether every word typed over the list (`filter`) is in the row's
    /// name, its address or its note, whatever their case.
    pub fn matches_filter(&self, filter: &str, row: &Entry) -> bool {
        if filter.trim().is_empty() {
            return true;
        }
        // The words made again only when the filter has changed.
        if self.filter_words.borrow().0 != filter {
            let words = filter.split_whitespace().map(str::to_lowercase).collect();
            *self.filter_words.borrow_mut() = (filter.to_string(), words);
        }
        let cached = self.filter_words.borrow();
        let found = |said: &str| cached.1.iter().all(|w| said.contains(w.as_str()));
        match self.haystacks.get(&row.id) {
            Some(said) => found(said),
            // A row the last read has not placed yet.
            None => found(&haystack_named(&name_of(row), row)),
        }
    }

    /// How many of `rows` are in `place`: counted once for each read, not
    /// once a frame for each place drawn.
    pub fn count(&self, place: &Place, rows: &[Arc<Entry>], categories: &Categories) -> usize {
        let mut cache = self.counts.borrow_mut();
        if cache.0 != self.generation {
            cache.0 = self.generation;
            cache.1.clear();
        }
        *cache.1.entry(format!("{place:?}")).or_insert_with(|| {
            rows.iter()
                .filter(|r| self.within(place, r, categories))
                .count()
        })
    }
}

/// What a place's right-click menu asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceAction {
    /// Runs this command, the place chosen first.
    Run(super::commands::Command),
    /// Options › Folders, where categories are added, edited and deleted.
    EditCategories,
    /// The folder the place's files go to ([`folder_of`]).
    OpenFolder,
}

/// A place's right-click menu, in order: its words (English, as the catalogs are keyed) and
/// what each does. The same in both windows.
pub fn place_menu(place: &Place) -> Vec<(&'static str, PlaceAction)> {
    use super::commands::Command;
    match place {
        Place::All | Place::Kind(_) | Place::Custom(_) => vec![
            ("Open folder", PlaceAction::OpenFolder),
            ("Add or edit categories…", PlaceAction::EditCategories),
        ],
        Place::Unfinished => vec![
            ("Resume all", PlaceAction::Run(Command::ResumeAll)),
            ("Stop All", PlaceAction::Run(Command::StopAll)),
        ],
        Place::Finished => vec![(
            "Delete Completed",
            PlaceAction::Run(Command::DeleteCompleted),
        )],
        Place::Queue(_) => vec![
            ("Start Queue", PlaceAction::Run(Command::StartQueue)),
            ("Stop Queue", PlaceAction::Run(Command::StopQueue)),
            ("Scheduler…", PlaceAction::Run(Command::Scheduler)),
        ],
    }
}

/// Every place menu's words, for the catalogs' test.
pub fn place_menu_words() -> Vec<&'static str> {
    [
        Place::All,
        Place::Unfinished,
        Place::Finished,
        Place::Queue(String::new()),
    ]
    .iter()
    .flat_map(place_menu)
    .map(|(words, _)| words)
    .collect()
}

/// The folder a place's files go to: a kind's or a person's own category's, else the download
/// folder.
pub fn folder_of(
    place: &Place,
    download_dir: &std::path::Path,
    categories: &Categories,
) -> std::path::PathBuf {
    match place {
        Place::Kind(kind) => categories.destination(download_dir, *kind),
        Place::Custom(name) => download_dir.join(name),
        _ => download_dir.to_path_buf(),
    }
}

#[cfg(test)]
mod tests;
