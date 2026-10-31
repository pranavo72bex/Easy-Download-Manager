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
        Column::Name => 220.0,
        // Room for "Main queue" (60 read "Main q…"), and one
        // width whether or not a row has a queue yet: it started at 96 in
        // an empty list and 140 in one with a queue, so a relaunch moved
        // every column after it and cut "Time left".
        Column::Queue => 116.0,
        Column::Size => 76.0,
        // "Waiting for Start queue" whole, with its shape.
        Column::Status => 196.0,
        Column::TimeLeft => 88.0,
        Column::Rate => 92.0,
        Column::LastTry => 100.0,
        Column::Description | Column::Referrer | Column::Folder => 160.0,
    }
}

/// How wide a column starts.
pub(super) fn width(column: Column) -> Width {
    match column {
        Column::Name => Width::initial(220.0).at_least(120.0).clip(true),
        Column::Queue => Width::initial(initial(Column::Queue)).clip(true),
        Column::Size => Width::initial(76.0),
        Column::Status => Width::initial(initial(Column::Status)).clip(true),
        Column::TimeLeft => Width::initial(88.0),
        Column::Rate => Width::initial(92.0),
        Column::LastTry => Width::initial(100.0),
        Column::Description => Width::initial(160.0).clip(true),
        Column::Referrer => Width::initial(160.0).clip(true),
        Column::Folder => Width::initial(160.0).clip(true),
    }
}
