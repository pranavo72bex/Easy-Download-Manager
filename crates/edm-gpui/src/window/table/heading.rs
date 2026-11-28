// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The table's header: its headings, told to the screen reader, sorted, dragged and resized.

use super::*;

/* ------------------------------------------- as a screen reader is told */

/// The list as a screen reader is told it (gaps row U23): a table named Downloads, of the
/// header and the rows of the place chosen, in the columns shown. It said a flat list.
pub fn told_table(table: Stateful<Div>, view: &Downloads) -> Stateful<Div> {
    as_table(table, view.list.shown.len(), columns(view).len())
}

/// `table` told as one of `rows` rows under its header's, in `columns` columns.
pub(super) fn as_table(table: Stateful<Div>, rows: usize, columns: usize) -> Stateful<Div> {
    table
        .role(Role::Table)
        .aria_label(t("Downloads"))
        .aria_row_count(rows + 1)
        .aria_column_count(columns)
}

/// A column's heading as a screen reader is told it: its column's name, its place, and which
/// way the list is sorted by it, where it is (`up` for the smallest first).
pub(super) fn told_heading(
    heading: Stateful<Div>,
    column: Column,
    place: usize,
    sorted: Option<bool>,
) -> Stateful<Div> {
    let heading = heading
        .role(Role::ColumnHeader)
        .aria_label(column.title())
        .aria_column_index(place);
    match sorted {
        Some(up) => heading.a11y_synthetic_children(move |built| {
            built.parent_node().set_sort_direction(if up {
                gpui::accesskit::SortDirection::Ascending
            } else {
                gpui::accesskit::SortDirection::Descending
            });
        }),
        None => heading,
    }
}

/// A row's cell as a screen reader is told it: its words, in its column's place.
pub(super) fn told_cell(
    cell: Stateful<Div>,
    place: usize,
    said: impl Into<SharedString>,
) -> Stateful<Div> {
    cell.role(Role::Cell)
        .aria_column_index(place)
        .aria_label(said)
}

/// `cell`, told to a screen reader as one where `told` (one is reading the window), under
/// `id`; else as drawn alone, with no node of its own to keep (gaps row 180's cost).
pub(super) fn maybe_told(
    cell: Div,
    told: bool,
    id: impl Into<gpui::ElementId>,
    place: usize,
    said: impl Into<SharedString>,
) -> AnyElement {
    if told {
        told_cell(cell.id(id), place, said).into_any_element()
    } else {
        cell.into_any_element()
    }
}

/// The header.
pub fn header(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let sort = view.list.sort;
    let widths = view.widths();
    div()
        .id("header")
        .role(Role::Row)
        // The table's first row (gaps row U23).
        .aria_row_index(0)
        .h(px(HEADER_HEIGHT))
        .flex_none()
        .flex()
        // The headings from the right where the window is mirrored (gaps row U35).
        .map(crate::window::mirror::row)
        .px_1()
        .border_b_1()
        .border_color(theme.separator)
        .text_size(px(crate::widgets::native::SMALL_TEXT))
        .text_color(theme.muted)
        .children(
            columns(view)
                .into_iter()
                .enumerate()
                .map(|(place, column)| {
                    // Every heading in words, the queue's too (gaps row E7).
                    let heading = column.title().to_string();
                    let arrow = match sort {
                        Some((c, true)) if c == column => " ▲",
                        Some((c, false)) if c == column => " ▼",
                        _ => "",
                    };
                    // The file name stays first: every other heading moves.
                    let movable = column != Column::Name;
                    let sorted = sort.filter(|(c, _)| *c == column).map(|(_, up)| up);
                    told_heading(
                        cell(column, &widths).id(column.key()),
                        column,
                        place,
                        sorted,
                    )
                    .hover(|s| s.bg(theme.stripe))
                    .active(move |s| s.bg(theme.pressed_soft()))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        // Kept in the settings; View › Sort by's tick follows.
                        view.sort_by(column, cx);
                        view.refresh_menus(cx);
                        cx.notify();
                    }))
                    .when(movable, |heading| {
                        heading
                            .on_drag(DraggedColumn { column, theme }, |dragged, _, _, cx| {
                                cx.new(|_| dragged.clone())
                            })
                            .drag_over::<DraggedColumn>(move |style, _, _, _| {
                                style.bg(theme.stripe)
                            })
                            .on_drop(cx.listener(move |view, dragged: &DraggedColumn, _, cx| {
                                view.move_column(dragged.column, column, cx)
                            }))
                    })
                    .child(super::fitted(screen.label(format!("{heading}{arrow}"))))
                    // Its words whole on hover where a column narrowed by hand cuts them.
                    .when_some(
                        super::width_of(column, &widths).filter(|&wide| {
                            crate::widgets::native::small_width(&format!("{heading}{arrow}"))
                                > wide - 16.0
                        }),
                        |cell, _| {
                            let heading = SharedString::from(heading.clone());
                            cell.tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(heading.clone())
                                    .build(window, cx)
                            })
                        },
                    )
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
                                    // From its right edge where the window is mirrored,
                                    // its edge being its left one (gaps row U35).
                                    let wide = if crate::window::mirror::rtl() {
                                        f32::from(event.bounds.right() - event.event.position.x)
                                    } else {
                                        f32::from(event.event.position.x - event.bounds.left())
                                    };
                                    view.resize_column(
                                        column,
                                        wide / crate::window::zoom::factor(),
                                        cx,
                                    );
                                },
                            ))
                            .child(
                                div()
                                    .id(gpui::ElementId::Name(
                                        format!("edge-{}", column.key()).into(),
                                    ))
                                    .absolute()
                                    .top_0()
                                    .map(|edge| {
                                        if crate::window::mirror::rtl() {
                                            edge.left_0()
                                        } else {
                                            edge.right_0()
                                        }
                                    })
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
                }),
        )
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
            .py(px(space::XXS))
            .rounded(px(space::CORNER_S))
            .border_1()
            .border_color(self.theme.separator)
            .bg(self.theme.window)
            .text_size(px(crate::widgets::native::SMALL_TEXT))
            .text_color(self.theme.text)
            .child(self.column.title())
    }
}
