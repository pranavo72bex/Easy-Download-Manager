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

/// What a card's round button does.
#[derive(Clone)]
enum Quick {
    /// Runs this command on the card.
    Run(dm_shell::app::commands::Command),
    /// Stops sharing the whole torrent whose files are here.
    StopSharing(String),
}

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
                    // A card just added fades in, as a list's row does (gaps row K7).
                    crate::window::table::arriving(
                        row.id,
                        div().child(card(this, row, i, kind, chosen, theme, &screen, cx)),
                    )
                })
                .collect::<Vec<_>>();
            // Cards drawn from now on, but the window's first, are ones just added.
            crate::window::table::list_drawn();
            drawn
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

/// What a card's round button does, with its picture and words: Stop while it runs or waits,
/// Resume while stopped or failed, Stop sharing for a whole torrent still shared; none else.
fn quick_of(row: &Entry) -> Option<(Quick, &'static str, &'static str)> {
    match row.state {
        State::Running | State::Queued => Some((
            Quick::Run(dm_shell::app::commands::Command::StopDownload),
            "icons/stop.svg",
            t("Stop"),
        )),
        State::Paused | State::Failed(_) => Some((
            Quick::Run(dm_shell::app::commands::Command::DownloadNow),
            "icons/resume.svg",
            t(dm_desktop_base::rows::resume_word(&[row])),
        )),
        // A whole torrent still shared: its sharing stopped in a tap, as a phone's torrent
        // apps pause a seeding torrent.
        State::Completed
            if dm_core::is_torrent(&row.url)
                && dm_core::sharing(std::path::Path::new(&row.path)).is_some() =>
        {
            Some((
                Quick::StopSharing(row.path.clone()),
                "icons/stop.svg",
                t("Stop sharing"),
            ))
        }
        State::Completed | State::Cancelled => None,
    }
}

/// A card's state in its colour, as a phone's torrent apps colour it: going in the accent,
/// failed in the warning's, finished in the success colour; on a chosen card, the choice's.
fn state_colour(row: &Entry, chosen: bool, theme: Theme) -> gpui::Rgba {
    if chosen {
        return theme.chosen().text;
    }
    match row.state {
        State::Running => theme.accent,
        State::Failed(_) => theme.warning,
        State::Completed => theme.success,
        _ => theme.muted,
    }
}

/// A card's line under its bar: on the left its state and size (for a running torrent, who it
/// comes from), on the right its speed (for a whole torrent still shared, what it sends).
fn figures(row: &Entry, status: &str, speed: String) -> (String, String) {
    let swarm = dm_core::is_torrent(&row.url)
        .then(|| dm_desktop_base::rows::swarm_of(row.id))
        .flatten();
    let size = size_words(row);
    let left = if size.is_empty() || matches!(row.state, State::Failed(_)) {
        status.to_string()
    } else if row.state == State::Running {
        // How far it is, the bar shows already: what has come of the whole is said.
        match swarm {
            Some(swarm) => dm_desktop_base::i18n::tf(
                "{} seeders · {} leechers",
                &[&swarm.seeders, &swarm.leechers()],
            ),
            None => size,
        }
    } else {
        format!("{status} · {size}")
    };
    let right = match swarm {
        Some(swarm) if row.state == State::Completed && swarm.connected > 0 => {
            dm_desktop_base::i18n::tf(
                "↑ {} · {} leechers",
                &[
                    &dm_desktop_base::format::rate(swarm.upload_speed),
                    &swarm.leechers(),
                ],
            )
        }
        _ => speed,
    };
    (left, right)
}

/// A card's small words, cut with "…" where they do not fit.
fn small(words: String, colour: gpui::Rgba, screen: &Screen) -> gpui::Div {
    div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(META_TEXT))
        .text_color(colour)
        .child(screen.label(SharedString::from(words)))
}

