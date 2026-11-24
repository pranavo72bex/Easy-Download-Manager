// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The download table, as a macOS list view draws one: a header of column
//! headings (a click sorts by that column, a second the other way), and
//! the rows of the place chosen, only those on screen laid out. The columns
//! (their order, which are hidden) and every cell's words come
//! from `dm_desktop_base::columns` and `rows`; numbers stand against
//! their column's right edge so their digits line up down the list. A
//! heading dragged onto another takes its place, as a macOS table's do; a
//! finished row dragged out of the window carries its file (`dragout`).

use dm_core::{Entry, State};
use dm_desktop_base::columns::Column;
use dm_desktop_base::format;
use dm_desktop_base::i18n::t;
#[cfg(test)]
use dm_shell::app::commands::Command;
use gpui::{
    div, prelude::*, uniform_list, AnyElement, Context, Div, Role, SharedString, Stateful, Window,
};

use crate::widgets::space;
use crate::window::list::{kind_picture, Downloads, ROW_HEIGHT};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod cells;
pub(crate) use cells::{arriving, list_drawn, set_still, state_mark};
mod heading;
mod pointer;
mod rowbuttons;
use rowbuttons::{buttons_width, row_actions, row_buttons, ROW_GROUP};

pub(crate) use cells::{finishing_bar, gone, leave, progress_track};
pub(crate) use heading::moved_onto;
#[cfg(test)]
use heading::{as_table, told_heading};
pub use heading::{header, told_table};
use heading::{maybe_told, told_cell};

/// The header's height, in points.
pub const HEADER_HEIGHT: f32 = 24.0;

/// How often a width being dragged, a column's or the side panel's, is kept in the settings
/// while the drag goes on: a few times a second, so a drag let go anywhere is kept.
pub(crate) const RESIZE_KEPT_EVERY: std::time::Duration = std::time::Duration::from_millis(200);

