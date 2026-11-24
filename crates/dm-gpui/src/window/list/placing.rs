// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a read works out from the rows beside their figures (gaps rows 181 and S16): the
//! queues they name, the name each file will get and each row's place. Every row's, where
//! rows came, went or moved; where the same rows stand in the same order and a few changed in
//! place (a download started, stopped or finished, a row renamed or moved to another queue),
//! only what follows from those few. Before the first frame only the rows it shows are named,
//! and the rest once it is drawn.

use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use dm_core::{Entry, State};
use dm_shell::app::names::name_of;

use super::Downloads;

/// How many rows are named before the first frame where the list holds more: more than the
/// tallest screen shows at the smallest text size (gaps row S16).
const FIRST_NAMED: usize = 200;

impl Downloads {
    /// Every row's queue, name and place worked out again, and the choice kept to the rows
    /// still listed: the rows came, went or moved, or the settings changed. Before the first
    /// frame, a long list's rows are named once the rows it shows are known
    /// (`name_first_screenful`), the rest after it (`name_the_rest`): at 10,000 rows, naming
    /// every one held the first frame back 35 ms (gaps row S16).
    pub(super) fn place_every_row(&mut self, settings: &dm_shell::Settings) {
        self.known.named_at = Some(Instant::now());
        // The queues the rows name, for Start queue's and Stop queue's
        // arrows, the menu bar and the side panel: worked out from the
        // rows where they changed, not asked of the engine at each read
        // and each tick, a pass over every row under the queue's lock
        // each time (gaps row 181).
        self.known.queue_list = dm_core::queue_names(self.rows.iter().map(|r| &**r));
        // Each row by the name its file will get: a waiting second
        // copy reads "name (1)".
        self.known.earlier = dm_shell::app::names::earlier_of_the_same_name(&self.rows);
        self.known.shown_names.clear();
        if crate::port::FRAMES.load(Ordering::Relaxed) == 0 && self.rows.len() > FIRST_NAMED {
            self.known.names_owed = true;
        } else {
            let every: Vec<usize> = (0..self.rows.len()).collect();
            self.name_rows(&every, settings);
            self.known.names_owed = false;
        }
        dm_desktop_base::rows::set_superseded(&self.rows);
        // A row taken off the list is no longer chosen.
        let ids: std::collections::HashSet<dm_core::Id> = self.rows.iter().map(|r| r.id).collect();
        self.list.selected.retain(|id| ids.contains(id));
        self.list
            .places
            .place_rows(&self.rows, &settings.categories());
    }

