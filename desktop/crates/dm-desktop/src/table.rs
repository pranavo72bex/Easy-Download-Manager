//! The download list: one row a download, in columns, sorted by clicking a heading, selected by clicking a row (Ctrl or Cmd to
//! add, Shift for a run), a menu on the right button, and a double-click to
//! open a row's progress, or its properties once finished.

use crate::i18n::{t, tf};
use std::collections::BTreeSet;

mod hover;

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
    let rows = app.visible_shared();
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
                } else if !app.filter.trim().is_empty() {
                    t("No download here matches the filter.")
                } else {
                    // Done and In progress are places, not categories.
                    t("Nothing here yet.")
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
                ui.label(RichText::new(t(crate::app::drop_hint())).small().weak());
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
        // Queue is empty for a download with no queue of its own, which is
        // most; drawn then, it only pushed the sixth column half out of the
        // window, its heading cut to a letter beside the sideways bar.
        .filter(|c| *c != Column::Queue || app.rows.iter().any(|r| !r.queue.is_empty()))
        .collect();
    // Plain rows; the modern look's are taller.
    let striped = false;
    // ListViewItemMinHeight, compact.
    let row_height = if crate::theme::Tokens::get(ui.ctx()).modern {
        32.0
    } else {
        22.0
    };
    // In a narrow window the file name gives up width first, down to
    // 120 points, so Status is still in view beside it: the name took its
    // 220 and pushed Status off the edge.
    // A selected row in a fill that stands out from the list, its words in
    // the colour that reads on it (it was 235 on 255).
    // In the modern look, a list view's: the subtle fill, for a hovered row
    // as for a selected one, which also carries the accent indicator; and
    // no column lines down the rows, only between the headings (a column's
    // edge still shows when the pointer is on it).
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.visuals_mut().selection.bg_fill = t.row_selected;
    ui.visuals_mut().selection.stroke.color = t.row_selected_text;
    if t.modern {
        ui.visuals_mut().widgets.hovered.bg_fill = t.hover;
        ui.visuals_mut().widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    }
    let paint_on = (ui.ctx().clone(), ui.layer_id(), ui.clip_rect());
    // A Queue column with nothing in it gives its room to the name: at
    // 720 px it stood wider than the name, empty.
    let name_width = name_width(&columns, ui.available_width());
    // Wider than the window, the columns scroll sideways rather than
    // running off its edge: at the default size the Note, Referrer and
    // Folder columns were past it with no way to reach them.
    // Scroll bars that take their own room, always drawn where needed:
    // floating over the list, the sideways one covered its last row, which
    // End and Down could not bring clear of it.
    ui.spacing_mut().scroll = crate::theme::steady_scroll();
    // Rows that meet, each its 32 points, as a list view's: the window's 8
    // between them made each row and its fill 40 tall.
    if t.modern {
        ui.spacing_mut().item_spacing.y = 0.0;
    }
    let mut list_clip = None;
    /* The up-and-down bar is the list's own, drawn in a strip at the
     * visible right edge. The table's would sit at its right edge, inside
     * the sideways scroll area, so it was out of sight whenever the columns
     * ran past the window: the table's is hidden and this one
     * follows it, or moves it when dragged or wheeled. */
    // The sideways bar's whole thickness: a floating one is drawn over the
    // content, and the room it allocates is less than it covers.
    let scroll = &ui.spacing().scroll;
    let bar_room = scroll
        .allocated_width()
        .max(scroll.bar_width + scroll.bar_inner_margin + scroll.bar_outer_margin);
    // Only what can be seen: with the Place drop-down or the clipboard's
    // bar above the list, the room left ran past the card's visible edge,
    // and at large text the last rows could never be scrolled to.
    let whole = ui.available_rect_before_wrap().intersect(ui.clip_rect());
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
            // The header's height as drawn last frame, not a guess: at 300%
            // it took more than the 22 points allowed, the body was sized a
            // row too tall and the last row stayed under the sideways bar.
            let head_id = egui::Id::unique("dm-list-head");
            let head = ui
                .data(|d| d.get_temp::<f32>(head_id))
                .unwrap_or(22.0 + ui.spacing().item_spacing.y);
            // And what the body ran past the visible list by, measured last
            // frame, until the list's height changes: with the clipboard's
            // bar above it at 300% the last row was still cut.
            let over_id = egui::Id::unique("dm-list-over");
            let over = ui
                .data(|d| d.get_temp::<(f32, f32)>(over_id))
                .filter(|(height, _)| (height - list_rect.height()).abs() < 0.5)
                .map_or(0.0, |(_, over)| over);
            let body_room =
                (list_rect.height() - head - bar_room - ui.spacing().item_spacing.y - over)
                    .max(f32::min(row_height, list_rect.height()));
            // The widths follow the window until the person drags one: then
            // theirs are kept. Kept from the first draw whatever happened to
            // the window after, a list opened narrow and widened left the
            // name 120 points beside a band of nothing, and one shrunk cut
            // Status to "Co…".
            let state_id = ui.scope_id().with(egui::IdSalt::new("list"));
            // Where a column's resize handle is: half the spacing past its
            // cell, and how near it a press grabs it.
            let grab = ui.style().interaction.resize_grab_radius_side;
            let half_gap = ui.spacing().item_spacing.x / 2.0;
            let dragged_id = egui::Id::unique("dm-columns-dragged");
            let width_id = egui::Id::unique("dm-list-width");
            // A person's widths are for the text size they were dragged at:
            // kept at another, Status and all after it were out of view at
            // 140%, so the list fits itself again.
            let zoom_id = egui::Id::unique("dm-columns-zoom");
            let zoom = ui.ctx().zoom_factor();
            let zoom_before = ui.data_mut(|d| d.get_persisted::<f32>(zoom_id));
            if zoom_before.is_some_and(|z| (z - zoom).abs() > 0.01) {
                ui.data_mut(|d| d.insert_persisted(dragged_id, false));
            }
            ui.data_mut(|d| d.insert_persisted(zoom_id, zoom));
            let dragged_before = ui
                .data_mut(|d| d.get_persisted::<bool>(dragged_id))
                .unwrap_or(false);
            let width_before = ui.data(|d| d.get_temp::<f32>(width_id));
            ui.data_mut(|d| d.insert_temp(width_id, list_rect.width()));
            let rearranged = std::mem::take(&mut app.columns_changed);
            if rearranged {
                ui.data_mut(|d| d.insert_persisted(dragged_id, false));
            }
            let refit = rearranged
                || (!dragged_before
                    && width_before.is_none_or(|w| (w - list_rect.width()).abs() > 0.5));
            let mut table = TableBuilder::new(ui)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                // One memory of the widths, whatever the window's width or
                // whether a row has a queue: one per case lost a width a
                // person dragged as soon as either changed.
                .id_salt("list")
                // Clear of the sideways bar along the list's foot: the last
                // row seen, its bar and the focus ring lay under it.
                .max_scroll_height(body_room)
                // Never taller than the room, either: the table's own least
                // height is 200 points, and in less (large text, the Place
                // drop-down above) its last rows ran under the status bar
                // and could never be scrolled to.
                .min_scrolled_height(body_room.min(200.0))
                .striped(striped)
                .resizable(true)
                // Dragged as well as clicked: a finished file is dragged out of the
                // list into a folder, a mail or a chat.
                .sense(egui::Sense::click_and_drag())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
            for (n, column) in columns.iter().enumerate() {
                // The last takes what is left, but never less than its own
                // width: Folder was left 80 px and read "/home/p/d…" in a
                // maximized window; the list scrolls sideways.
                table = table.column(if n + 1 == columns.len() {
                    Width::remainder().at_least(initial(*column)).clip(true)
                } else if *column == Column::Name {
                    Width::initial(name_width).at_least(120.0).clip(true)
                } else {
                    width(*column)
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
            if refit {
                table.reset();
            }
            let mut edges: Vec<f32> = Vec::new();
            let out = table
                .header(if t.modern { 32.0 } else { 22.0 }, |mut header| {
                    for (n, column) in columns.iter().enumerate() {
                        let mut cell = egui::Rect::NOTHING;
                        let (rect, _) = header.col(|ui| {
                            cell = ui.max_rect();
                            // Room for the selection's indicator at the
                            // row's start, kept by the headings too.
                            if n == 0 && t.modern {
                                ui.add_space(LEAD);
                            }
                            heading(app, ui, *column)
                        });
                        edges.push(rect.right() + half_gap);
                        // A line between the headings, not down the rows.
                        if t.modern && n + 1 < columns.len() && cell.is_positive() {
                            let (ctx, layer, clip) = &paint_on;
                            ctx.layer_painter(*layer).with_clip_rect(*clip).vline(
                                cell.right() + half_gap,
                                cell.y_range().shrink(8.0),
                                egui::Stroke::new(1.0, t.border),
                            );
                        }
                    }
                })
                .body(|mut body| {
                    // What of the list can be seen: the focus ring is kept
                    // inside it, and Page Up and Down move by it.
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
                        for (n, column) in columns.iter().enumerate() {
                            row.col(|ui| {
                                if n == 0 && t.modern {
                                    ui.add_space(LEAD);
                                }
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
                        // The selection's indicator: the accent pill at the
                        // row's start, where the row can be seen.
                        if selected && t.modern {
                            let left = response.rect.left().max(seen.left());
                            let pill = egui::Rect::from_center_size(
                                egui::pos2(left + 2.5, response.rect.center().y),
                                egui::vec2(3.0, 16.0),
                            );
                            response
                                .ctx
                                .layer_painter(response.layer_id)
                                .with_clip_rect(seen)
                                .rect_filled(pill, 1.5, t.brand);
                        }
                        // While a question waits, the list behind it takes
                        // no clicks: a row selected or opened under the
                        // Remove question was not the one it removed.
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
                        // Not where a column's edge is: a quick press and
                        // drag there to resize took the row along.
                        let on_edge = response
                            .ctx
                            .input(|i| i.pointer.press_origin())
                            .is_some_and(|at| edges.iter().any(|x| (at.x - x).abs() <= grab));
                        if response.drag_started_by(egui::PointerButton::Primary) && !on_edge {
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
                            // window, it opened over the places.
                            .anchor(egui::PopupAnchor::ParentRect(response.rect.intersect(seen)))
                            // Below the row, else above it: where neither
                            // fitted, egui put it at the row's left, over
                            // the places (a short window, or the browser bar
                            // above the list).
                            .align(egui::RectAlign::BOTTOM_START)
                            .align_alternatives(&[egui::RectAlign::TOP_START])
                            .open_memory(by_key.then_some(egui::SetOpenCommand::Bool(true)))
                            .show(|ui| row_menu(app, ui));
                    });
                });
            // Each column's edge named for a screen reader, which found nine
            // handles with no name.
            for (i, column) in columns.iter().enumerate() {
                let said = crate::i18n::tf("Width of the {} column", &[&column.title()]);
                ui.ctx()
                    .accesskit_node_builder(state_id.with("resize_column").with(i), |node| {
                        node.set_label(said);
                    });
            }
            // A column's edge dragged: the person's widths from now on.
            let dragged_now = (0..columns.len()).any(|i| {
                ui.ctx()
                    .read_response(state_id.with("resize_column").with(i))
                    .is_some_and(|r| r.dragged())
            });
            if dragged_now && !dragged_before {
                ui.data_mut(|d| d.insert_persisted(dragged_id, true));
            }
            ui.data_mut(|d| {
                d.insert_temp(
                    egui::Id::unique("dm-list-head"),
                    (out.inner_rect.min.y - list_rect.min.y).max(0.0),
                )
            });
            // How far the body ran past what can be seen above the bar.
            let seen_bottom = list_rect.max.y.min(ui.clip_rect().max.y) - bar_room;
            let past = (out.inner_rect.max.y - seen_bottom).max(0.0);
            ui.data_mut(|d| {
                let id = egui::Id::unique("dm-list-over");
                let before = d
                    .get_temp::<(f32, f32)>(id)
                    .filter(|(height, _)| (height - list_rect.height()).abs() < 0.5)
                    .map_or(0.0, |(_, over)| over);
                d.insert_temp(id, (list_rect.height(), before + past));
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
                    // As tall as the strip and no taller: a scroll area is
                    // never under 64 points, and in a strip of one row (300%
                    // text with the clipboard's bar above) it could not reach
                    // as far as the list, clamped the list's offset to its
                    // own and pulled the list back a little each frame, so
                    // the last row was never there.
                    .min_scrolled_height(strip.height())
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
        // a column, a place in it means nothing.
        if files.is_empty() && app.sort.is_none() {
            app.reordering = rows.get(i).map(|r| r.id);
        } else if files.is_empty() {
            // Said, rather than the drag doing nothing.
            app.said = crate::i18n::Said::new(
                "The list is sorted by a column: click its heading until the arrow goes, then drag to reorder.",
                &[],
            );
        }
        app.drag_out = files;
    }
    reorder(app, ui, list_clip);
    hover::actions(app, ui, &rows, list_clip);
    if let Some(id) = menu.and_then(|i| rows.get(i)).map(|r| r.id) {
        // A right click on a row outside the selection acts on that row.
        if !app.selected.contains(&id) {
            app.selected = BTreeSet::from([id]);
        }
        // The keys go on from the row clicked, as after any click: the
        // ring stayed on the row the arrows had last reached.
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

mod cells;
mod columns;
mod menus;
pub(crate) use menus::delete_after_times;
mod selection;
mod status;

use cells::*;
pub use columns::*;
use menus::*;
pub use selection::*;
pub use status::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// In a narrow window the name gives way so Status fits.
    #[test]
    fn the_name_gives_way_first_in_a_narrow_window() {
        let columns = Column::ALL.to_vec();
        assert_eq!(name_width(&columns, 1200.0), 220.0);
        let narrow = name_width(&columns, 600.0);
        assert!((120.0..220.0).contains(&narrow), "{narrow}");
        // The window's first size: Time left and Speed whole beside the
        // name (Speed was off to the right).
        let first = name_width(&columns, 730.0);
        let through_speed: f32 = [Column::Size, Column::Status, Column::TimeLeft, Column::Rate]
            .iter()
            .map(|c| initial(*c) + 8.0)
            .sum();
        assert!(first >= 120.0 && first + through_speed <= 730.0, "{first}");
        // What it gave is what Size and Status need beside it.
        let beside: f32 = [Column::Size, Column::Status]
            .iter()
            .map(|c| initial(*c) + 8.0)
            .sum();
        assert!(narrow + beside <= 600.0, "{narrow} + {beside}");
        assert_eq!(name_width(&columns, 200.0), 120.0, "never under 120");
    }

    /// The arrow keys, Home and End move the selection, and Shift
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
