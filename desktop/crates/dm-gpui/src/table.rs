// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The download table, as a macOS list view draws one: a header of column
//! headings (a click sorts by that column, a second the other way), and
//! the rows of the place chosen, only those on screen laid out. The columns
//! (their order, which are hidden) and every cell's words are the egui
//! window's (`dm_desktop_base::columns` and `rows`); numbers stand against
//! their column's right edge so their digits line up down the list. A
//! heading dragged onto another takes its place, as a macOS table's do.

use dm_core::{Entry, State};
use dm_desktop_base::columns::Column;
use dm_desktop_base::format;
use dm_desktop_base::i18n::t;
use gpui::{
    div, prelude::*, px, relative, uniform_list, AnyElement, Context, Role, SharedString, Window,
};

use crate::list::{kind_picture, Downloads, ROW_HEIGHT};
use crate::screen::Screen;
use crate::theme::Theme;

/// The header's height, in points.
pub const HEADER_HEIGHT: f32 = 24.0;

/// A column's width in points; the file name takes what the others leave.
fn width(column: Column) -> Option<f32> {
    match column {
        Column::Name => None,
        Column::Queue => Some(22.0),
        Column::Size => Some(72.0),
        Column::Status => Some(150.0),
        Column::TimeLeft => Some(68.0),
        Column::Rate => Some(84.0),
        Column::LastTry => Some(118.0),
        Column::Description | Column::Referrer | Column::Folder => Some(140.0),
    }
}

/// Whether a column's figures stand against its right edge.
fn numeric(column: Column) -> bool {
    matches!(column, Column::Size | Column::TimeLeft | Column::Rate)
}

/// The columns shown, in the person's order.
pub fn columns(view: &Downloads) -> Vec<Column> {
    let settings = view.backend.settings();
    Column::ordered(&settings.column_order)
        .into_iter()
        .filter(|c| !settings.columns_hidden.iter().any(|h| h == c.key()))
        .collect()
}

/// The file name's column never narrower than this: the other columns
/// scroll sideways first, as a macOS list does.
const NAME_AT_LEAST: f32 = 180.0;

/// A cell of `column` sized as its column is: one line, cut short with an
/// ellipsis where its words are longer.
fn cell(column: Column) -> gpui::Div {
    let cell = div()
        .h_full()
        .flex()
        .items_center()
        .px_2()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis();
    let cell = match width(column) {
        Some(w) => cell.w(px(w)).flex_none(),
        None => cell.flex_1().min_w(px(NAME_AT_LEAST)),
    };
    if numeric(column) {
        cell.justify_end()
    } else {
        cell
    }
}

/// What a cell says, where it says words.
fn words(column: Column, row: &Entry) -> String {
    match column {
        Column::Name | Column::Queue => String::new(),
        Column::Size => dm_desktop_base::rows::size_of(row),
        Column::Status => dm_desktop_base::rows::status_of(row),
        Column::TimeLeft if row.state == State::Running => match row.progress.eta {
            Some(eta) => format::span(eta),
            // A file of known size with no rate to go on yet says so.
            None if row.progress.total.is_some() => t("Calculating…").to_string(),
            None => String::new(),
        },
        Column::Rate if row.state == State::Running && row.progress.speed > 1.0 => {
            format::rate(row.progress.speed)
        }
        Column::LastTry => format::when(row.finished_at),
        Column::Description => row.description.clone(),
        Column::Referrer if !row.referrer.is_empty() => format::shown_url(&row.referrer),
        Column::Folder => dm_desktop_base::rows::folder_of(row),
        _ => String::new(),
    }
}

