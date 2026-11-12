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
//! heading dragged onto another takes its place, as a macOS table's do; a
//! finished row dragged out of the window carries its file (`dragout`).

use dm_core::{Entry, State};
use dm_desktop_base::columns::Column;
use dm_desktop_base::format;
use dm_desktop_base::i18n::t;
use gpui::{
    div, prelude::*, relative, uniform_list, AnyElement, Context, Role, SharedString, Window,
};

use crate::list::{kind_picture, Downloads, ROW_HEIGHT};
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

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
    let settings = &view.drawn;
    Column::ordered(&settings.column_order)
        .into_iter()
        .filter(|c| !settings.columns_hidden.iter().any(|h| h == c.key()))
        .collect()
}

/// About how many letters Status shows at its least width: longer words are offered whole on
/// hover.
const STATUS_FITS: usize = 22;

/// The widths a person gave columns, by key (`Settings::column_widths`).
pub(crate) type Widths = std::collections::BTreeMap<String, f32>;

/// The narrowest and widest a column is dragged to.
const NARROWEST: f32 = 24.0;
const WIDEST: f32 = 800.0;

/// A column's width: the one a person gave it, else its own; none for the name, which takes
/// what the others leave.
fn width_of(column: Column, widths: &Widths) -> Option<f32> {
    width(column).map(|own| {
        widths
            .get(column.key())
            .map_or(own, |&chosen| chosen.clamp(NARROWEST, WIDEST))
    })
}

/// The most Status grows to, as a share of its least width.
const STATUS_GROWS_TO: f32 = 2.0;

/// A file's name split where a long one is cut: what goes before its extension, and the
/// extension with its dot. The extension is kept whole, so a long name reads
/// "A very long name of a fi….mp4" rather than losing what kind of file it is (gaps row U3).
fn name_parts(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        // Not a leading dot (".bashrc"), and an extension of a few letters, not a sentence.
        Some(dot) if dot > 0 && name.len() - dot <= 8 && !name[dot..].contains(' ') => {
            name.split_at(dot)
        }
        _ => (name, ""),
    }
}

/// The file name's column never narrower than this: the other columns
/// scroll sideways first, as a macOS list does.
const NAME_AT_LEAST: f32 = 180.0;

/// How wide the shown columns are together, in points, the file name's at
/// its least: narrower than this, the list scrolls sideways.
pub fn least_width(view: &Downloads) -> f32 {
    // The row's own margins (`px_1` each side) beside its cells.
    let widths = view.widths();
    columns(view)
        .into_iter()
        .map(|column| width_of(column, &widths).unwrap_or(NAME_AT_LEAST))
        .sum::<f32>()
        + 8.0
}

/// A cell of `column` sized as its column is: one line, cut short with an
/// ellipsis where its words are longer.
fn cell(column: Column, widths: &Widths) -> gpui::Div {
    let cell = div()
        .h_full()
        .flex()
        .items_center()
        .px_2()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis();
    let chosen = widths.contains_key(column.key());
    let cell = match (column, width_of(column, widths)) {
        // Status takes a share of what a wider window gives, up to twice its width, so a
        // failure's words are read whole where there is room (gaps row U3); the name takes the
        // rest, twice as much.
        (Column::Status, Some(w)) if !chosen => cell
            .flex_grow(1.0)
            .flex_shrink_0()
            .flex_basis(px(w))
            .min_w(px(w))
            .max_w(px(w * STATUS_GROWS_TO)),
        (_, Some(w)) => cell.w(px(w)).flex_none(),
        (_, None) => cell.flex_1().flex_grow(2.0).min_w(px(NAME_AT_LEAST)),
    };
    if numeric(column) {
        cell.justify_end()
    } else {
        cell
    }
}

