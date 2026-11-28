// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list's section headings, as Finder groups a sorted list (the maintainer's choice of
//! 2026-11-26, "Section headings"): sorted by Status, Last try date or Queue, a heading stands
//! over each run of rows of one state, day or queue (`columns::group_of`). The rows stay one
//! flat list (`ListView::shown`): the keys, Select all, a screen reader's rows and the chosen
//! row know nothing of the headings, which are lines of the table between them. A line is a
//! heading or a row, each a row's height, so the table is still a list of lines of one height;
//! these turn a row's place in the list into its line and back. A phone's cards have none.

use super::Downloads;

/// A line of the table: a heading, or the row at a place in `shown` and its place under its
/// heading (for the stripes, which start again under each heading).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Line {
    Heading(usize),
    Row { at: usize, nth: usize },
}

impl Downloads {
    /// The headings drawn, each with the place in `shown` of its first row: none on a phone.
    pub(crate) fn headings(&self) -> &[(usize, gpui::SharedString)] {
        if self.layout.narrow.laid_out {
            &[]
        } else {
            &self.list.headings
        }
    }

    /// How many lines the table has: its rows and its headings.
    pub(crate) fn line_count(&self) -> usize {
        self.list.shown.len() + self.headings().len()
    }

    /// The line the row at `at` in `shown` is drawn on.
    pub(crate) fn line_of(&self, at: usize) -> usize {
        at + self.headings().partition_point(|h| h.0 <= at)
    }

    /// What is drawn on line `line`; none past the end.
    pub(crate) fn line(&self, line: usize) -> Option<Line> {
        let headings = self.headings();
        // The headings at or above it: heading k stands on line `its first row + k`.
        let above = headings
            .iter()
            .enumerate()
            .take_while(|(k, h)| h.0 + k <= line)
            .count();
        let group_from = match above.checked_sub(1) {
            Some(k) if headings[k].0 + k == line => return Some(Line::Heading(k)),
            Some(k) => headings[k].0,
            None => 0,
        };
        let at = line - above;
        (at < self.list.shown.len()).then_some(Line::Row {
            at,
            nth: at - group_from,
        })
    }

    /// The rows in `shown` drawn on `lines`.
    pub(crate) fn rows_on(&self, lines: std::ops::Range<usize>) -> std::ops::Range<usize> {
        let rows_before = |line: usize| {
            let headings = self.headings();
            let above = headings
                .iter()
                .enumerate()
                .take_while(|(k, h)| h.0 + k < line)
                .count();
            (line - above).min(self.list.shown.len())
        };
        rows_before(lines.start)..rows_before(lines.end).max(rows_before(lines.start))
    }

    /// The headings of the list as now sorted: where its sort groups rows, one wherever the
    /// group changes down `shown`.
    pub(super) fn group(&mut self) {
        let Some((column, _)) = self.list.sort else {
            self.list.headings.clear();
            return;
        };
        let today = edm_core::clock::local_now().date;
        let mut headings: Vec<(usize, gpui::SharedString)> = Vec::new();
        let mut last: Option<String> = None;
        for (at, &r) in self.list.shown.iter().enumerate() {
            let Some(row) = self.rows.get(r) else {
                continue;
            };
            let Some(group) = edm_desktop_base::columns::group_of(column, row, today) else {
                // A column that does not group: no heading anywhere.
                self.list.headings.clear();
                return;
            };
            if last.as_ref() != Some(&group) {
                headings.push((at, group.clone().into()));
                last = Some(group);
            }
        }
        self.list.headings = headings;
    }
}
