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
    // Why a held row waits, said on hovering it.
    let held = app.held_because();
    if rows.is_empty() {
        // The headings first, where the list's are, and what to do under
        // them: said above them, it read as a title over an empty table.
        header_only(app, ui);
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
    // In a narrow window the file name gives up width first, down to
    // 120 points, so Status is still in view beside it: the name took its
    // 220 and pushed Status off the edge.
    // A selected row in a fill that stands out from the list, its words in
    // the colour that reads on it (gap 132: it was 235 on 255).
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.visuals_mut().selection.bg_fill = t.row_selected;
    ui.visuals_mut().selection.stroke.color = t.row_selected_text;
    // A Queue column with nothing in it gives its room to the name: at
    // 720 px it stood wider than the name, empty (gap 140).
    let queue_empty = rows.iter().all(|r| r.queue.is_empty());
    let name_width = name_width(&columns, ui.available_width(), queue_empty);
    // Wider than the window, the columns scroll sideways rather than
    // running off its edge: at the default size the Note, Referrer and
    // Folder columns were past it with no way to reach them.
    egui::ScrollArea::horizontal()
        .id_salt("list-sideways")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut table = TableBuilder::new(ui)
                // Kept apart by how much room the name was given, so a
                // narrower window starts from widths that fit it.
                .id_salt(("list", (name_width / 25.0) as i32, queue_empty))
                .striped(striped)
                .resizable(true)
                // Dragged as well as clicked: a finished file is dragged out of the
                // list into a folder, a mail or a chat.
                .sense(egui::Sense::click_and_drag())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
            for (n, column) in columns.iter().enumerate() {
                table = table.column(if n + 1 == columns.len() {
                    Width::remainder().at_least(80.0).clip(true)
                } else if *column == Column::Name {
                    Width::initial(name_width).at_least(120.0).clip(true)
                } else {
                    width(*column, queue_empty)
                });
            }
            // The row the keys moved to, brought into view.
            if let Some(i) = app
                .scroll_to
                .take()
                .and_then(|id| rows.iter().position(|r| r.id == id))
            {
                table = table.scroll_to_row(i, None);
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
                        let selected = app.selected.contains(&entry.id);
                        row.set_selected(selected);
                        for column in &columns {
                            row.col(|ui| cell(ui, *column, entry, held.as_deref(), selected));
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
    if let Some(id) = menu.and_then(|i| rows.get(i)).map(|r| r.id) {
        // A right click on a row outside the selection acts on that row.
        if !app.selected.contains(&id) {
            app.selected = BTreeSet::from([id]);
        }
        // The keys go on from the row clicked, as after any click: the
        // ring stayed on the row the arrows had last reached (gap 134).
        app.cursor = Some(id);
        app.anchor = Some(id);
    }
    if let Some(id) = double.and_then(|i| rows.get(i)).map(|r| r.id) {
        app.selected = BTreeSet::from([id]);
        app.cursor = Some(id);
        app.anchor = Some(id);
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

/// How wide the file name starts in `room`: its 220 points where the
/// columns up to Status fit beside it, else what is left after them, but
/// never under 120.
pub fn name_width(columns: &[Column], room: f32, queue_empty: bool) -> f32 {
    let through_status = columns
        .iter()
        .position(|c| *c == Column::Status)
        .unwrap_or(columns.len().saturating_sub(1));
    let others: f32 = columns
        .iter()
        .take(through_status + 1)
        .filter(|c| **c != Column::Name)
        .map(|c| initial(*c, queue_empty) + 8.0)
        .sum();
    (room - others - 24.0).clamp(120.0, 220.0)
}

/// Where each column but the name starts, in points.
fn initial(column: Column, queue_empty: bool) -> f32 {
    match column {
        Column::Name => 220.0,
        Column::Queue if queue_empty => 60.0,
        Column::Queue => 110.0,
        Column::Size => 76.0,
        Column::Status => 130.0,
        Column::TimeLeft => 88.0,
        Column::Rate => 92.0,
        Column::LastTry => 100.0,
        Column::Description | Column::Referrer | Column::Folder => 160.0,
    }
}

/// How wide a column starts.
fn width(column: Column, queue_empty: bool) -> Width {
    match column {
        Column::Name => Width::initial(220.0).at_least(120.0).clip(true),
        Column::Queue => Width::initial(initial(Column::Queue, queue_empty)).clip(true),
        Column::Size => Width::initial(76.0),
        Column::Status => Width::initial(130.0).clip(true),
        Column::TimeLeft => Width::initial(88.0),
        Column::Rate => Width::initial(92.0),
        Column::LastTry => Width::initial(100.0),
        Column::Description => Width::initial(160.0).clip(true),
        Column::Referrer => Width::initial(160.0).clip(true),
        Column::Folder => Width::initial(160.0).clip(true),
    }
}

/// One cell: what a column says about a download.
fn cell(ui: &mut egui::Ui, column: Column, entry: &Entry, held: Option<&str>, selected: bool) {
    match column {
        Column::Name => {
            ui.horizontal(|ui| {
                // The file type's picture goes before each name.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
                crate::icons::file(ui.painter(), rect, entry.category);
                // Drawn in reading order where it is Arabic or Hebrew.
                let name = name_of(entry);
                let drawn = ui.label(&*crate::bidi::visual(&name));
                crate::bidi::said_as(&drawn, &name);
                drawn.on_hover_text(format::shown_url(&entry.url));
            });
        }
        // The queue's name, dm's own in the language in force.
        Column::Queue => {
            if !entry.queue.is_empty() {
                let name = crate::i18n::name(&entry.queue);
                ui.label(&name).on_hover_text(name);
            }
        }
        Column::Size => {
            ui.label(size_of(entry));
        }
        Column::Status => status_cell(ui, entry, held, selected),
        Column::TimeLeft => {
            if entry.state == State::Running {
                ui.label(entry.progress.eta.map(format::span).unwrap_or_default());
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
                let drawn = ui.label(&*crate::bidi::visual(&folder));
                crate::bidi::said_as(&drawn, &folder);
                drawn.on_hover_text(&folder);
            }
        }
    }
}

fn header_only(app: &mut DmApp, ui: &mut egui::Ui) {
    ui.add_space(10.0);
    egui::ScrollArea::horizontal()
        .id_salt("headings-sideways")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for column in app.column_order.clone() {
                    if !app.hidden_columns.contains(&column) {
                        heading(app, ui, column);
                    }
                }
            });
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
    app.cursor = ids.get(i).copied();
}

/// A move of the keyboard's place in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
    First,
    Last,
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
        (Step::Up, None) => last,
        (Step::Down, None) => 0,
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
    menu_item(
        app,
        ui,
        Command::DownloadNow,
        app.title_of(Command::DownloadNow),
    );
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
    shown_size(entry).map(format::bytes).unwrap_or_default()
}

/// The size the Size column shows: the file's, or what has arrived of one
/// whose length the server did not say. The column sorts by it.
pub fn shown_size(entry: &Entry) -> Option<u64> {
    entry
        .progress
        .total
        .or(Some(entry.progress.downloaded).filter(|n| *n > 0))
}

/// A finished row whose file is no longer where dm saved it.
pub fn gone(entry: &Entry) -> bool {
    !entry.path.is_empty() && !std::path::Path::new(&entry.path).exists()
}

fn status_cell(ui: &mut egui::Ui, entry: &Entry, held: Option<&str>, selected: bool) {
    let text = status_of(entry);
    let tokens = crate::theme::Tokens::get(ui.ctx());
    // On a selected row every word is in the selection's own colour: red,
    // the brand's blue or a faded grey would not read on its fill.
    let on_row = |colour: egui::Color32| {
        if selected {
            tokens.row_selected_text
        } else {
            colour
        }
    };
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
                ui.painter().rect_filled(done, 1.5, on_row(t.brand));
            });
        }
        State::Running => {
            ui.label(text);
        }
        State::Failed(_) => {
            // The whole reason on hover, with where to sign in when the
            // server wants it.
            ui.colored_label(on_row(tokens.error), t("Error"))
                .on_hover_text(text);
        }
        State::Paused if entry.paused_by_schedule => {
            let label = ui.label(text);
            if let Some(why) = held {
                label.on_hover_text(why);
            }
        }
        // Kept as history, but the file was moved or deleted since: said,
        // rather than offered to Open as if it were there.
        State::Completed if gone(entry) => {
            let words = egui::RichText::new(t("File missing"));
            let words = if selected { words } else { words.weak() };
            ui.label(words)
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

    /// Gap 125: in a narrow window the name gives way so Status fits.
    #[test]
    fn the_name_gives_way_first_in_a_narrow_window() {
        let columns = Column::ALL.to_vec();
        assert_eq!(name_width(&columns, 1200.0, false), 220.0);
        let narrow = name_width(&columns, 480.0, false);
        assert!((120.0..220.0).contains(&narrow), "{narrow}");
        // What it gave is what Queue, Size and Status need beside it.
        assert!(
            narrow + 110.0 + 76.0 + 130.0 + 3.0 * 8.0 <= 480.0,
            "{narrow}"
        );
        assert_eq!(name_width(&columns, 200.0, false), 120.0, "never under 120");
        // Gap 140: an empty Queue column gives its room to the name.
        let room = 720.0 - 240.0;
        assert!(name_width(&columns, room, true) > name_width(&columns, room, false));
        assert!(name_width(&columns, room, true) >= initial(Column::Queue, true));
    }

    /// Gap 117: the arrow keys, Home and End move the selection, and Shift
    /// extends it from where it started.
    #[test]
    fn the_arrow_keys_move_the_selection_and_shift_extends_it() {
        let rows: Vec<Id> = (1..=5).collect();
        let none = BTreeSet::new();
        let (picked, anchor, cursor) = stepped(&rows, &none, None, None, Step::Down, false);
        assert_eq!((picked.clone(), cursor), (BTreeSet::from([1]), Some(1)));
        let (picked, anchor, cursor) = stepped(&rows, &picked, anchor, cursor, Step::Down, false);
        assert_eq!(picked, BTreeSet::from([2]));
        let (picked, anchor, cursor) = stepped(&rows, &picked, anchor, cursor, Step::Down, true);
        let (picked, anchor, cursor) = stepped(&rows, &picked, anchor, cursor, Step::Down, true);
        assert_eq!(picked, BTreeSet::from([2, 3, 4]));
        let (picked, anchor, cursor) = stepped(&rows, &picked, anchor, cursor, Step::Up, true);
        assert_eq!(picked, BTreeSet::from([2, 3]));
        let (picked, anchor, cursor) = stepped(&rows, &picked, anchor, cursor, Step::Last, false);
        assert_eq!((picked.clone(), cursor), (BTreeSet::from([5]), Some(5)));
        let (picked, _, _) = stepped(&rows, &picked, anchor, cursor, Step::Down, false);
        assert_eq!(picked, BTreeSet::from([5]), "the bottom stays the bottom");
        let (picked, _, cursor) = stepped(&rows, &picked, anchor, cursor, Step::First, false);
        assert_eq!((picked, cursor), (BTreeSet::from([1]), Some(1)));
        let (picked, _, _) = stepped(&rows, &none, None, None, Step::Up, false);
        assert_eq!(
            picked,
            BTreeSet::from([5]),
            "Up with nothing picked starts at the bottom"
        );
    }

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
