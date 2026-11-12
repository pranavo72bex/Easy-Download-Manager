// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Stepping through the list by keyboard: Up, Down, Home, End, Page Up and Page Down, Shift
//! extending the run from where it started. Both windows' (gaps row U6).

use std::collections::BTreeSet;

use dm_core::Id;

/// A move of the keyboard's place in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// One row up.
    Up,
    /// One row down.
    Down,
    /// To the first row.
    First,
    /// To the last row.
    Last,
    /// Page Up and Page Down: this many rows.
    PageUp(usize),
    /// Page Down: this many rows.
    PageDown(usize),
}

/// The selection after Up, Down, Home or End, the row a later Shift
/// extends from, and the row now moved to. From the row last moved to or
/// clicked; with nothing there yet, Down starts at the top and Up at the
/// bottom. Shift selects the run from where it was started to the row
/// moved to.
pub fn stepped(
    rows: &[Id],
    selected: &BTreeSet<Id>,
    anchor: Option<Id>,
    cursor: Option<Id>,
    step: Step,
    shift: bool,
) -> (BTreeSet<Id>, Option<Id>, Option<Id>) {
    let Some(last) = rows.len().checked_sub(1) else {
        return (selected.clone(), anchor, cursor);
    };
    let from = cursor
        .and_then(|c| rows.iter().position(|r| *r == c))
        .or_else(|| rows.iter().position(|r| selected.contains(r)));
    let to = match (step, from) {
        (Step::First, _) => 0,
        (Step::Last, _) => last,
        (Step::Up, Some(at)) => at.saturating_sub(1),
        (Step::Down, Some(at)) => (at + 1).min(last),
        (Step::PageUp(n), Some(at)) => at.saturating_sub(n.max(1)),
        (Step::PageDown(n), Some(at)) => (at + n.max(1)).min(last),
        (Step::Up | Step::PageUp(_), None) => last,
        (Step::Down | Step::PageDown(_), None) => 0,
    };
    let Some(&id) = rows.get(to) else {
        return (selected.clone(), anchor, cursor);
    };
    if shift {
        let start = anchor
            .and_then(|a| rows.iter().position(|r| *r == a))
            .or(from)
            .unwrap_or(to);
        let (a, b) = if start <= to {
            (start, to)
        } else {
            (to, start)
        };
        let run = rows
            .get(a..=b)
            .unwrap_or_default()
            .iter()
            .copied()
            .collect();
        (run, rows.get(start).copied(), Some(id))
    } else {
        (BTreeSet::from([id]), Some(id), Some(id))
    }
}
