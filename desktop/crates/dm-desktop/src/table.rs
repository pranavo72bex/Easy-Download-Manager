//! The download list: one row a download, in columns, sorted by clicking a heading, selected by clicking a row (Ctrl or Cmd to
//! add, Shift for a run), a menu on the right button, and a double-click to
//! open a row's progress, or its properties once finished.

use crate::i18n::t;
use std::collections::BTreeSet;

use dm_core::{Entry, Id, State};
use egui::RichText;
use egui_extras::{Column as Width, TableBuilder};

use crate::app::{name_of, status_of, Column, DmApp};
use crate::commands::Command;
use crate::format;

pub fn show(app: &mut DmApp, ui: &mut egui::Ui) {
    // A click on a row's text selects the row. Selectable text took the
    // click for itself, so clicking a file's name selected nothing.
    ui.style_mut().interaction.selectable_labels = false;
    let rows = app.visible();
    if rows.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(if app.rows.is_empty() {
                    t("Nothing queued")
                } else {
                    t("Nothing here")
                })
                .size(16.0),
            );
            ui.label(
                RichText::new(if app.rows.is_empty() {
                    t("Add URL, paste a link, or send one from your browser.")
                } else {
                    t("No downloads in this category.")
                })
                .weak(),
            );
        });
        // The headings still show, so the columns are always there to read.
        header_only(app, ui);
        return;
    }

    let mut clicked: Option<(usize, egui::Modifiers)> = None;
    let mut double: Option<usize> = None;
    let mut menu: Option<usize> = None;
    let mut dragged: Option<usize> = None;
    let mut drawn: Vec<(dm_core::Id, egui::Rect)> = Vec::new();

    let columns: Vec<Column> = app
        .column_order
        .iter()
        .copied()
        .filter(|c| !app.hidden_columns.contains(c))
        .collect();
    // Plain rows; the modern look's are taller.
    let striped = false;
    let row_height = if crate::theme::Tokens::get(ui.ctx()).modern {
        36.0
    } else {
        22.0
    };
    let mut table = TableBuilder::new(ui)
        .striped(striped)
        .resizable(true)
        // Dragged as well as clicked: a finished file is dragged out of the
        // list into a folder, a mail or a chat.
        .sense(egui::Sense::click_and_drag())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    for (n, column) in columns.iter().enumerate() {
        table = table.column(if n + 1 == columns.len() {
            Width::remainder().at_least(80.0).clip(true)
        } else {
            width(*column)
        });
    }
    table
        .header(22.0, |mut header| {
            for column in &columns {
                header.col(|ui| heading(app, ui, *column));
            }
        })
        .body(|body| {
            body.rows(row_height, rows.len(), |mut row| {
                let i = row.index();
                let entry = &rows[i];
                row.set_selected(app.selected.contains(&entry.id));
                for column in &columns {
                    row.col(|ui| cell(ui, *column, entry));
                }
                let response = row.response();
                drawn.push((entry.id, response.rect));
                if response.clicked() {
                    clicked = Some((i, response.ctx.input(|input| input.modifiers)));
                }
                if response.double_clicked() {
                    double = Some(i);
                }
                if response.secondary_clicked() {
                    menu = Some(i);
                }
                if response.drag_started_by(egui::PointerButton::Primary) {
                    dragged = Some(i);
                }
                response.context_menu(|ui| row_menu(app, ui));
            });
        });

    if let Some((i, modifiers)) = clicked {
        select(app, &rows, i, modifiers);
    }
    // Kept in place rather than rebuilt: only the control protocol reads it.
    app.row_rects.clear();
    app.row_rects.extend(drawn);
    if let Some(i) = dragged {
        app.drag_out = files_to_drag(app, &rows, i);
    }
    if let Some(i) = menu {
        // A right click on a row outside the selection acts on that row.
        if !app.selected.contains(&rows[i].id) {
            app.selected = BTreeSet::from([rows[i].id]);
        }
    }
    if let Some(i) = double {
        app.selected = BTreeSet::from([rows[i].id]);
        // A double-click on a file name opens its Properties, for any
        // download; a finished one is opened from there.
        app.queued.push(Command::Properties);
    }
}