    /// The rows at the places `restated`, each with the row it was, changed in place in more
    /// than their figures, the other rows as they were: only what follows from them is worked
    /// out again. They are named again, and so are the rows not yet placed of a name and folder
    /// one of them had or has where its naming moved (it settled, was placed, renamed or sent
    /// to another folder), and those whose count of earlier rows of their name moved; they are
    /// placed again; the queues are worked out again where one moved to another queue, and the
    /// files taken by a later row where a finished one changed. At 10,000 rows, each download
    /// that started, stopped or finished named and placed every row again (gaps row 181).
    /// Whether one of them came into or left the rows shown.
    pub(super) fn restate(
        &mut self,
        restated: &[(usize, Arc<Entry>)],
        settings: &dm_shell::Settings,
    ) -> bool {
        let mut to_name: Vec<usize> = Vec::with_capacity(restated.len());
        let mut named: Vec<(Option<String>, String)> = Vec::new();
        let (mut queues_moved, mut finished_moved) = (false, false);
        for (at, old) in restated {
            let Some(new) = self.rows.get(*at) else {
                continue;
            };
            to_name.push(*at);
            queues_moved |= old.queue != new.queue;
            finished_moved |= old.state == State::Completed || new.state == State::Completed;
            if !named_alike(old, new) {
                named.push((old.save_dir.clone(), name_of(old)));
                named.push((new.save_dir.clone(), name_of(new)));
            }
        }
        if queues_moved {
            self.known.queue_list = dm_core::queue_names(self.rows.iter().map(|r| &**r));
        }
        if !named.is_empty() {
            let (earlier, alike) = dm_shell::app::names::earlier_and_alike(&self.rows, &named);
            to_name.extend(
                self.rows
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| earlier.get(&row.id) != self.known.earlier.get(&row.id))
                    .map(|(at, _)| at),
            );
            to_name.extend(alike);
            self.known.earlier = earlier;
        }
        to_name.sort_unstable();
        to_name.dedup();
        self.name_rows(&to_name, settings);
        if finished_moved {
            dm_desktop_base::rows::set_superseded(&self.rows);
        }
        let at: Vec<usize> = restated.iter().map(|(at, _)| *at).collect();
        let categories = settings.categories();
        self.list.places.place_at(&self.rows, &at, &categories);
        at.iter().filter_map(|&i| self.rows.get(i)).any(|row| {
            let shows = self.list.places.within(&self.list.place, row, &categories)
                && self.list.places.matches_filter(&self.list.filter_text, row);
            shows != self.list.positions.contains_key(&row.id)
        })
    }

    /// Before the first frame, the rows it can show named: the first rows of the list as shown,
    /// those chosen and those running (a progress window may open before it).
    pub(super) fn name_first_screenful(&mut self) {
        let settings = Rc::clone(&self.known.drawn);
        let mut first: Vec<usize> = self.list.shown.iter().take(FIRST_NAMED).copied().collect();
        first.extend(self.rows_seen());
        first.extend(
            self.rows
                .iter()
                .enumerate()
                .filter(|(_, row)| row.state == State::Running)
                .map(|(at, _)| at),
        );
        first.sort_unstable();
        first.dedup();
        self.name_rows(&first, &settings);
    }

    /// The rows not named before the first frame named, once it is drawn, and before anything
    /// reads every row's name (the control port's answers).
    pub(crate) fn name_the_rest(&mut self) {
        if !std::mem::take(&mut self.known.names_owed) {
            return;
        }
        let settings = Rc::clone(&self.known.drawn);
        let rest: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| !self.known.shown_names.contains_key(&row.id))
            .map(|(at, _)| at)
            .collect();
        self.name_rows(&rest, &settings);
    }
}

/// Whether the other rows' names stand beside `new` as they stood beside `old`: they follow a
/// row through the count of earlier rows not yet placed of its name and folder
/// (`earlier_of_the_same_name`), and through its file once it is placed.
fn named_alike(old: &Entry, new: &Entry) -> bool {
    old.state.settled() == new.state.settled()
        && old.path == new.path
        && old.save_dir == new.save_dir
        && name_of(old) == name_of(new)
}

#[cfg(test)]
mod tests {
    use super::named_alike;
    use dm_core::{Entry, State};

    fn row(state: State, path: &str, filename: &str) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "https://e.com/big.bin",
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": filename,
            "path": path,
        }))
        .unwrap()
    }

    /// gaps row 181: a download started or stopped leaves the other rows' names as they were;
    /// one that finished, was placed or renamed may change them.
    #[test]
    fn only_a_row_settled_placed_or_renamed_moves_the_others_names() {
        let queued = row(State::Queued, "", "");
        assert!(named_alike(&queued, &row(State::Running, "", "")));
        assert!(named_alike(&queued, &row(State::Paused, "", "")));
        assert!(named_alike(&queued, &row(State::Running, "", "big.bin")));
        assert!(!named_alike(
            &queued,
            &row(State::Completed, "/d/big.bin", "big.bin")
        ));
        assert!(!named_alike(
            &queued,
            &row(State::Failed("x".into()), "", "")
        ));
        assert!(!named_alike(&queued, &row(State::Queued, "", "small.bin")));
    }
}
