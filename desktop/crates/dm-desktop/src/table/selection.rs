// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Which rows are selected, and stepping through them by keyboard.

use super::*;

pub(super) fn select(
    app: &mut DmApp,
    rows: &[std::sync::Arc<Entry>],
    i: usize,
    modifiers: egui::Modifiers,
) {
    let ids: Vec<Id> = rows.iter().map(|r| r.id).collect();
    (app.selected, app.anchor) = selection(&ids, &app.selected, app.anchor, i, modifiers);
    app.cursor = ids.get(i).copied();
}

/// Stepping through the list by keyboard, shared with the GPUI window.
pub use dm_desktop_base::selection::{stepped, Step};

/// The rows selected after a click on the `i`th, and the row a later
/// shift-click extends from. That is the row last clicked without shift,
/// as every file list has it; extending from the topmost selected row
/// turned "click row 5, shift-click row 8" into rows 1 to 8 whenever row 1
/// was already ticked.
pub fn selection(
    rows: &[Id],
    selected: &BTreeSet<Id>,
    anchor: Option<Id>,
    i: usize,
    modifiers: egui::Modifiers,
) -> (BTreeSet<Id>, Option<Id>) {
    let Some(&id) = rows.get(i) else {
        return (selected.clone(), anchor);
    };
    if modifiers.command {
        let mut now = selected.clone();
        if !now.remove(&id) {
            now.insert(id);
        }
        (now, Some(id))
    } else if modifiers.shift && !selected.is_empty() {
        let from = anchor
            .and_then(|a| rows.iter().position(|r| *r == a))
            .or_else(|| rows.iter().position(|r| selected.contains(r)))
            .unwrap_or(i);
        let (a, b) = if from <= i { (from, i) } else { (i, from) };
        let now = rows
            .get(a..=b)
            .unwrap_or_default()
            .iter()
            .copied()
            .collect();
        (now, anchor.or(Some(id)))
    } else {
        (BTreeSet::from([id]), Some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_list_keeps_the_selection_as_it_was() {
        // A click past the last row changes nothing (the keys' half of
        // this is `dm_desktop_base::selection`'s).
        let kept = BTreeSet::from([7]);
        assert_eq!(
            selection(&[1, 2], &kept, Some(7), 5, egui::Modifiers::NONE),
            (kept, Some(7))
        );
    }
}
