//! The download list: one row a download, in columns, sorted by clicking a heading, selected by clicking a row (Ctrl or Cmd to
//! add, Shift for a run), a menu on the right button, and a double-click to
//! open a row's progress, or its properties once finished.

use crate::i18n::t;
use std::collections::BTreeSet;

use dm_core::{Entry, Id, State};
use egui::RichText;
use egui_extras::{Column as Width, TableBuilder};

use crate::app::{status_of, Column, DmApp};
use crate::commands::Command;
use crate::format;

pub fn show(app: &mut DmApp, ui: &mut egui::Ui) {
    // A click on a row's text selects the row. Selectable text took the
    // click for itself, so clicking a file's name selected nothing.
    ui.style_mut().interaction.selectable_labels = false;
    let rows = app.visible();
    let asking = app.dialogs.asking();
    let mut categories = app.applied.categories();
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
            if app.rows.is_empty() {
                // The next step as a button, not only words.
                ui.add_space(8.0);
                if crate::dialogs::primary(ui, "Add a download").clicked() {
                    app.queued.push(Command::AddUrl);
                }
                ui.add_space(8.0);
                ui.label(RichText::new(t(crate::app::DROP_HINT)).small().weak());
            }
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
    // Scroll bars that take their own room, always drawn where needed:
    // floating over the list, the sideways one covered its last row, which
    // End and Down could not bring clear of it (gap 147).
    ui.spacing_mut().scroll = crate::theme::steady_scroll();
    let mut list_clip = None;
    /* The up-and-down bar is the list's own, drawn in a strip at the
     * visible right edge. The table's would sit at its right edge, inside
     * the sideways scroll area, so it was out of sight whenever the columns
     * ran past the window (gap 152): the table's is hidden and this one
     * follows it, or moves it when dragged or wheeled. */
    let bar_room = ui.spacing().scroll.allocated_width();
    let whole = ui.available_rect_before_wrap();
    let list_rect = egui::Rect::from_min_max(
        whole.min,
        egui::pos2((whole.max.x - bar_room).max(whole.min.x), whole.max.y),
    );
    let moved = app.list_bar_moved.take();
    // From the left at launch: an offset egui kept from the last session
    // opened the list with Name scrolled out of view.
    let first = !std::mem::replace(&mut app.list_drawn, true);
    let mut body_out: Option<(f32, f32, egui::Rect)> = None;
    let mut sideways = egui::ScrollArea::horizontal()
        .id_salt("list-sideways")
        .auto_shrink([false, false]);
    if first {
        sideways = sideways.horizontal_scroll_offset(0.0);
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(list_rect), |ui| {
        sideways.show(ui, |ui| {
            let body_room =
                (list_rect.height() - 22.0 - bar_room - 2.0 * ui.spacing().item_spacing.y)
                    .max(row_height);
            let mut table = TableBuilder::new(ui)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                // One memory of the widths, whatever the window's width or
                // whether a row has a queue: one per case lost a width a
                // person dragged as soon as either changed (round 16).
                .id_salt("list")
                // Clear of the sideways bar along the list's foot: the last
                // row seen, its bar and the focus ring lay under it (UI
                // round 6).
                .max_scroll_height(body_room)
                .striped(striped)
                .resizable(true)
                // Dragged as well as clicked: a finished file is dragged out of the
                // list into a folder, a mail or a chat.
                .sense(egui::Sense::click_and_drag())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
            for (n, column) in columns.iter().enumerate() {
                // The last takes what is left, but never less than its own
                // width: Folder was left 80 px and read "/home/p/d…" in a
                // maximized window (round 13); the list scrolls sideways.
                table = table.column(if n + 1 == columns.len() {
                    Width::remainder()
                        .at_least(initial(*column, queue_empty))
                        .clip(true)
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
            if let Some(y) = moved {
                table = table.vertical_scroll_offset(y);
            }
            let out = table
                .header(22.0, |mut header| {
                    for column in &columns {
                        header.col(|ui| heading(app, ui, *column));
                    }
                })
                .body(|mut body| {
                    // What of the list can be seen: the focus ring is kept
                    // inside it, and Page Up and Down move by it (gaps 147, 148).
                    let seen = body.ui_mut().clip_rect();
                    list_clip = Some(seen);
                    body.rows(row_height, rows.len(), |mut row| {
                        let i = row.index();
                        let entry = &rows[i];
                        let selected = app.selected.contains(&entry.id);
                        row.set_selected(selected);
                        // Worked out for the rows drawn only, not the whole
                        // list every time a command asks what it acts on.
                        let planned = columns
                            .contains(&Column::Folder)
                            .then(|| app.planned_folder(entry, &mut categories))
                            .flatten();
                        let name = app.name_it_will_get(entry, &mut categories);
                        for column in &columns {
                            row.col(|ui| {
                                cell(
                                    ui,
                                    *column,
                                    entry,
                                    held.as_deref(),
                                    selected,
                                    planned.as_deref(),
                                    &name,
                                )
                            });
                        }
                        let response = row.response();
                        drawn.push((entry.id, response.rect));
                        // While a question waits, the list behind it takes
                        // no clicks: a row selected or opened under the
                        // Remove question was not the one it removed
                        // (round 15).
                        if asking {
                            return;
                        }
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
                        // Shift+F10 or the Menu key: the same menu for the
                        // row with the keyboard's place, under the row (a
                        // right-click's menu opens at the pointer, which a
                        // key does not have).
                        let by_key = app.cursor == Some(entry.id)
                            && std::mem::take(&mut app.row_menu_by_key);
                        egui::Popup::menu(&response)
                            .id(response.id.with("key-menu"))
                            // Under the part of the row that can be seen:
                            // under the whole row, which runs past the
                            // window, it opened over the places (UI round 5).
                            .anchor(egui::PopupAnchor::ParentRect(response.rect.intersect(seen)))
                            .open_memory(by_key.then_some(egui::SetOpenCommand::Bool(true)))
                            .show(|ui| row_menu(app, ui));
                    });
                });
            body_out = Some((
                out.state.clamped_offset().y,
                out.content_size.y,
                out.inner_rect,
            ));
        });
    });
    app.list_bar = None;
    if let Some((offset, tall, seen)) = body_out {
        let strip = egui::Rect::from_min_max(
            egui::pos2(list_rect.max.x, seen.min.y),
            egui::pos2(whole.max.x, seen.max.y),
        );
        if tall > seen.height() + 0.5 {
            app.list_bar = Some(strip);
            let bar = ui.scope_builder(egui::UiBuilder::new().max_rect(strip), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("list-bar")
                    .auto_shrink([false, false])
                    .max_height(strip.height())
                    .vertical_scroll_offset(offset)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        ui.allocate_space(egui::vec2(0.0, tall));
                    })
            });
            let to = bar.inner.state.clamped_offset().y;
            if (to - offset).abs() > 0.5 {
                app.list_bar_moved = Some(to);
                ui.ctx().request_repaint();
            }
        }
    }

    if let Some((i, modifiers)) = clicked {
        select(app, &rows, i, modifiers);
    }
    app.list_clip = list_clip;
    app.list_page = list_clip
        .map(|clip| (clip.height() / row_height).floor().max(1.0) as usize)
        .unwrap_or(1)
        .saturating_sub(1)
        .max(1);
    // Kept in place rather than rebuilt: only the control protocol reads it.
    app.row_rects.clear();
    app.row_rects.extend(drawn);
    if let Some(i) = dragged {
        let files = files_to_drag(app, &rows, i);
        // A row with no file to carry is dragged to another place in the
        // queue instead, where the list is in the queue's order: sorted by
        // a column, a place in it means nothing (gap 50).
        if files.is_empty() && app.sort.is_none() {
            app.reordering = rows.get(i).map(|r| r.id);
        } else if files.is_empty() {
            // Said, rather than the drag doing nothing (UI round 6).
            app.said = crate::i18n::Said::new(
                "The list is sorted by a column: click its heading until the arrow goes, then drag to reorder.",
                &[],
            );
        }
        app.drag_out = files;
    }
    reorder(app, ui, list_clip);
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
        // What Options › General says a double-click does: Properties by
        // default, for any download; Open or its folder for a finished
        // one, Properties still for one that is not.
        let finished = rows
            .iter()
            .any(|r| r.id == id && r.state == State::Completed);
        app.queued.push(match app.applied.double_click.as_str() {
            "open" if finished => Command::OpenFile,
            "folder" if finished => Command::OpenFolder,
            _ => Command::Properties,
        });
    }
}

