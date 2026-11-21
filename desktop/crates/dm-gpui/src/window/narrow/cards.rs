// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The rows of the place chosen as cards, at a phone's width: each its file's kind and name,
//! its progress bar, its size (what has come of it while it is not finished), its speed, its
//! time left and its state, in the table's own words (`table::words`). A tap opens a card as a
//! double-click opens a row; its Stop or Resume button does that in one tap; its ⋯, a long
//! press or a right-click opens its row menu as a sheet.

use dm_core::{Entry, State};
use dm_desktop_base::columns::Column;
use dm_desktop_base::i18n::t;
use gpui::{div, prelude::*, uniform_list, Context, Role, SharedString};

use crate::widgets::space;
use crate::window::list::{kind_picture, Downloads};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// A card's height, in points: a name, a bar and two lines of figures, a finger's room around
/// (well over `space::TOUCH`); taller on a phone, whose text is larger.
pub(crate) const CARD_HEIGHT: f32 = if crate::platform::ON_A_PHONE {
    92.0
} else {
    84.0
};

/// A card's file name, as a phone's lists set an item's first line (Material's 16 sp).
const NAME_TEXT: f32 = 16.0;
/// A card's line of state and figures (Material's 13 sp supporting text).
const META_TEXT: f32 = 13.0;
/// The tile of the file's kind at a card's start.
const TILE: f32 = 44.0;
/// The round Stop or Resume button.
const QUICK: f32 = 40.0;

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
    // The last card can be scrolled clear of the floating Add URL button over the list.
    .pb(px(super::FLOATING + 2.0 * space::XL))
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
    // The card's one-tap action, as a phone's download list has it: Stop while it runs, Resume
    // while it waits, is stopped or failed; none once it is whole or cancelled.
    let quick = match row.state {
        State::Running | State::Queued => Some((
            dm_shell::app::commands::Command::StopDownload,
            "icons/stop.svg",
            t("Stop"),
        )),
        State::Paused | State::Failed(_) => Some((
            dm_shell::app::commands::Command::DownloadNow,
            "icons/resume.svg",
            t(dm_desktop_base::rows::resume_word(&[row])),
        )),
        State::Completed | State::Cancelled => None,
    };
    let card_theme = if chosen { theme.chosen() } else { theme };
    let small = |words: String, colour| {
        div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_size(px(META_TEXT))
            .text_color(colour)
            .child(screen.label(SharedString::from(words)))
    };
    // Its state in its colour, as a phone's torrent apps colour it: going in the accent, failed
    // in the warning's, finished in the success colour.
    let state_colour = if chosen {
        card_theme.text
    } else {
        match row.state {
            State::Running => theme.accent,
            State::Failed(_) => theme.warning,
            State::Completed => theme.success,
            _ => theme.muted,
        }
    };
    let left_words = {
        let size = size_words(row);
        if size.is_empty() || matches!(row.state, State::Failed(_)) {
            status.clone()
        } else if row.state == State::Running {
            // How far it is, the bar shows already: what has come of the whole is said.
            size
        } else {
            format!("{status} · {size}")
        }
    };
    // The speed under the bar; the time left beside the name, where the line under the bar has
    // no room for both with a size in gigabytes.
    let right_words = speed;
    let round = |id: (&'static str, usize), words: SharedString| {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(words)
            .flex_none()
            .size(px(space::TOUCH))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .active(move |b| b.bg(theme.control))
            // Its own tap, not the card's.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
    };
    let tint = if chosen { card_theme.text } else { colour };
    div()
        .id(("card", id as usize))
        .role(Role::ListItem)
        .aria_label(SharedString::from(format!("{name}, {status}")))
        .aria_selected(chosen)
        .aria_row_index(i + 1)
        .w_full()
        .h(px(CARD_HEIGHT))
        .pl(px(space::L))
        .pr(px(space::XS))
        .flex()
        .items_center()
        .gap(px(space::M))
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
        // Its kind's picture on a tile of its colour.
        .child(
            div()
                .flex_none()
                .size(px(TILE))
                .rounded(px(TILE / 4.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::Rgba { a: 0.16, ..tint })
                .child(gpui::svg().path(picture).size(px(24.0)).text_color(tint)),
        )
        // Its name, its bar, and a line of its state and figures.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(space::S))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::M))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(NAME_TEXT))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(screen.label(name)),
                        )
                        .when(!left.trim().is_empty(), |line| {
                            line.child(div().flex_none().child(small(left, card_theme.muted)))
                        }),
                )
                .child(div().w_full().child(crate::window::table::progress_track(
                    fraction.unwrap_or(0.0),
                    false,
                    card_theme,
                )))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap(px(space::M))
                        .child(small(left_words, state_colour))
                        .child(
                            div()
                                .flex_none()
                                .child(small(right_words, card_theme.muted)),
                        ),
                ),
        )
        // Its one-tap action: Stop while it runs or waits, Resume while stopped or failed.
        .when_some(quick, |card, (command, picture, words)| {
            card.child(
                round(("card-quick", id as usize), words.into())
                    .size(px(QUICK))
                    .border_1()
                    .border_color(if chosen {
                        card_theme.text
                    } else {
                        theme.control_border
                    })
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.choose_card(id);
                        view.run(command, cx);
                        cx.notify();
                    }))
                    .child(
                        screen.mark(
                            words,
                            gpui::svg()
                                .path(picture)
                                .size(px(20.0))
                                .text_color(if chosen {
                                    card_theme.text
                                } else {
                                    theme.accent
                                }),
                        ),
                    ),
            )
        })
        // Its menu.
        .child(
            round(("card-menu", id as usize), t("Row menu").into())
                .w(px(space::TOUCH - space::S))
                .text_size(px(NAME_TEXT + 4.0))
                .text_color(card_theme.muted)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.open_card_menu(id, cx);
                }))
                .child(screen.mark(t("Row menu"), div().child("⋮"))),
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
        // A tap opens the card, as a phone's list opens what is tapped: what a double-click on
        // its row does (Options › General), the card chosen first so the menus speak of it.
        let _ = clicks;
        self.choose_card(id);
        self.double_click(id, cx);
        cx.notify();
    }

    /// Card `id` chosen alone, as a click chooses a row.
    fn choose_card(&mut self, id: dm_core::Id) {
        self.click_row(id, false, false);
    }
}