/// What a drag starting on row `i` carries: the selection's finished files
/// when the row is in it, else that row's alone — each one still where dm
/// put it. Nothing for a download that has not finished.
fn files_to_drag(app: &DmApp, rows: &[Entry], i: usize) -> Vec<std::path::PathBuf> {
    let Some(start) = rows.get(i) else {
        return Vec::new();
    };
    let carried: Vec<&Entry> = if app.selected.contains(&start.id) {
        rows.iter()
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

/// How wide a column starts.
fn width(column: Column) -> Width {
    match column {
        Column::Name => Width::initial(220.0).at_least(120.0).clip(true),
        Column::Queue => Width::initial(24.0),
        Column::Size => Width::initial(76.0),
        Column::Status => Width::initial(130.0).clip(true),
        Column::TimeLeft => Width::initial(72.0),
        Column::Rate => Width::initial(92.0),
        Column::LastTry => Width::initial(100.0),
        Column::Description => Width::initial(160.0).clip(true),
        Column::Referrer => Width::initial(160.0).clip(true),
        Column::Folder => Width::initial(160.0).clip(true),
    }
}

/// One cell: what a column says about a download.
fn cell(ui: &mut egui::Ui, column: Column, entry: &Entry) {
    match column {
        Column::Name => {
            ui.horizontal(|ui| {
                // The file type's picture goes before each name.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
                crate::icons::file(ui.painter(), rect, entry.category);
                ui.label(name_of(entry))
                    .on_hover_text(format::shown_url(&entry.url));
            });
        }
        Column::Queue => {
            if !entry.queue.is_empty() {
                ui.label(t("Queue"))
                    .on_hover_text(crate::i18n::name(&entry.queue));
            }
        }
        Column::Size => {
            ui.label(size_of(entry));
        }
        Column::Status => status_cell(ui, entry),
        Column::TimeLeft => {
            if entry.state == State::Running {
                ui.label(format::clock(entry.progress.eta).replace(" left", ""));
            }
        }
        Column::Rate => {
            if entry.state == State::Running && entry.progress.speed > 1.0 {
                ui.label(format::rate(entry.progress.speed));
            }
        }
        Column::LastTry => {
            ui.label(format::when(entry.finished_at));
        }
        Column::Description => {
            ui.label(&entry.description);
        }
        Column::Referrer => {
            if !entry.referrer.is_empty() {
                ui.label(format::shown_url(&entry.referrer))
                    .on_hover_text(&entry.referrer);
            }
        }
        Column::Folder => {
            let folder = crate::app::folder_of(entry);
            if !folder.is_empty() {
                ui.label(&folder).on_hover_text(&folder);
            }
        }
    }
}

fn header_only(app: &mut DmApp, ui: &mut egui::Ui) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        for column in app.column_order.clone() {
            if !app.hidden_columns.contains(&column) {
                heading(app, ui, column);
            }
        }
    });
}

/// A heading that sorts by its column, saying which way.
fn heading(app: &mut DmApp, ui: &mut egui::Ui, column: Column) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let marker = match app.sort {
        Some((c, true)) if c == column => " ↑",
        Some((c, false)) if c == column => " ↓",
        _ => "",
    };
    // Grid headers in the modern look: semibold, in the secondary text colour.
    let text = RichText::new(format!("{}{marker}", column.title()));
    let text = if t.modern {
        text.family(crate::theme::bold(ui.ctx())).color(t.text2)
    } else {
        text.strong()
    };
    let response = ui.add(egui::Button::new(text).frame(false));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, column.title()));
    if response.clicked() {
        app.sort_by(column);
    }
    response.context_menu(|ui| app.columns_menu(ui));
}