/// What a drag starting on row `i` carries: the selection's finished files
/// when the row is in it, else that row's alone — each one still where dm
/// put it. Nothing for a download that has not finished.
/// A row dragged up or down the list: a line where it will go while the
/// button is held, and moved there in the queue's order when let go.
fn reorder(app: &mut DmApp, ui: &egui::Ui, clip: Option<egui::Rect>) {
    let Some(moving) = app.reordering else {
        return;
    };
    let (at, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
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
            // Thick enough to be seen as the place it goes (UI round 6).
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
        // Room for "Main queue": the width it starts with is the width it
        // keeps when rows get a queue, and 60 read "Main q…" (UI round 6).
        Column::Queue if queue_empty => 96.0,
        Column::Queue => 140.0,
        Column::Size => 76.0,
        // "Waiting for Start queue" whole, with its shape (UI round 6).
        Column::Status => 196.0,
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
        Column::Status => Width::initial(initial(Column::Status, queue_empty)).clip(true),
        Column::TimeLeft => Width::initial(88.0),
        Column::Rate => Width::initial(92.0),
        Column::LastTry => Width::initial(100.0),
        Column::Description => Width::initial(160.0).clip(true),
        Column::Referrer => Width::initial(160.0).clip(true),
        Column::Folder => Width::initial(160.0).clip(true),
    }
}