/// The header.
pub fn header(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let sort = view.sort;
    div()
        .id("header")
        .role(Role::Row)
        .h(px(HEADER_HEIGHT))
        .flex_none()
        .flex()
        .px_1()
        .border_b_1()
        .border_color(theme.separator)
        .text_size(px(crate::native::SMALL_TEXT))
        .text_color(theme.muted)
        .children(columns(view).into_iter().map(|column| {
            let heading = if column == Column::Queue {
                String::new()
            } else {
                column.title().to_string()
            };
            let arrow = match sort {
                Some((c, true)) if c == column => " ▲",
                Some((c, false)) if c == column => " ▼",
                _ => "",
            };
            // The file name stays first: every other heading moves.
            let movable = column != Column::Name;
            cell(column)
                .id(column.key())
                .role(Role::ColumnHeader)
                .aria_label(column.title())
                .hover(|s| s.bg(theme.stripe))
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.sort_by(column);
                    // View › Sort by's tick follows.
                    view.refresh_menus(cx);
                    cx.notify();
                }))
                .when(movable, |heading| {
                    heading
                        .on_drag(DraggedColumn { column, theme }, |dragged, _, _, cx| {
                            cx.new(|_| dragged.clone())
                        })
                        .drag_over::<DraggedColumn>(move |style, _, _, _| style.bg(theme.stripe))
                        .on_drop(cx.listener(move |view, dragged: &DraggedColumn, _, cx| {
                            view.move_column(dragged.column, column, cx)
                        }))
                })
                .child(screen.label(format!("{heading}{arrow}")))
        }))
}

/// `order` with `moved` in `onto`'s place: after it when it came from its
/// left, before it from its right. `None` where either is not there.
fn moved_onto(mut order: Vec<Column>, moved: Column, onto: Column) -> Option<Vec<Column>> {
    let from = order.iter().position(|c| *c == moved)?;
    order.remove(from);
    let to = order.iter().position(|c| *c == onto)?;
    order.insert(if from > to { to } else { to + 1 }, moved);
    Some(order)
}

/// A heading being dragged to a new place: what is drawn under the pointer
/// meanwhile.
#[derive(Clone)]
pub struct DraggedColumn {
    column: Column,
    theme: Theme,
}

impl Render for DraggedColumn {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py(px(2.0))
            .rounded(px(4.0))
            .border_1()
            .border_color(self.theme.separator)
            .bg(self.theme.window)
            .text_size(px(crate::native::SMALL_TEXT))
            .text_color(self.theme.text)
            .child(self.column.title())
    }
}

impl Downloads {
    /// A heading dropped on another's: the column takes that one's place
    /// (after it when dropped further right, before it further left), kept
    /// in the settings as the order of every column, hidden ones too.
    pub(crate) fn move_column(&mut self, moved: Column, onto: Column, cx: &mut Context<Self>) {
        if moved == onto || moved == Column::Name || onto == Column::Name {
            return;
        }
        let mut wanted = self.backend.settings();
        let Some(order) = moved_onto(Column::ordered(&wanted.column_order), moved, onto) else {
            return;
        };
        wanted.column_order = order.iter().map(|c| c.key().to_string()).collect();
        if let Err(e) = self.apply_settings(wanted, cx) {
            self.say(e);
        }
        cx.notify();
    }
}