fn select(app: &mut DmApp, rows: &[Entry], i: usize, modifiers: egui::Modifiers) {
    let ids: Vec<Id> = rows.iter().map(|r| r.id).collect();
    (app.selected, app.anchor) = selection(&ids, &app.selected, app.anchor, i, modifiers);
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

/// The row menu.
fn row_menu(app: &mut DmApp, ui: &mut egui::Ui) {
    for command in [
        Command::OpenFile,
        Command::OpenWith,
        Command::OpenFolder,
        Command::MoveRename,
        Command::Redownload,
    ] {
        menu_item(app, ui, command, command.title());
    }
    ui.separator();
    menu_item(app, ui, Command::DownloadNow, Command::DownloadNow.title());
    menu_item(app, ui, Command::StopDownload, t("Stop download"));
    menu_item(app, ui, Command::UpdateLink, t("Update download link"));
    menu_item(app, ui, Command::Remove, t("Remove"));
    ui.separator();
    menu_item(app, ui, Command::MoveToQueue, t("Move to queue"));
    menu_item(
        app,
        ui,
        Command::RemoveFromQueue,
        Command::RemoveFromQueue.title(),
    );
    ui.separator();
    menu_item(app, ui, Command::Properties, t("Properties"));
}

fn menu_item(app: &mut DmApp, ui: &mut egui::Ui, command: Command, label: &str) {
    if ui
        .add_enabled(app.enabled(command), egui::Button::new(label))
        .clicked()
    {
        app.queued.push(command);
        ui.close();
    }
}

fn size_of(entry: &Entry) -> String {
    match entry.progress.total {
        Some(total) => format::bytes(total),
        None if entry.progress.downloaded > 0 => format::bytes(entry.progress.downloaded),
        None => String::new(),
    }
}

/// A finished row whose file is no longer where dm saved it.
pub fn gone(entry: &Entry) -> bool {
    !entry.path.is_empty() && !std::path::Path::new(&entry.path).exists()
}

fn status_cell(ui: &mut egui::Ui, entry: &Entry) {
    let text = status_of(entry);
    match &entry.state {
        // The modern look: the percentage with a thin brand bar under it, as its
        // ProgressBar draws; the classic look writes the percentage alone.
        State::Running if crate::theme::Tokens::get(ui.ctx()).modern => {
            let t = crate::theme::Tokens::get(ui.ctx());
            let fraction = entry.progress.fraction().unwrap_or(0.0) as f32;
            ui.vertical(|ui| {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(text).size(13.0));
                let width = ui.available_width().max(60.0);
                let (track, _) =
                    ui.allocate_exact_size(egui::vec2(width, 3.0), egui::Sense::hover());
                ui.painter().rect_filled(track, 1.5, t.border);
                let mut done = track;
                done.set_width(track.width() * fraction.clamp(0.0, 1.0));
                ui.painter().rect_filled(done, 1.5, t.brand);
            });
        }
        State::Running => {
            ui.label(text);
        }
        State::Failed(reason) => {
            ui.colored_label(crate::theme::Tokens::get(ui.ctx()).error, t("Error"))
                .on_hover_text(reason);
        }
        // Kept as history, but the file was moved or deleted since: said,
        // rather than offered to Open as if it were there.
        State::Completed if gone(entry) => {
            ui.label(egui::RichText::new(t("File missing")).weak())
                .on_hover_text(format!("{} is no longer there", entry.path));
        }
        _ => {
            ui.label(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_click_extends_from_the_row_last_clicked() {
        let rows: Vec<Id> = (1..=10).collect();
        let shift = egui::Modifiers::SHIFT;
        let command = egui::Modifiers::COMMAND;
        let none = egui::Modifiers::NONE;
        // Row 1 ticked, then row 5 added with the command key: row 5 is
        // where a shift-click now extends from.
        let (picked, anchor) = selection(&rows, &BTreeSet::new(), None, 0, none);
        let (picked, anchor) = selection(&rows, &picked, anchor, 4, command);
        let (picked, anchor) = selection(&rows, &picked, anchor, 7, shift);
        assert_eq!(picked, (5..=8).collect::<BTreeSet<Id>>());
        // And again from the same place, the other way.
        let (picked, _) = selection(&rows, &picked, anchor, 2, shift);
        assert_eq!(picked, (3..=5).collect::<BTreeSet<Id>>());
    }
}