/// What a cell says, where it says words; when a row finished, from the
/// words the read kept (`whens`).
fn words(column: Column, row: &Entry, whens: &format::Whens) -> String {
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
        Column::LastTry => whens.of(row.finished_at),
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
    let widths = view.widths();
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
            cell(column, &widths)
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
                // Its right edge dragged makes it wider or narrower, and a double-click there
                // fits it to what it shows (gaps row U4). Not the name's, which takes the rest.
                .when(width(column).is_some(), |heading| {
                    heading
                        .relative()
                        .on_drag_move::<ResizingColumn>(cx.listener(
                            move |view, event: &gpui::DragMoveEvent<ResizingColumn>, _, cx| {
                                if event.drag(cx).column != column {
                                    return;
                                }
                                let wide = f32::from(event.event.position.x - event.bounds.left());
                                view.resize_column(column, wide / crate::zoom::factor(), cx);
                            },
                        ))
                        .child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("edge-{}", column.key()).into(),
                                ))
                                .absolute()
                                .top_0()
                                .right_0()
                                .h_full()
                                .w(px(6.0))
                                .cursor(gpui::CursorStyle::ResizeColumn)
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(
                                    move |view, event: &gpui::ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        if event.click_count() == 2 {
                                            view.fit_column(column, cx);
                                        }
                                    },
                                ))
                                .on_drag(ResizingColumn { column }, |dragged, _, _, cx| {
                                    cx.new(|_| dragged.clone())
                                }),
                        )
                })
        }))
        .on_drop(cx.listener(|view, _: &ResizingColumn, _, cx| view.finish_resize(cx)))
}

/// A heading's edge being dragged: nothing drawn under the pointer but the column itself.
#[derive(Clone)]
pub struct ResizingColumn {
    column: Column,
}

impl Render for ResizingColumn {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// `order` with `moved` in `onto`'s place: after it when it came from its
/// left, before it from its right. `None` where either is not there. For
/// the columns' headings and the toolbar's buttons alike.
pub(crate) fn moved_onto<T: PartialEq>(mut order: Vec<T>, moved: T, onto: T) -> Option<Vec<T>> {
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
    /// The columns' widths as drawn: the ones kept, and the one being dragged.
    pub(crate) fn widths(&self) -> Widths {
        let mut widths = self.drawn.column_widths.clone();
        if let Some((column, wide)) = self.resizing {
            widths.insert(column.key().to_string(), wide);
        }
        widths
    }

    /// A heading's edge dragged to `wide` points (at 100%): drawn at once, kept a few times a
    /// second meanwhile and at its end, so a drag let go anywhere is kept.
    pub(crate) fn resize_column(&mut self, column: Column, wide: f32, cx: &mut Context<Self>) {
        let wide = wide.clamp(NARROWEST, WIDEST).round();
        self.resizing = Some((column, wide));
        let due = self
            .resize_kept
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(200));
        if due {
            self.resize_kept = Some(std::time::Instant::now());
            self.keep_width(column, wide);
        }
        cx.notify();
    }

    /// The drag let go over the header: its last width kept.
    pub(crate) fn finish_resize(&mut self, cx: &mut Context<Self>) {
        if let Some((column, wide)) = self.resizing {
            self.keep_width(column, wide);
            self.read();
        }
        self.resizing = None;
        self.resize_kept = None;
        cx.notify();
    }

    /// A double-click on a heading's edge: the column as wide as the longest thing it shows
    /// among the rows listed (and its heading), within what a column may be.
    pub(crate) fn fit_column(&mut self, column: Column, cx: &mut Context<Self>) {
        let longest = self
            .shown
            .iter()
            .filter_map(|&r| self.rows.get(r))
            .map(|row| words(column, row, &self.whens).chars().count())
            .chain([column.title().chars().count() + 2])
            .max()
            .unwrap_or(0);
        // About the width of a letter of the list's text, and the cell's margins.
        let wide = longest as f32 * crate::native::TEXT * 0.58 + 18.0;
        self.resizing = None;
        self.keep_width(column, wide.clamp(NARROWEST, WIDEST).round());
        self.read();
        cx.notify();
    }

    fn keep_width(&self, column: Column, wide: f32) {
        self.backend.keep_look(|settings| {
            settings
                .column_widths
                .insert(column.key().to_string(), wide);
        });
    }

    /// A heading dropped on another's: the column takes that one's place
    /// (after it when dropped further right, before it further left), kept
    /// in the settings as the order of every column, hidden ones too.
    pub(crate) fn move_column(&mut self, moved: Column, onto: Column, cx: &mut Context<Self>) {
        if moved == onto || moved == Column::Name || onto == Column::Name {
            return;
        }
        let now = self.backend.settings();
        let Some(order) = moved_onto(Column::ordered(&now.column_order), moved, onto) else {
            return;
        };
        // The window's look alone: the file is written off its thread.
        self.backend.keep_look(|settings| {
            settings.column_order = order.iter().map(|c| c.key().to_string()).collect();
        });
        self.read();
        cx.notify();
    }
}

