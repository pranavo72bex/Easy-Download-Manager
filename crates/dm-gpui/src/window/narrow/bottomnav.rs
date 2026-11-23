// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The bottom navigation at a phone's width, as a phone's download and torrent apps have one
//! (the maintainer, 2026-11-21): All and Unfinished a thumb's tap away, Places, which opens the
//! drawer of every place (Finished, the categories and queues), and Options (the maintainer,
//! 2026-11-23: "add Options in navigation bar instead of Finished option"; it stays in the
//! menu too). A place is the side panel's own, chosen as the side panel chooses it
//! (`Downloads::choose`).

use dm_desktop_base::i18n::t;
use dm_shell::app::places::Place;
use gpui::{div, prelude::*, Context, Div, Role, SharedString, Stateful};

use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The bar's height, as Material's navigation bar (80 dp) less the room a phone's own
/// navigation bar already keeps under it.
pub(crate) const HEIGHT: f32 = 68.0;

/// A tab's picture, as Material's: 24 points.
const ICON: f32 = 24.0;

/// What a tab does.
#[derive(Clone)]
enum Tab {
    /// Shows this place's rows.
    Place(Place),
    /// Opens the drawer of every place.
    Drawer,
    /// Opens Options, as the menu's Options does.
    Options,
}

/// The bottom navigation.
pub(super) fn bottom_nav(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> Stateful<Div> {
    let place = &view.list.place;
    let drawer_open = view.layout.narrow.drawer.is_some();
    // Places is lit for a place no other tab is (Finished, a category, a queue) and while its
    // drawer is open.
    let elsewhere = !matches!(place, Place::All | Place::Unfinished);
    let tabs = [
        (
            Tab::Place(Place::All),
            "icons/place-all.svg",
            t("All"),
            *place == Place::All && !drawer_open,
        ),
        (
            Tab::Place(Place::Unfinished),
            "icons/place-unfinished.svg",
            t("Unfinished"),
            *place == Place::Unfinished && !drawer_open,
        ),
        (
            Tab::Drawer,
            "icons/sidebar.svg",
            t("Places"),
            elsewhere || drawer_open,
        ),
        (Tab::Options, "icons/options.svg", t("Options"), false),
    ];
    div()
        .id("bottom-nav")
        .role(Role::TabList)
        .flex_none()
        .h(px(HEIGHT))
        .flex()
        .items_center()
        .bg(crate::window::theme::frame(theme.sidebar))
        .border_t_1()
        .border_color(theme.separator)
        .children(
            tabs.into_iter()
                .enumerate()
                .map(|(i, (tab, picture, words, lit))| {
                    tab_drawn(i, tab, picture, words, lit, theme, screen, cx)
                }),
        )
}

/// One tab: its picture in a pill lit where it is the one shown, its word under it.
#[allow(clippy::too_many_arguments)]
fn tab_drawn(
    i: usize,
    tab: Tab,
    picture: &'static str,
    words: &'static str,
    lit: bool,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let colour = if lit { theme.text } else { theme.muted };
    div()
        .id(("bottom-tab", i))
        .role(Role::Tab)
        .aria_label(SharedString::from(words))
        .aria_selected(lit)
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(space::XS))
        .on_click(cx.listener(move |view, _, _, cx| {
            match &tab {
                Tab::Place(place) => {
                    view.layout.narrow.drawer = None;
                    view.layout.narrow.sheet = None;
                    view.choose(place.clone());
                }
                Tab::Drawer => {
                    view.toggle_drawer(cx);
                }
                Tab::Options => {
                    view.layout.narrow.drawer = None;
                    view.layout.narrow.sheet = None;
                    view.open_options(cx);
                }
            }
            cx.notify();
        }))
        .child(
            div()
                .w(px(56.0))
                .h(px(30.0))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .when(lit, |pill| pill.bg(theme.selection))
                .child(gpui::svg().path(picture).size(px(ICON)).text_color(if lit {
                    theme.chosen().text
                } else {
                    colour
                })),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colour)
                .when(lit, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                .child(screen.label(SharedString::from(words))),
        )
}
