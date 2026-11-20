// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The rows of the place chosen as cards, at a phone's width: each its file's kind and name,
//! its progress bar, its size (what has come of it while it is not finished), its speed, its
//! time left and its state, in the table's own words (`table::words`). A tap chooses a card, a
//! tap on the card chosen opens it as a double-click opens a row; its ⋯, a long press or a
//! right-click opens its row menu as a sheet.

use dm_core::{Entry, State};
use dm_desktop_base::columns::Column;
use dm_desktop_base::i18n::t;
use gpui::{div, prelude::*, uniform_list, Context, Role, SharedString};

use crate::widgets::{native, space};
use crate::window::list::{kind_picture, Downloads};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// A card's height, in points: a name, a bar and two lines of figures, a finger's room around
/// (well over `space::TOUCH`).
pub(crate) const CARD_HEIGHT: f32 = 84.0;

/// The cards of the place chosen, only those on screen laid out, scrolled as the table is.
pub(super) fn cards(
    view: &Downloads,
    theme: Theme,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let screen = view.screen.clone();
    let count = view.list.shown.len();
    uniform_list(
        "cards",
        count,
        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
            let _drawing = dm_shell::app::costs::drawing();
            this.laid_out(range.clone(), cx);
            let categories = this.known.drawn.categories();
            range
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
                    card(this, row, i, kind, chosen, theme, &screen, cx)
                })
                .collect()
        }),
    )
    .track_scroll(&view.list.scroll)
    .flex_1()
}

/// What a card says of its size: what has come of the whole while it is not finished, the
/// whole once it is.
fn size_words(row: &Entry) -> String {
    let size = dm_desktop_base::rows::size_of(row);
    let going = !matches!(row.state, State::Completed) && row.progress.downloaded > 0;
    if going && !size.is_empty() {
        format!(
            "{} / {size}",
            dm_desktop_base::format::bytes(row.progress.downloaded)
        )
    } else {
        size
    }
}

/// Row `row`, the `i`th shown, as a card.
#[allow(clippy::too_many_arguments)]
fn card(
    view: &Downloads,
    row: &Entry,
    i: usize,
    kind: Option<dm_core::Category>,
    chosen: bool,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> gpui::AnyElement {
    let id = row.id;
    let name = crate::window::table::shown_name(row, &view.known.shown_names);
    let status = dm_desktop_base::rows::status_of(row);
    let whens = &view.known.whens;
    let speed = crate::window::table::words(Column::Rate, row, whens);
    let left = crate::window::table::words(Column::TimeLeft, row, whens);
    let fraction = match row.state {
        State::Completed => Some(1.0),
        _ => row.progress.fraction(),
    };
    let (picture, colour) = kind_picture(kind);
    let card_theme = if chosen { theme.chosen() } else { theme };
    let small = |words: String, colour| {
        div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(native::SMALL_TEXT))
            .text_color(colour)
            .child(screen.label(SharedString::from(words)))
    };
    div()
        .id(("card", id as usize))
        .role(Role::ListItem)
        .aria_label(SharedString::from(format!("{name}, {status}")))
        .aria_selected(chosen)
        .aria_row_index(i + 1)
        .w_full()
        .relative()
        .h(px(CARD_HEIGHT))
        .px(px(space::L))
        .py(px(space::M))
        .flex()
        .flex_col()
        .justify_between()
        .border_b_1()
        .border_color(theme.separator)
        .when(chosen, |c| {
            c.bg(theme.selection).text_color(card_theme.text)
        })
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |view, _, _, _| {
                view.layout.narrow.pressed = Some((id, std::time::Instant::now()));
            }),
        )
        .on_mouse_down(
            gpui::MouseButton::Right,
            cx.listener(move |view, _, _, cx| view.open_card_menu(id, cx)),
        )
        .on_click(cx.listener(move |view, event: &gpui::ClickEvent, _, cx| {
            if !event.standard_click() {
                return;
            }
            view.card_tapped(id, event.click_count(), cx);
        }))
        // The name, with its kind's picture and the card's menu.
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(space::M))
                .child(
                    gpui::svg()
                        .path(picture)
                        .size(px(16.0))
                        .flex_none()
                        .text_color(if chosen { card_theme.text } else { colour }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(screen.label(name)),
                )
                // Room for the card's menu over its top right.
                .child(div().flex_none().w(px(space::TOUCH - space::L))),
        )
        // The card's menu: a finger's target in the card's top right corner.
        .child(
            div()
                .id(("card-menu", id as usize))
                .role(Role::Button)
                .aria_label(t("Row menu"))
                .absolute()
                .top_0()
                .right_0()
                .size(px(space::TOUCH))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.open_card_menu(id, cx);
                }))
                .child(screen.mark(t("Row menu"), div().child("⋯"))),
        )
        .child(div().w_full().child(crate::window::table::progress_track(
            fraction.unwrap_or(0.0),
            false,
            card_theme,
        )))
        // Its size and speed, then its state and time left.
        .child(
            div()
                .flex()
                .justify_between()
                .gap(px(space::M))
                .child(small(size_words(row), card_theme.muted))
                .child(small(speed, card_theme.muted)),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .gap(px(space::M))
                .child(small(status, card_theme.text))
                .child(small(left, card_theme.muted)),
        )
        .into_any_element()
}

impl Downloads {
    /// A tap on card `id`: held long, its menu; on the card already chosen alone, or twice,
    /// what a double-click on its row does (Options › General); else the card chosen alone.
    fn card_tapped(&mut self, id: dm_core::Id, clicks: usize, cx: &mut Context<Self>) {
        let held = self
            .layout
            .narrow
            .pressed
            .take()
            .filter(|(pressed, _)| *pressed == id)
            .is_some_and(|(_, at)| at.elapsed() >= super::LONG_PRESS);
        if held {
            self.open_card_menu(id, cx);
            return;
        }
        let alone = self.list.selected.len() == 1 && self.list.selected.contains(&id);
        if alone || clicks == 2 {
            self.double_click(id, cx);
        } else {
            self.click_row(id, false, false);
        }
        cx.notify();
    }
}