/// One row's cells.
fn row_cells(
    row: &Entry,
    names: &std::collections::HashMap<dm_core::Id, String>,
    columns: &[Column],
    kind: Option<dm_core::Category>,
    theme: Theme,
    screen: &Screen,
) -> Vec<AnyElement> {
    columns
        .iter()
        .map(|&column| match column {
            Column::Name => {
                let (picture, colour) = kind_picture(kind);
                let name: gpui::SharedString = match names.get(&row.id) {
                    Some(name) => name.clone().into(),
                    None => crate::list::line(row).0,
                };
                cell(column)
                    .gap_2()
                    .child(
                        gpui::svg()
                            .path(picture)
                            .size(px(16.0))
                            .flex_none()
                            .text_color(colour),
                    )
                    .child(screen.label(name))
                    .into_any_element()
            }
            // Reference A's Q: a mark for a download in a queue.
            Column::Queue => cell(column)
                .justify_center()
                .when(!row.queue.is_empty(), |c| {
                    c.child(div().size(px(8.0)).rounded_full().bg(theme.selection))
                })
                .into_any_element(),
            Column::Status => {
                let said = words(column, row);
                let fraction = (row.state == State::Running)
                    .then(|| row.progress.fraction())
                    .flatten();
                cell(column)
                    .flex_col()
                    .items_start()
                    .justify_center()
                    .child(screen.label(said))
                    .when_some(fraction, |c, f| {
                        c.child(
                            div()
                                .mt(px(2.0))
                                .w_full()
                                .h(px(3.0))
                                .rounded_full()
                                .bg(theme.separator)
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .bg(theme.selection)
                                        .w(relative(f.clamp(0.0, 1.0) as f32)),
                                ),
                        )
                    })
                    .into_any_element()
            }
            _ => {
                let said = words(column, row);
                let numeric = numeric(column);
                cell(column)
                    .when(!numeric, |c| c.text_color(theme.muted))
                    .child(screen.label(SharedString::from(said)))
                    .into_any_element()
            }
        })
        .collect()
}

/// The rows of the place chosen.
pub fn rows(view: &Downloads, theme: Theme, cx: &mut Context<Downloads>) -> impl IntoElement {
    let screen = view.screen.clone();
    let count = view.shown.len();
    uniform_list(
        "rows",
        count,
        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
            let categories = this.backend.settings().categories();
            let columns = columns(this);
            range
                .filter_map(|i| {
                    this.shown
                        .get(i)
                        .and_then(|&r| this.rows.get(r))
                        .map(|row| (i, row))
                })
                .map(|(i, row)| {
                    let kind = this.places.placed(row, &categories).0;
                    let chosen = this.selected.contains(&row.id);
                    // A chosen row in the selection colour, its words on it,
                    // as a macOS list draws one.
                    let background = theme.selection;
                    let theme = if chosen { theme.chosen() } else { theme };
                    div()
                        .id(i)
                        .role(Role::Row)
                        .aria_selected(chosen)
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .px_1()
                        .flex()
                        .when(chosen, |r| r.bg(background).text_color(theme.text))
                        .when(!chosen && i % 2 == 1, |r| r.bg(theme.stripe))
                        .on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |view, event: &gpui::MouseDownEvent, _, cx| {
                                view.open_menu(i, event.position);
                                cx.notify();
                            }),
                        )
                        .on_click(cx.listener(move |view, event: &gpui::ClickEvent, _, cx| {
                            // A Control-click is a right-click on macOS.
                            if event.modifiers().control && cfg!(target_os = "macos") {
                                if let Some(at) = event.mouse_position() {
                                    view.open_menu(i, at);
                                    cx.notify();
                                    return;
                                }
                            }
                            let held = event.modifiers();
                            view.click_row(i, held.platform || held.control, held.shift);
                            if event.click_count() == 2 {
                                view.double_click(i, cx);
                            }
                            cx.notify();
                        }))
                        .children(row_cells(
                            row,
                            &this.shown_names,
                            &columns,
                            kind,
                            theme,
                            &screen,
                        ))
                })
                .collect()
        }),
    )
    .track_scroll(&view.scroll)
    .flex_1()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_dropped_on_another_takes_its_place() {
        use Column::{Name, Queue, Size, Status};
        let order = vec![Name, Queue, Size, Status];
        // From the left: after the one dropped on.
        assert_eq!(
            moved_onto(order.clone(), Queue, Status),
            Some(vec![Name, Size, Status, Queue])
        );
        // From the right: before it.
        assert_eq!(
            moved_onto(order.clone(), Status, Queue),
            Some(vec![Name, Status, Queue, Size])
        );
        assert_eq!(moved_onto(order, Column::Rate, Queue), None);
    }
}