/// One cell: what a column says about a download.
fn cell(
    ui: &mut egui::Ui,
    column: Column,
    entry: &Entry,
    held: Option<&str>,
    selected: bool,
    planned: Option<&str>,
    name: &str,
) {
    match column {
        Column::Name => {
            ui.horizontal(|ui| {
                // The file type's picture goes before each name.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
                crate::icons::file(ui.painter(), rect, crate::app::kind_of(entry));
                // Drawn in reading order where it is Arabic or Hebrew.
                let drawn = ui.label(&*crate::bidi::visual(name));
                crate::bidi::said_as(&drawn, name);
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
        // Numbers against the column's right edge, so their digits line up
        // down the list (a design review: they were ragged on the left).
        Column::Size => {
            right(ui, |ui| ui.label(digits(ui, size_of(entry))));
        }
        Column::Status => status_cell(ui, entry, held, selected),
        Column::TimeLeft => {
            if entry.state == State::Running {
                right(ui, |ui| {
                    let left = entry.progress.eta.map(format::span).unwrap_or_default();
                    ui.label(digits(ui, left))
                });
            }
        }
        Column::Rate => {
            if entry.state == State::Running && entry.progress.speed > 1.0 {
                right(ui, |ui| {
                    ui.label(digits(ui, format::rate(entry.progress.speed)))
                });
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
            let mut folder = crate::app::folder_of(entry);
            if folder.is_empty() {
                folder = planned.unwrap_or_default().to_string();
            }
            if !folder.is_empty() {
                // A cell cut short shows its whole text on hover by itself;
                // a tooltip of our own drew the path twice (round 13).
                let drawn = ui.label(&*crate::bidi::visual(&folder));
                crate::bidi::said_as(&drawn, &folder);
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
                // Each heading in its column's own width, as over a list
                // with rows: laid end to end, a number heading's right
                // alignment took all the room left and the rest vanished
                // (UI round 2).
                let tall = ui.spacing().interact_size.y;
                for column in app.column_order.clone() {
                    if !app.hidden_columns.contains(&column) {
                        let wide = initial(column, true);
                        ui.allocate_ui_with_layout(
                            egui::vec2(wide, tall),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(wide);
                                heading(app, ui, column);
                            },
                        );
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
    // Over the right edge where the numbers under it are (UI round 1).
    let numbers = matches!(column, Column::Size | Column::TimeLeft | Column::Rate);
    let response = if numbers {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Clear of the next column's heading, which it ran into (UI
            // round 5); the numbers under it keep the same gap.
            ui.add_space(NUMBER_GAP);
            ui.add(egui::Button::new(text).frame(false))
        })
        .inner
    } else {
        ui.add(egui::Button::new(text).frame(false))
    };
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

/// Lays `add` out against the cell's right edge.
/// Text with digits of one width, so numbers line up down a column and
/// do not jitter as they change (gap 48).
pub fn digits(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(crate::theme::numbers(ui.ctx()))
}

/// The room kept right of a number and its heading, before the next column.
const NUMBER_GAP: f32 = 6.0;

fn right(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> egui::Response) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_space(NUMBER_GAP);
        add(ui);
    });
}

/// The shape before a row's status, so its state is told by a shape as
/// well as by words and colour: a design review found the words alone.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Glyph {
    Running,
    Paused,
    Waiting,
    Done,
    Failed,
    Cancelled,
}

fn glyph_of(entry: &Entry) -> Glyph {
    match &entry.state {
        State::Running => Glyph::Running,
        State::Failed(_) => Glyph::Failed,
        State::Completed => Glyph::Done,
        State::Cancelled => Glyph::Cancelled,
        State::Queued => Glyph::Waiting,
        // Waiting for the schedule, or added for later and waiting for its
        // queue: a clock, not the pause of one a person stopped (round 16).
        State::Paused if entry.paused_by_schedule || crate::app::waits_for_its_queue(entry) => {
            Glyph::Waiting
        }
        _ => Glyph::Paused,
    }
}

/// Draws `glyph` in a 12-point square, in one colour.
fn draw_glyph(ui: &mut egui::Ui, glyph: Glyph, colour: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let line = egui::Stroke::new(1.5, colour);
    match glyph {
        Glyph::Running => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::vec2(-3.0, -4.5),
                    c + egui::vec2(4.5, 0.0),
                    c + egui::vec2(-3.0, 4.5),
                ],
                egui::Color32::TRANSPARENT,
                line,
            ));
        }
        Glyph::Paused => {
            for dx in [-2.0, 2.0] {
                p.line_segment([c + egui::vec2(dx, -4.0), c + egui::vec2(dx, 4.0)], line);
            }
        }
        Glyph::Waiting => {
            p.circle_stroke(c, 5.0, line);
            p.line_segment([c, c + egui::vec2(0.0, -3.0)], line);
            p.line_segment([c, c + egui::vec2(2.5, 0.0)], line);
        }
        Glyph::Done => {
            p.line_segment([c + egui::vec2(-4.5, 0.0), c + egui::vec2(-1.5, 3.5)], line);
            p.line_segment([c + egui::vec2(-1.5, 3.5), c + egui::vec2(4.5, -3.5)], line);
        }
        Glyph::Failed => {
            p.circle_stroke(c, 5.0, line);
            p.line_segment([c + egui::vec2(-2.0, -2.0), c + egui::vec2(2.0, 2.0)], line);
            p.line_segment([c + egui::vec2(2.0, -2.0), c + egui::vec2(-2.0, 2.0)], line);
        }
        // A circle struck through: stopped for good, not paused (it drew
        // the pause shape, UI round 3).
        Glyph::Cancelled => {
            p.circle_stroke(c, 5.0, line);
            p.line_segment([c + egui::vec2(-3.5, 3.5), c + egui::vec2(3.5, -3.5)], line);
        }
    }
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
    let glyph = glyph_of(entry);
    let glyph_colour = match glyph {
        Glyph::Running => tokens.progress(),
        Glyph::Done => tokens.success,
        Glyph::Failed => tokens.error,
        Glyph::Paused | Glyph::Waiting | Glyph::Cancelled => tokens.weak,
    };
    // The bar under the words in the modern look: while running, and kept
    // where it stopped for a paused or failed row (it vanished), dimmed or
    // in the error colour. Not for a row that has fetched nothing.
    let fraction = entry.progress.fraction().unwrap_or(0.0) as f32;
    let bar = tokens.modern
        && match &entry.state {
            State::Running => true,
            State::Paused | State::Failed(_) => fraction > 0.0,
            _ => false,
        };
    let bar_colour = match glyph {
        Glyph::Failed => tokens.error,
        Glyph::Paused | Glyph::Waiting | Glyph::Cancelled => tokens.weak,
        _ => tokens.progress(),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        draw_glyph(ui, glyph, on_row(glyph_colour));
        // Words alone sit on the row's line, as the name and size do; only
        // with a bar under them do they stack (they sat 5 px high).
        let layout = if bar {
            egui::Layout::top_down(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        };
        ui.with_layout(layout, |ui| {
            if bar {
                ui.add_space(4.0);
            }
            match &entry.state {
                State::Failed(_) => {
                    // The whole reason on hover, with where to sign in when
                    // the server wants it: in plain words, and below them
                    // what the engine said.
                    let hover = match crate::app::failure_as_said(entry) {
                        Some(raw) => format!("{text}\n\n{raw}"),
                        None => text,
                    };
                    ui.colored_label(on_row(tokens.error), t("Error"))
                        .on_hover_text(hover);
                }
                State::Paused if entry.paused_by_schedule => {
                    let label = ui.label(text);
                    if let Some(why) = held {
                        label.on_hover_text(why);
                    }
                }
                // Kept as history, but the file was moved or deleted since:
                // said, rather than offered to Open as if it were there.
                State::Completed if gone(entry) => {
                    let words = egui::RichText::new(t("File missing"));
                    let words = if selected { words } else { words.weak() };
                    ui.label(words)
                        .on_hover_text(format!("{} is no longer there", entry.path));
                }
                // Cut with "…" where the column is narrow, the whole on hover:
                // clipped, "Waiting for Sta" ended mid-word (UI round 1).
                _ => {
                    ui.add(egui::Label::new(digits(ui, text)).truncate());
                }
            }
            if bar {
                let width = ui.available_width().max(60.0);
                let (track, response) =
                    ui.allocate_exact_size(egui::vec2(width, 3.0), egui::Sense::hover());
                // A bar a screen reader reads too, with how far it has come
                // (it was paint alone, gap 48); the row around it names the
                // file.
                response.widget_info(|| {
                    let mut info = egui::WidgetInfo::labeled(
                        egui::accesskit::Role::ProgressIndicator,
                        true,
                        t("Progress"),
                    );
                    info.value = Some((f64::from(fraction.clamp(0.0, 1.0)) * 100.0).floor());
                    info
                });
                ui.painter().rect_filled(track, 1.5, tokens.border);
                let mut done = track;
                done.set_width(track.width() * fraction.clamp(0.0, 1.0));
                ui.painter().rect_filled(done, 1.5, on_row(bar_colour));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gap 125: in a narrow window the name gives way so Status fits.
    #[test]
    fn the_name_gives_way_first_in_a_narrow_window() {
        let columns = Column::ALL.to_vec();
        assert_eq!(name_width(&columns, 1200.0, false), 220.0);
        let narrow = name_width(&columns, 600.0, false);
        assert!((120.0..220.0).contains(&narrow), "{narrow}");
        // What it gave is what Queue, Size and Status need beside it.
        let beside: f32 = [Column::Queue, Column::Size, Column::Status]
            .iter()
            .map(|c| initial(*c, false) + 8.0)
            .sum();
        assert!(narrow + beside <= 600.0, "{narrow} + {beside}");
        assert_eq!(name_width(&columns, 200.0, false), 120.0, "never under 120");
        // Gap 140: an empty Queue column gives its room to the name.
        let room = 600.0;
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