/// A round button on a card, a finger's size, whose tap is its own and not the card's.
fn round(
    id: (&'static str, usize),
    words: SharedString,
    theme: Theme,
) -> gpui::Stateful<gpui::Div> {
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
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Row `row`, the `i`th shown, as a card: its kind's tile, its name and time left, its bar and
/// figures, its round button and its menu.
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
    let card_theme = if chosen { theme.chosen() } else { theme };
    let (left_words, right_words) = figures(row, &status, speed);
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
            cx.listener(move |view, event: &gpui::MouseDownEvent, _, cx| {
                view.card_pressed(id, event.position, cx);
            }),
        )
        // A finger that moves is scrolling the list: no long press.
        .on_mouse_move(
            cx.listener(move |view, event: &gpui::MouseMoveEvent, _, _| {
                view.card_moved(id, event.position);
            }),
        )
        .on_scroll(cx.listener(|view, _, _, _| {
            view.layout.narrow.pressed = None;
        }))
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
        .relative()
        // A finger held on it: GPUI's own long press, claimed so it is no tap (gaps row C22).
        .child(long_press_layer(id, cx))
        .child(tile(kind, chosen, card_theme))
        // Its name and time left, its bar, and a line of its state and figures.
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
                            line.child(div().flex_none().child(small(
                                left,
                                card_theme.muted,
                                screen,
                            )))
                        }),
                )
                // No bar on a finished card: its tick says it (gaps row E4).
                // One just finished fills and fades rather than vanishing (gaps row K7).
                .children(crate::window::table::finishing_bar(row, card_theme))
                .when(row.state != State::Completed, |card| {
                    card.child(div().w_full().child(crate::window::table::progress_track(
                        row.id,
                        row.progress.fraction().unwrap_or(0.0),
                        false,
                        card_theme,
                    )))
                })
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap(px(space::M))
                        // A mark told by its shape, then the words (gaps row E4).
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(space::XS))
                                .min_w_0()
                                .children(crate::window::table::state_mark(&row.state, card_theme))
                                .child(small(left_words, state_colour(row, chosen, theme), screen)),
                        )
                        .child(div().flex_none().child(small(
                            right_words,
                            card_theme.muted,
                            screen,
                        ))),
                ),
        )
        .when_some(quick_of(row), |card, quick| {
            card.child(quick_button(id, quick, chosen, theme, screen, cx))
        })
        // Its menu.
        .child(
            round(("card-menu", id as usize), t("Row menu").into(), theme)
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

/// A card's kind, its picture on a tile of its colour.
fn tile(kind: Option<dm_core::Category>, chosen: bool, card_theme: Theme) -> gpui::Div {
    let (picture, colour) = kind_picture(kind);
    let tint = if chosen { card_theme.text } else { colour };
    div()
        .flex_none()
        .size(px(TILE))
        .rounded(px(TILE / 4.0))
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::Rgba { a: 0.16, ..tint })
        .child(gpui::svg().path(picture).size(px(24.0)).text_color(tint))
}

