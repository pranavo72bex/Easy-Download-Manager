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
            Column::Name => "File Name",
            // Its heading draws the queue's mark; this is its name in the
            // columns menu and for a screen reader.
            Column::Queue => "Queue",
            Column::Size => "Size",
            Column::Status => "Status",
            Column::TimeLeft => "Time left",
            Column::Rate => "Transfer rate",
            Column::LastTry => "Last Try Date",
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A heading or View › Sort by sorts up, then down, then back in the
    /// order the downloads were added (gaps row U29).
    #[test]
    fn a_third_choice_puts_the_list_back_in_the_order_added() {
        let up = next_sort(None, Column::Size);
        assert_eq!(up, Some((Column::Size, true)));
        let down = next_sort(up, Column::Size);
        assert_eq!(down, Some((Column::Size, false)));
        assert_eq!(next_sort(down, Column::Size), None);
        // Another column starts at up, whichever way the list was sorted.
        assert_eq!(next_sort(down, Column::Name), Some((Column::Name, true)));
    }

    /// The sort is kept in the settings and read back as it was; settings
    /// saved before it was kept load with the list in the order added.
    #[test]
    fn the_sort_kept_is_the_sort_read_back() {
        let mut settings = dm_shell::settings::Settings::default();
        assert_eq!(kept_sort(&settings), None);
        for sort in [
            Some((Column::Rate, false)),
            Some((Column::Name, true)),
            None,
        ] {
            keep_sort(&mut settings, sort);
            assert_eq!(kept_sort(&settings), sort);
        }
        let old: dm_shell::settings::Settings =
            serde_json::from_str("{}").expect("settings with nothing in them load");
        assert_eq!(kept_sort(&old), None);
        assert!(old.sort_ascending);
    }

    #[test]
    fn columns_sort_by_what_they_show() {
        use dm_core::State;
        use std::cmp::Ordering::*;
        let named = |name: &str| -> Entry {
            serde_json::from_value(serde_json::json!({
                "id": 1,
                "url": "https://e.com/f.bin",
                "state": serde_json::to_value(State::Paused).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": name,
            }))
            .unwrap()
        };
        // Names: numbers by value.
        assert_eq!(
            compare(Column::Name, &named("part 2"), &named("part 10")),
            Less
        );
        // Size: what has arrived of a file of unknown length is its size.
        let (mut big, mut small) = (named("a"), named("b"));
        big.progress.downloaded = 5000;
        small.progress.total = Some(10);
        assert_eq!(compare(Column::Size, &big, &small), Greater);
        // Speed and time left: a stopped row's last figures are not shown,
        // and do not sort it ahead of one that is running.
        let (mut stopped, mut running) = (named("a"), named("b"));
        stopped.progress.speed = 9e9;
        stopped.progress.eta = Some(std::time::Duration::from_secs(1));
        running.state = State::Running;
        running.progress.speed = 100.0;
        running.progress.eta = Some(std::time::Duration::from_secs(500));
        assert_eq!(compare(Column::Rate, &stopped, &running), Less);
        assert_eq!(compare(Column::TimeLeft, &stopped, &running), Less);
    }
}