/// The file's name as its row shows it.
fn shown_name(
    row: &Entry,
    names: &std::collections::HashMap<dm_core::Id, String>,
) -> gpui::SharedString {
    match names.get(&row.id) {
        Some(name) => name.clone().into(),
        None => crate::list::line(row).0,
    }
}

/// A row as a screen reader says it: its file's name, then its status.
fn spoken(
    row: &Entry,
    names: &std::collections::HashMap<dm_core::Id, String>,
) -> gpui::SharedString {
    format!(
        "{}, {}",
        shown_name(row, names),
        dm_desktop_base::rows::status_of(row)
    )
    .into()
}

/// One row's cells.
fn row_cells(
    row: &Entry,
    view: &Downloads,
    columns: &[Column],
    kind: Option<dm_core::Category>,
    theme: Theme,
    screen: &Screen,
    widths: &Widths,
) -> Vec<AnyElement> {
    let (names, whens) = (&view.shown_names, &view.whens);
    columns
        .iter()
        .map(|&column| match column {
            Column::Name => {
                let (picture, colour) = kind_picture(kind);
                let name = shown_name(row, names);
                cell(column, widths)
                    .gap_2()
                    .child(
                        gpui::svg()
                            .path(picture)
                            .size(px(16.0))
                            .flex_none()
                            .text_color(colour),
                    )
                    .child({
                        let (stem, extension) = name_parts(&name);
                        let drawn = div()
                            .flex()
                            .min_w_0()
                            .overflow_hidden()
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(SharedString::from(stem.to_string())),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .child(SharedString::from(extension.to_string())),
                            );
                        // Read as the whole name, as it was one label.
                        screen.mark(name.clone(), drawn)
                    })
                    .into_any_element()
            }
            // Reference A's Q: a mark for a download in a queue.
            Column::Queue => cell(column, widths)
                .justify_center()
                .when(!row.queue.is_empty(), |c| {
                    c.child(div().size(px(8.0)).rounded_full().bg(theme.selection))
                })
                .into_any_element(),
            Column::Status => {
                let said = words(column, row, whens);
                let fraction = (row.state == State::Running)
                    .then(|| row.progress.fraction())
                    .flatten();
                // The whole words on hover, and the engine's own where the row says them in
                // plainer ones: a cut failure could not be read (gaps row U3).
                let whole = match dm_desktop_base::rows::failure_as_said(row) {
                    Some(raw) => format!("{said}\n{raw}"),
                    None => said.clone(),
                };
                cell(column, widths)
                    .id(("status", row.id))
                    .flex_col()
                    .items_start()
                    .justify_center()
                    .when(whole.chars().count() > STATUS_FITS, |c| {
                        c.tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(SharedString::from(whole.clone()))
                                .build(window, cx)
                        })
                    })
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
                let said = words(column, row, whens);
                let numeric = numeric(column);
                cell(column, widths)
                    .when(!numeric, |c| c.text_color(theme.muted))
                    .child(screen.label(SharedString::from(said)))
                    .into_any_element()
            }
        })
        .collect()
}

/// What stands under the headings where the place chosen shows no row: as
/// the egui window says it (`rows::empty_words`), and while nothing at all
/// is listed, the next step as a button and where links may be dropped.
/// Without it a new window was an empty table that said nothing.
pub fn empty(view: &Downloads, theme: Theme, cx: &mut Context<Downloads>) -> impl IntoElement {
    let screen = view.screen.clone();
    let filtered = !view.filter.read(cx).value().trim().is_empty();
    let (heading, line, next_step) =
        dm_desktop_base::rows::empty_words(!view.rows.is_empty(), filtered);
    div()
        .id("empty-list")
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .pt(px(40.0))
        .px(px(crate::native::MARGIN))
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(16.0))
                .child(screen.label(SharedString::from(heading))),
        )
        .child(
            div()
                .text_color(theme.muted)
                .child(screen.label(SharedString::from(line))),
        )
        .when(next_step, |empty| {
            empty
                .child(
                    div().pt(px(8.0)).child(
                        crate::native::button(
                            "add-a-download",
                            t("Add a download").into(),
                            true,
                            theme,
                            &screen,
                        )
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.run(dm_shell::app::commands::Command::AddUrl, cx);
                        })),
                    ),
                )
                .child(
                    div()
                        .pt(px(8.0))
                        .text_size(px(crate::native::SMALL_TEXT))
                        .text_color(theme.muted)
                        .child(screen.label(SharedString::from(t(crate::drops::DROP_HINT)))),
                )
        })
}