/// A column's width in points; the file name takes what the others leave.
fn width(column: Column) -> Option<f32> {
    match column {
        Column::Name => None,
        // The queue's name, where it was a dot (gaps row E7).
        Column::Queue => Some(72.0),
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
    let settings = &view.known.drawn;
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

/// `words` in a cell, able to be narrower than they are: cut at the cell's end with an
/// ellipsis where the column is narrower. As a flex item of their own width a right-aligned
/// column's words began outside it ("ertragungsrate" for "Übertragungsrate", gaps row E2).
pub(crate) fn fitted(words: impl IntoElement) -> gpui::Div {
    div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(words)
}

/// A cell of `column` sized as its column is: one line, cut short with an
/// ellipsis where its words are longer.
fn cell(column: Column, widths: &Widths) -> gpui::Div {
    let cell = div()
        .h_full()
        .flex()
        // Its contents from the right where the window is mirrored (gaps row U35).
        .map(crate::window::mirror::row)
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
pub(crate) fn words(column: Column, row: &Entry, whens: &format::Whens) -> String {
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

impl Downloads {
    /// The columns' widths as drawn: the ones kept, and the one being dragged.
    pub(crate) fn widths(&self) -> Widths {
        let mut widths = self.known.drawn.column_widths.clone();
        if let Some((column, wide)) = self.layout.resizing {
            widths.insert(column.key().to_string(), wide);
        }
        widths
    }

    /// A heading's edge dragged to `wide` points (at 100%): drawn at once, kept a few times a
    /// second meanwhile and at its end, so a drag let go anywhere is kept.
    pub(crate) fn resize_column(&mut self, column: Column, wide: f32, cx: &mut Context<Self>) {
        let wide = wide.clamp(NARROWEST, WIDEST).round();
        self.layout.resizing = Some((column, wide));
        let due = self
            .layout
            .resize_kept
            .is_none_or(|at| at.elapsed() >= RESIZE_KEPT_EVERY);
        if due {
            self.layout.resize_kept = Some(std::time::Instant::now());
            self.keep_width(column, wide);
        }
        cx.notify();
    }

    /// The drag let go over the header: its last width kept.
    pub(crate) fn finish_resize(&mut self, cx: &mut Context<Self>) {
        if let Some((column, wide)) = self.layout.resizing {
            self.keep_width(column, wide);
            self.read();
        }
        self.layout.resizing = None;
        self.layout.resize_kept = None;
        cx.notify();
    }

    /// A double-click on a heading's edge: the column as wide as the longest thing it shows
    /// among the rows listed (and its heading), within what a column may be.
    pub(crate) fn fit_column(&mut self, column: Column, cx: &mut Context<Self>) {
        let longest = self
            .list
            .shown
            .iter()
            .filter_map(|&r| self.rows.get(r))
            .map(|row| words(column, row, &self.known.whens).chars().count())
            .chain([column.title().chars().count() + 2])
            .max()
            .unwrap_or(0);
        // About the width of a letter of the list's text, and the cell's margins.
        let wide = longest as f32 * crate::widgets::native::TEXT * 0.58 + 18.0;
        self.layout.resizing = None;
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
pub(crate) fn shown_name(
    row: &Entry,
    names: &std::collections::HashMap<dm_core::Id, String>,
) -> gpui::SharedString {
    match names.get(&row.id) {
        Some(name) => name.clone().into(),
        None => crate::window::list::line(row).0,
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

/// The columns shown, in order, and the widths a person gave them; and
/// whether a screen reader is reading the window, so each cell is told as
/// one (`maybe_told`).
struct Layout<'a> {
    columns: &'a [Column],
    widths: &'a Widths,
    told: bool,
}

/// One row's cells, its own `buttons` at the name's right end.
fn row_cells(
    row: &Entry,
    view: &Downloads,
    layout: Layout<'_>,
    kind: Option<dm_core::Category>,
    theme: Theme,
    screen: &Screen,
    buttons: AnyElement,
) -> Vec<AnyElement> {
    let Layout {
        columns,
        widths,
        told,
    } = layout;
    let (names, whens) = (&view.known.shown_names, &view.known.whens);
    let mut buttons = Some(buttons);
    let id = row.id as usize;
    columns
        .iter()
        .enumerate()
        .map(|(place, &column)| match column {
            Column::Name => {
                let (picture, colour) = kind_picture(kind);
                // On a chosen row the picture takes the row's words' colour: its own read 1.02
                // to 1 on macOS's selection (gaps row U22).
                let colour = if theme.is_chosen() {
                    theme.text
                } else {
                    colour
                };
                let name = shown_name(row, names);
                // Room kept for the row's own buttons where they show, so the name ends
                // before them and not under them (gaps row E7).
                let kept = buttons_width(row_actions(row).len());
                let named = cell(column, widths)
                    .gap_2()
                    .relative()
                    // At the left where the window is mirrored, as the buttons are (U35).
                    .map(|c| {
                        let shown = theme.is_chosen();
                        if crate::window::mirror::rtl() {
                            c.when(shown, |c| c.pl(px(kept)))
                                .group_hover(ROW_GROUP, move |s| s.pl(px(kept)))
                        } else {
                            c.when(shown, |c| c.pr(px(kept)))
                                .group_hover(ROW_GROUP, move |s| s.pr(px(kept)))
                        }
                    })
                    .children(buttons.take())
                    .child(
                        gpui::svg()
                            .path(picture)
                            .size(px(16.0))
                            .flex_none()
                            .text_color(colour),
                    )
                    .child(cells::name_drawn(&name, screen));
                maybe_told(named, told, ("name", id), place, name)
            }
            // The queue a download waits in, by its name (gaps row E7): it was a dot under a
            // heading with no word.
            Column::Queue => {
                let queue = if row.queue.is_empty() {
                    String::new()
                } else {
                    dm_desktop_base::i18n::name(&row.queue)
                };
                let named = cell(column, widths)
                    .when(!theme.is_chosen(), |c| c.text_color(theme.muted))
                    .child(fitted(screen.label(queue.clone())));
                maybe_told(named, told, ("queue", id), place, queue)
            }
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
                    .when(told, |c| told_cell(c, place, said.clone()))
                    .flex_col()
                    .items_start()
                    // Against the right where the window is mirrored (gaps row U35).
                    .when(crate::window::mirror::rtl(), |c| c.items_end())
                    .justify_center()
                    .when(whole.chars().count() > STATUS_FITS, |c| {
                        c.tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(SharedString::from(whole.clone()))
                                .build(window, cx)
                        })
                    })
                    // Its state in its colour, as the phone's cards say it: a failure in the
                    // warning's, a finished download in the success colour, one going in the
                    // accent; on a chosen row, the choice's own text colour.
                    .when(!theme.is_chosen(), |c| match row.state {
                        State::Failed(_) => c.text_color(theme.warning),
                        State::Completed => c.text_color(theme.success),
                        State::Running => c.text_color(theme.accent),
                        _ => c,
                    })
                    // A mark told by its shape, then the words (gaps row E4): a state is
                    // never told by colour alone (`docs/ui-design.md`, rule 5).
                    .child(
                        div()
                            .flex()
                            .map(crate::window::mirror::row)
                            .items_center()
                            .gap(px(space::XS))
                            .max_w_full()
                            .children(cells::state_mark(&row.state, theme))
                            .child(fitted(screen.label(said))),
                    )
                    .when_some(fraction, |c, f| {
                        c.child(cells::progress_track(row.id, f, told, theme))
                    })
                    // A bar just finished fills and fades rather than vanishing (gaps row K7).
                    .children(cells::finishing_bar(row, theme))
                    .into_any_element()
            }
            _ => {
                let mut said = words(column, row, whens);
                // A row not saved yet shows the folder it will go to, without that guess
                // written into the row (`names::planned_folder`).
                if column == Column::Folder && said.is_empty() {
                    let settings = &view.known.drawn;
                    said = dm_shell::app::names::planned_folder(
                        row,
                        &settings.folders.download_dir,
                        &mut settings.categories(),
                    )
                    .unwrap_or_default();
                }
                let said = SharedString::from(said);
                let numeric = numeric(column);
                let worded = cell(column, widths)
                    .when(!numeric, |c| c.text_color(theme.muted))
                    .child(fitted(screen.label(said.clone())));
                maybe_told(worded, told, (column.key(), id), place, said)
            }
        })
        .collect()
}

/// What stands under the headings where the place chosen shows no row: as
/// `rows::empty_words` says it, and while nothing at all
/// is listed, the next step as a button and where links may be dropped.
/// Without it a new window was an empty table that said nothing.
pub fn empty(view: &Downloads, theme: Theme, cx: &mut Context<Downloads>) -> impl IntoElement {
    let screen = view.screen.clone();
    let filtered = !view.list.filter.read(cx).value().trim().is_empty();
    let (heading, line, next_step) =
        dm_desktop_base::rows::empty_words(!view.rows.is_empty(), filtered);
    // A phone has no browser extension and nothing is dropped on it: a link comes by Share.
    let line = if crate::platform::ON_A_PHONE && next_step {
        t(PHONE_EMPTY)
    } else {
        line
    };
    div()
        .id("empty-list")
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .pt(px(40.0))
        .px(px(crate::widgets::native::MARGIN))
        .gap(px(space::XS))
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
        .text_center()
        .when(next_step, |empty| {
            empty
                .child(
                    div().pt(px(space::M)).child(
                        crate::widgets::native::button(
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
                .when(!crate::platform::ON_A_PHONE, |empty| {
                    empty.child(
                        div()
                            .pt(px(space::M))
                            .text_size(px(crate::widgets::native::SMALL_TEXT))
                            .text_color(theme.muted)
                            .child(
                                screen.label(SharedString::from(t(
                                    crate::platform::drops::DROP_HINT,
                                ))),
                            ),
                    )
                })
        })
}

/// What an empty list says of the next step on a phone.
pub(crate) const PHONE_EMPTY: &str = "Share a link to dm from any app, or add one here.";

/// The rows of the place chosen.
pub fn rows(view: &Downloads, theme: Theme, cx: &mut Context<Downloads>) -> impl IntoElement {
    let screen = view.screen.clone();
    let count = view.list.shown.len();
    uniform_list(
        "rows",
        count,
        cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
            // The rows are built as the list is laid out, after `render`:
            // part of the frame all the same (`costs::drawing`).
            let _drawing = dm_shell::app::costs::drawing();
            // Each cell a node of its own only while a screen reader reads
            // the window: nothing else asks for them (gaps row U23).
            let told = window.is_a11y_active();
            this.laid_out(range.clone(), cx);
            let categories = this.known.drawn.categories();
            let columns = columns(this);
            let widths = this.widths();
            let list = cx.entity().downgrade();
            // Whether a chosen row is finished, asked once, the first time a
            // chosen row that is not needs to know: it carries the others.
            let chosen_finished = std::cell::OnceCell::new();
            let drawn = range
                .filter_map(|i| {
                    this.list
                        .shown
                        .get(i)
                        .and_then(|&r| this.rows.get(r))
                        .map(|row| (i, row))
                })
                .map(|(i, row)| {
                    let kind = this.list.places.placed(row, &categories).0;
                    let chosen = this.list.selected.contains(&row.id);
                    // A chosen row in the selection colour, its words on it,
                    // as a macOS list draws one.
                    let background = theme.selection;
                    let original = theme;
                    let theme = if chosen { theme.chosen() } else { theme };
                    // A finished row's file dragged out; and, in the list's own order, any
                    // row dragged onto another to stand there (gaps row U39).
                    let reorders = this.list.sort.is_none();
                    let draggable = reorders
                        || crate::platform::dragout::may_carry(row)
                        || (chosen
                            && *chosen_finished.get_or_init(|| {
                                this.rows.iter().any(|r| {
                                    this.list.selected.contains(&r.id)
                                        && crate::platform::dragout::may_carry(r)
                                })
                            }));
                    let dragged = draggable.then(|| crate::platform::dragout::DraggedRow {
                        id: row.id,
                        list: list.clone(),
                        name: shown_name(row, &this.known.shown_names),
                        theme: original,
                    });
                    // Known by its download, not by where it is: the
                    // handlers below act on this row wherever a sort or a
                    // new row has put it by the time they run.
                    let id = row.id;
                    let drawn = div()
                        .id(("row", id as usize))
                        .role(Role::Row)
                        // What a screen reader says of it: its file and how
                        // it stands, as the row shows them (it said nothing).
                        .aria_label(spoken(row, &this.known.shown_names))
                        .aria_selected(chosen)
                        // Its place in the table, under the header's row.
                        .aria_row_index(i + 1)
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .px_1()
                        .flex()
                        // The columns from the right where the window is mirrored (gaps row U35).
                        .map(crate::window::mirror::row)
                        .when(chosen, |r| r.bg(background).text_color(theme.text))
                        .when(!chosen && i % 2 == 1, |r| r.bg(theme.stripe));
                    pointer::row_handlers(drawn, id, dragged, reorders, original, cx)
                        // Its own buttons at the name's right end, where
                        // the pointer is on it or it is chosen (gaps row U13).
                        .group(ROW_GROUP)
                        .children(row_cells(
                            row,
                            this,
                            Layout {
                                columns: &columns,
                                widths: &widths,
                                told,
                            },
                            kind,
                            theme,
                            &screen,
                            row_buttons(
                                row,
                                chosen,
                                if chosen { background } else { theme.background },
                                original,
                                cx,
                            )
                            .into_any_element(),
                        ))
                        // A row just added fades in, one taken off fades out (gaps row K7).
                        .map(|row| cells::arriving(id, row))
                })
                .collect::<Vec<_>>();
            // Rows drawn from now on, but for the window's first, are ones just added.
            cells::list_drawn();
            drawn
        }),
    )
    .track_scroll(&view.list.scroll)
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
    fn the_list_is_told_as_a_table_of_headings_and_cells() {
        use crate::window::a11y::node_of;
        // The header's row and three of downloads, in four columns.
        let table = node_of(&as_table(div().id("table"), 3, 4));
        assert_eq!(table.role(), Role::Table);
        assert_eq!(table.label(), Some("Downloads"));
        assert_eq!(table.row_count(), Some(4));
        assert_eq!(table.column_count(), Some(4));

        let heading = node_of(&told_heading(div().id("size"), Column::Size, 2, Some(true)));
        assert_eq!(heading.role(), Role::ColumnHeader);
        assert_eq!(heading.label(), Some(Column::Size.title()));
        assert_eq!(heading.column_index(), Some(2));

        let cell = node_of(&told_cell(div().id(("size", 7usize)), 2, "1.5 MB"));
        assert_eq!(cell.role(), Role::Cell);
        assert_eq!(cell.label(), Some("1.5 MB"));
        assert_eq!(cell.column_index(), Some(2));
    }

    /// A row's own buttons by how it stands, on the row under the pointer:
    /// Details on one not finished, Delete on one that is not writing its file.
    #[test]
    fn a_rows_buttons_follow_its_state() {
        let of = |state: State| -> Vec<Command> {
            let mut row: Entry = serde_json::from_value(serde_json::json!({
                "id": 1,
                "url": "http://example.com/f.bin",
                "state": {"kind": "queued"},
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "f.bin",
            }))
            .unwrap();
            row.state = state;
            row_actions(&row).into_iter().map(|(c, _, _)| c).collect()
        };
        use Command::*;
        assert_eq!(of(State::Running), [StopDownload, ShowProgress]);
        assert_eq!(of(State::Queued), [StopDownload, ShowProgress]);
        assert_eq!(of(State::Paused), [DownloadNow, ShowProgress, Remove]);
        assert_eq!(
            of(State::Failed("gone".into())),
            [DownloadNow, ShowProgress, Remove]
        );
        assert_eq!(of(State::Completed), [OpenFile, OpenFolder, Remove]);
        assert_eq!(of(State::Cancelled), [Remove]);
    }

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