/// A card's round button, doing what [`quick_of`] says.
fn quick_button(
    id: dm_core::Id,
    (command, picture, words): (Quick, &'static str, &'static str),
    chosen: bool,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> gpui::Stateful<gpui::Div> {
    let card_theme = if chosen { theme.chosen() } else { theme };
    round(("card-quick", id as usize), words.into(), theme)
        .size(px(QUICK))
        .border_1()
        .border_color(if chosen {
            card_theme.text
        } else {
            theme.control_border
        })
        .on_click(cx.listener(move |view, _, _, cx| {
            view.choose_card(id);
            match &command {
                Quick::Run(command) => view.run(*command, cx),
                Quick::StopSharing(path) => {
                    dm_core::stop_sharing(std::path::Path::new(path));
                }
            }
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
        )
}

/// A layer over card `id` that takes GPUI's long press on it (a phone's touch held still, which
/// the platform's gesture recogniser reports as `LongPressEvent`, not as a held button): the
/// card's menu opens while the finger is still down, and the press is claimed, so it is not
/// also a tap that opens the download.
fn long_press_layer(id: dm_core::Id, cx: &mut Context<Downloads>) -> gpui::AnyElement {
    held_layer(cx, move |view, cx| {
        view.layout.narrow.pressed = None;
        view.layout.narrow.held_open = Some(id);
        view.open_card_menu(id, cx);
    })
}

/// A layer, over all of what it is put in, that runs `on_hold` when a finger is held still on
/// it (GPUI's `LongPressEvent`) and claims the press.
pub(crate) fn held_layer(
    cx: &mut Context<Downloads>,
    on_hold: impl Fn(&mut Downloads, &mut Context<Downloads>) + Clone + 'static,
) -> gpui::AnyElement {
    let view = cx.entity().downgrade();
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let view = view.clone();
            let on_hold = on_hold.clone();
            window.on_mouse_event(move |event: &gpui::LongPressEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Bubble
                    || event.phase != gpui::TouchPhase::Started
                    || !bounds.contains(&event.start_position)
                {
                    return;
                }
                window.prevent_default();
                // Felt, as a phone's own long press is (gaps row C22).
                dm_desktop_base::phone::felt();
                cx.stop_propagation();
                let _ = view.update(cx, |view, cx| on_hold(view, cx));
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// How far, in points, a finger may move from where it pressed a card and still be holding it:
/// past it, it is scrolling the list.
const MOVED_FROM_PRESS: f32 = 10.0;

impl Downloads {
    /// A tap on card `id`: held long, its menu; on the card already chosen alone, or twice,
    /// what a double-click on its row does (Options › General); else the card chosen alone.
    fn card_tapped(&mut self, id: dm_core::Id, clicks: usize, cx: &mut Context<Self>) {
        // The lift after a long press that already opened the menu.
        if self.layout.narrow.held_open.take() == Some(id) {
            self.layout.narrow.pressed = None;
            return;
        }
        // Choosing several: the card ticked or unticked; none left ends the choosing.
        if self.layout.narrow.choosing {
            // The press is over: its timer must not open the menu as well.
            self.layout.narrow.pressed = None;
            if !self.list.selected.remove(&id) {
                self.list.selected.insert(id);
            }
            self.list.cursor = Some(id);
            if self.list.selected.is_empty() {
                self.layout.narrow.choosing = false;
            }
            cx.notify();
            return;
        }
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

    /// A finger down on card `id` at `from`: held there for `LONG_PRESS`, the card's menu opens
    /// while it is still down, as a phone's long press does (gaps row C22).
    pub(crate) fn card_pressed(
        &mut self,
        id: dm_core::Id,
        from: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let at = std::time::Instant::now();
        self.layout.narrow.pressed = Some((id, at));
        self.layout.narrow.pressed_from = Some(from);
        self.layout.narrow.held_open = None;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(super::LONG_PRESS).await;
            let _ = this.update(cx, |view, cx| {
                // Still the same press, not lifted, moved or scrolled since.
                if view.layout.narrow.pressed != Some((id, at)) {
                    return;
                }
                view.layout.narrow.pressed = None;
                view.layout.narrow.held_open = Some(id);
                view.open_card_menu(id, cx);
            });
        })
        .detach();
    }

    /// The finger moved over card `id` to `to`: past a few points from where it went down, the
    /// press is a scroll, not a long press.
    pub(crate) fn card_moved(&mut self, id: dm_core::Id, to: gpui::Point<gpui::Pixels>) {
        let Some(((pressed, _), from)) = self
            .layout
            .narrow
            .pressed
            .zip(self.layout.narrow.pressed_from)
        else {
            return;
        };
        let (dx, dy) = (f32::from(to.x - from.x), f32::from(to.y - from.y));
        if pressed != id || dx * dx + dy * dy > MOVED_FROM_PRESS * MOVED_FROM_PRESS {
            self.layout.narrow.pressed = None;
        }
    }

    /// The finger lifted from card `id`, as a tap ends: what [`Self::card_tapped`] makes of it.
    #[cfg(test)]
    pub(crate) fn card_lifted(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        self.card_tapped(id, 1, cx);
    }

    /// Card `id` chosen alone, as a click chooses a row.
    fn choose_card(&mut self, id: dm_core::Id) {
        self.click_row(id, false, false);
    }
}
