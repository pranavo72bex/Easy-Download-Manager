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

/// A move of the keyboard's place in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
    First,
    Last,
    /// Page Up and Page Down: this many rows.
    PageUp(usize),
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
