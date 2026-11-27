// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The download table's columns, as both windows show them: each one's
//! key in the settings, the order a person chose, its heading, and how
//! rows sort by it.

use dm_core::{Entry, State};
use dm_shell::app::names::name_of;

use crate::format;
use crate::i18n::t;

/// A column of the download table, in its default order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    /// The file's name.
    Name,
    /// The queue the download is in.
    Queue,
    /// The size.
    Size,
    /// The status.
    Status,
    /// The time left.
    TimeLeft,
    /// The transfer rate.
    Rate,
    /// The time of the last try.
    LastTry,
    /// The description.
    Description,
    /// The page a browser handed the download over from.
    Referrer,
    /// Where the finished file is, or where it will go.
    Folder,
}

impl Column {
    /// In the order a new window shows them, reference A's: File Name, Q
    /// (narrow), Size, Status, Time left, Transfer rate, Last Try Date and
    /// Description (decisions.md, "dm works exactly like reference A").
    pub const ALL: [Column; 10] = [
        Column::Name,
        Column::Queue,
        Column::Size,
        Column::Status,
        Column::TimeLeft,
        Column::Rate,
        Column::LastTry,
        Column::Description,
        Column::Referrer,
        Column::Folder,
    ];

    /// What the settings file calls it.
    pub fn key(self) -> &'static str {
        match self {
            Column::Name => "name",
            Column::Queue => "queue",
            Column::Size => "size",
            Column::Status => "status",
            Column::TimeLeft => "time_left",
            Column::Rate => "rate",
            Column::LastTry => "last_try",
            Column::Description => "description",
            Column::Referrer => "referrer",
            Column::Folder => "folder",
        }
    }

    /// The column saved under `key`, if there is one.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|c| c.key() == key)
    }

    /// Every column in the order a person chose, the file name first and
    /// any the list does not mention after, in their usual place.
    pub fn ordered(keys: &[String]) -> Vec<Column> {
        let mut order = vec![Column::Name];
        for column in keys.iter().filter_map(|k| Column::from_key(k)) {
            if !order.contains(&column) {
                order.push(column);
            }
        }
        for column in Column::ALL {
            if !order.contains(&column) {
                order.push(column);
            }
        }
        order
    }

    /// Its heading, in the language in force.
    pub fn title(self) -> &'static str {
        t(self.english())
    }

    /// Its heading in English, which the catalogs are keyed by.
    pub fn english(self) -> &'static str {
        match self {
            Column::Name => "File name",
            // Its heading draws the queue's mark; this is its name in the
            // columns menu and for a screen reader.
            Column::Queue => "Queue",
            Column::Size => "Size",
            Column::Status => "Status",
            Column::TimeLeft => "Time left",
            Column::Rate => "Transfer rate",
            Column::LastTry => "Last try date",
            Column::Description => "Description",
            Column::Referrer => "Referrer",
            Column::Folder => "Folder",
        }
    }
}

/// The list's sort: the column and whether up; none for the order the
/// downloads were added in (the queue's own).
pub type Sort = Option<(Column, bool)>;

/// The sort after a heading's click or View › Sort by's choice of `column`:
/// up, then the other way, then back in the order the downloads were added
/// (a list once sorted could not go back to it, where rows are dragged into
/// another order). Another column starts again at up.
#[must_use]
pub fn next_sort(now: Sort, column: Column) -> Sort {
    match now {
        Some((c, true)) if c == column => Some((c, false)),
        Some((c, false)) if c == column => None,
        _ => Some((column, true)),
    }
}

/// The sort kept in `settings`, so the list is sorted the same way at the
/// next launch: the column's key and which way, or no key for none.
pub fn keep_sort(settings: &mut dm_shell::settings::Settings, sort: Sort) {
    match sort {
        Some((column, ascending)) => {
            settings.sort_column = column.key().to_string();
            settings.sort_ascending = ascending;
        }
        None => settings.sort_column.clear(),
    }
}

/// The sort `settings` keep: what [`keep_sort`] wrote, read back at launch.
#[must_use]
pub fn kept_sort(settings: &dm_shell::settings::Settings) -> Sort {
    Column::from_key(&settings.sort_column).map(|c| (c, settings.sort_ascending))
}

/// How two rows compare under a column, for sorting.
pub fn compare(column: Column, a: &Entry, b: &Entry) -> std::cmp::Ordering {
    match column {
        // "part 2" before "part 10", as a person reads them.
        Column::Name => format::natural(&name_of(a), &name_of(b)),
        Column::Queue => a.queue.cmp(&b.queue),
        // By the size the column shows, which for a file of unknown length
        // is what has arrived of it.
        Column::Size => crate::rows::shown_size(a).cmp(&crate::rows::shown_size(b)),
        // By state, then how far along: the same order in every language,
        // and 9% before 45% rather than after it as text would put it.
        Column::Status => {
            let rank = |e: &Entry| match e.state {
                State::Running => 0,
                State::Queued => 1,
                State::Paused => 2,
                State::Failed(_) => 3,
                State::Cancelled => 4,
                State::Completed => 5,
            };
            rank(a).cmp(&rank(b)).then_with(|| {
                a.progress
                    .fraction()
                    .partial_cmp(&b.progress.fraction())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        }
        // Only a running row shows a time left or a speed; a stopped one's
        // last figures are stale, and sorted as the blank they are shown as.
        Column::TimeLeft => {
            let left = |e: &Entry| {
                (e.state == State::Running)
                    .then_some(e.progress.eta)
                    .flatten()
                    .filter(|d| !d.is_zero())
            };
            left(a).cmp(&left(b))
        }
        Column::Rate => {
            let speed = |e: &Entry| {
                (e.state == State::Running && e.progress.speed > 1.0).then_some(e.progress.speed)
            };
            speed(a)
                .partial_cmp(&speed(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        Column::LastTry => a.finished_at.cmp(&b.finished_at),
        Column::Description => a.description.cmp(&b.description),
        Column::Referrer => a.referrer.cmp(&b.referrer),
        Column::Folder => crate::rows::folder_of(a).cmp(&crate::rows::folder_of(b)),
    }
}

/// The heading a row stands under where the list is sorted by `column`, as Finder groups a
/// sorted list: by its state under Status, by the day it was last tried under Last try date
/// (`today` the local day), by its queue under Queue; none under the other columns, whose
/// groups would be a row each. Each group is a run of rows under the sort, either way up: the
/// heading follows from what `compare` sorts by.
#[must_use]
pub fn group_of(column: Column, row: &Entry, today: dm_core::clock::Date) -> Option<String> {
    let words = match column {
        Column::Status => match row.state {
            State::Running => "Downloading",
            State::Queued => "Queued",
            State::Paused => "Stopped",
            State::Failed(_) => "Failed",
            State::Cancelled => "Cancelled",
            State::Completed => "Finished",
        },
        Column::LastTry => match row
            .finished_at
            .and_then(|at| dm_core::clock::local_at(i64::try_from(at).ok()?))
        {
            None => "Unfinished",
            Some(at) => match today.days_since_epoch() - at.date.days_since_epoch() {
                ..=0 => "Today",
                1 => "Yesterday",
                2..=7 => "Previous 7 days",
                8..=30 => "Previous 30 days",
                _ => "Earlier",
            },
        },
        Column::Queue if row.queue.is_empty() => "No queue",
        Column::Queue => return Some(row.queue.clone()),
        _ => return None,
    };
    Some(t(words).to_string())
}

#[cfg(test)]
mod tests;
