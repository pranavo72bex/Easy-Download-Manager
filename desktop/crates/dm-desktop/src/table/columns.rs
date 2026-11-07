// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Dragging columns and rows, and the widths the columns start at.

use super::*;

/// What a drag starting on row `i` carries: the selection's finished files
/// when the row is in it, else that row's alone — each one still where dm
/// put it. Nothing for a download that has not finished.
/// A row dragged up or down the list: a line where it will go while the
/// button is held, and moved there in the queue's order when let go.
pub(super) fn reorder(app: &mut DmApp, ui: &egui::Ui, clip: Option<egui::Rect>) {
    let Some(moving) = app.reordering else {
        return;
    };
    let (at, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
    // Escape lets go without moving it, as with any drag.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.reordering = None;
        return;
    }
    let place = |id: &Id| app.rows.iter().position(|r| r.id == *id);
    let over = at.and_then(|at| {
        app.row_rects
            .iter()
            .find(|(_, rect)| rect.y_range().contains(at.y))
            .map(|(id, rect)| (*id, *rect))
    });
    let target = over.filter(|(id, _)| *id != moving);
    if down {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let (Some((id, rect)), Some(from)) = (target, place(&moving)) {
            // Below the row when moving down, above it when moving up:
            // where the row will be once let go.
            let y = if place(&id).is_some_and(|to| to > from) {
                rect.bottom()
            } else {
                rect.top()
            };
            let tokens = crate::theme::Tokens::get(ui.ctx());
            let painter = ui
                .ctx()
                .layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::unique("dm-reorder"),
                ))
                .with_clip_rect(clip.unwrap_or(rect));
            // Thick enough to be seen as the place it goes.
            painter.hline(rect.x_range(), y, egui::Stroke::new(3.0, tokens.progress()));
        }
        return;
    }
    app.reordering = None;
    if let (Some((id, _)), Some(from)) = (target, place(&moving)) {
        if let Some(to) = place(&id) {
            app.backend.shift(moving, to as isize - from as isize);
            app.refresh();
        }
    }
}

pub(super) fn files_to_drag(
    app: &DmApp,
    rows: &[std::sync::Arc<Entry>],
    i: usize,
) -> Vec<std::path::PathBuf> {
    let Some(start) = rows.get(i) else {
        return Vec::new();
    };
    let carried: Vec<&Entry> = if app.selected.contains(&start.id) {
        rows.iter()
            .map(|r| &**r)
            .filter(|r| app.selected.contains(&r.id))
            .collect()
    } else {
        vec![start]
    };
    carried
        .into_iter()
        // Finished, and its file still there (not `gone`).
        .filter(|r| r.state == State::Completed && !r.path.is_empty() && !gone(r))
        .map(|r| std::path::PathBuf::from(&r.path))
        .filter(|p| p.is_absolute())
        .collect()
}

/// How wide the file name starts in `room`: its 220 points where the
/// columns up to Speed fit beside it, else up to Time left, else up to
/// Status, giving way for the first of these that leaves it 120. At the
/// window's first size Time left read "Time le" and its numbers one digit.
pub fn name_width(columns: &[Column], room: f32) -> f32 {
    let beside = |through: Column| -> Option<f32> {
        let last = columns.iter().position(|c| *c == through)?;
        Some(
            columns
                .iter()
                .take(last + 1)
                .filter(|c| **c != Column::Name)
                .map(|c| initial(*c) + 8.0)
                .sum(),
        )
    };
    let fits = |others: f32| room - others - 24.0;
    [Column::Rate, Column::TimeLeft, Column::Status]
        .into_iter()
        .filter_map(beside)
        .map(fits)
        .find(|&name| name >= 120.0)
        .unwrap_or(120.0)
        .min(220.0)
}

/// Where each column but the name starts, in points.
pub(super) fn initial(column: Column) -> f32 {
    match column {
        Column::Name => 180.0,
        // Reference A's Q: a mark for a download in a queue, the queue's
        // name on hover, so the column takes no room from the rest.
        Column::Queue => 28.0,
        // Reference A's widths, near enough: its eight columns fit an
        // 800-pixel window; dm's Status alone was 196 and pushed Transfer
        // rate, Last Try Date and Description out of sight.
        Column::Size => 70.0,
        Column::Status => 120.0,
        Column::TimeLeft => 70.0,
        Column::Rate => 84.0,
        Column::LastTry => 96.0,
        Column::Description | Column::Referrer | Column::Folder => 140.0,
    }
}

/// How wide a column starts.
pub(super) fn width(column: Column) -> Width {
    match column {
        Column::Name => Width::initial(initial(Column::Name))
            .at_least(120.0)
            .clip(true),
        Column::Queue => Width::initial(initial(Column::Queue))
            .at_least(24.0)
            .clip(true),
        Column::Size => Width::initial(initial(Column::Size)),
        Column::Status => Width::initial(initial(Column::Status)).clip(true),
        Column::TimeLeft => Width::initial(initial(Column::TimeLeft)),
        Column::Rate => Width::initial(initial(Column::Rate)),
        Column::LastTry => Width::initial(initial(Column::LastTry)),
        Column::Description => Width::initial(initial(Column::Description)).clip(true),
        Column::Referrer => Width::initial(initial(Column::Referrer)).clip(true),
        Column::Folder => Width::initial(initial(Column::Folder)).clip(true),
    }
}