/// The rows of the place chosen.
pub fn rows(view: &Downloads, theme: Theme, cx: &mut Context<Downloads>) -> impl IntoElement {
    let screen = view.screen.clone();
    let count = view.shown.len();
    uniform_list(
        "rows",
        count,
        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
            // The rows are built as the list is laid out, after `render`:
            // part of the frame all the same (`costs::drawing`).
            let _drawing = dm_shell::app::costs::drawing();
            this.laid_out(range.clone(), cx);
            let categories = this.drawn.categories();
            let columns = columns(this);
            let widths = this.widths();
            let list = cx.entity().downgrade();
            // Whether a chosen row is finished, asked once, the first time a
            // chosen row that is not needs to know: it carries the others.
            let chosen_finished = std::cell::OnceCell::new();
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
                    let original = theme;
                    let theme = if chosen { theme.chosen() } else { theme };
                    let draggable = crate::dragout::may_carry(row)
                        || (chosen
                            && *chosen_finished.get_or_init(|| {
                                this.rows.iter().any(|r| {
                                    this.selected.contains(&r.id) && crate::dragout::may_carry(r)
                                })
                            }));
                    let dragged = draggable.then(|| crate::dragout::DraggedRow {
                        id: row.id,
                        list: list.clone(),
                        name: shown_name(row, &this.shown_names),
                        theme: original,
                    });
                    // Known by its download, not by where it is: the
                    // handlers below act on this row wherever a sort or a
                    // new row has put it by the time they run.
                    let id = row.id;
                    div()
                        .id(("row", id as usize))
                        .role(Role::Row)
                        // What a screen reader says of it: its file and how
                        // it stands, as the row shows them (it said nothing).
                        .aria_label(spoken(row, &this.shown_names))
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
                                view.open_menu(id, event.position);
                                cx.notify();
                            }),
                        )
                        .on_click(cx.listener(move |view, event: &gpui::ClickEvent, _, cx| {
                            // The primary button's alone: GPUI hands the
                            // right button's click here too, after its press
                            // opened the menu above, and that chose the row
                            // by itself, so Move/Rename for two chosen rows
                            // renamed one (Linux, real input, 2026-11-11).
                            if !event.standard_click() {
                                return;
                            }
                            // A Control-click is a right-click on macOS.
                            if event.modifiers().control && cfg!(target_os = "macos") {
                                if let Some(at) = event.mouse_position() {
                                    view.open_menu(id, at);
                                    cx.notify();
                                    return;
                                }
                            }
                            let held = event.modifiers();
                            view.click_row(id, held.platform || held.control, held.shift);
                            if event.click_count() == 2 {
                                view.double_click(id, cx);
                            }
                            cx.notify();
                        }))
                        // A finished row's file dragged out of the window
                        // (`dragout`); a click without moving still chooses
                        // the row, and a double-click opens it.
                        .when_some(dragged, |r, dragged| {
                            r.on_drag(dragged, |dragged, _, _, cx| dragged.picture(cx))
                                .external_drag_payload(
                                    |dragged: &crate::dragout::DraggedRow, _, cx| {
                                        dragged.payload(cx)
                                    },
                                )
                        })
                        .children(row_cells(
                            row, this, &columns, kind, theme, &screen, &widths,
                        ))
                })
                .collect()
        }),
    )
    .track_scroll(&view.scroll)
    .flex_1()
    // A sideways gesture is the table's, to move the columns (`list.rs`);
    // the rows took it as an up-and-down one.
    .map(|mut list| {
        list.style().restrict_scroll_to_axis = Some(true);
        list
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_names_extension_is_kept_apart_to_stay_whole() {
        assert_eq!(name_parts("A long film.mp4"), ("A long film", ".mp4"));
        assert_eq!(name_parts("archive.tar.gz"), ("archive.tar", ".gz"));
        assert_eq!(name_parts(".bashrc"), (".bashrc", ""));
        assert_eq!(name_parts("no extension"), ("no extension", ""));
        assert_eq!(name_parts("v1.2 of the talk"), ("v1.2 of the talk", ""));
    }

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
