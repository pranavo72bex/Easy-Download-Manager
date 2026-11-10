// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A pop-up button, as macOS's own: what is chosen, and a click opens the
//! system's own menu of the choices (an `NSMenu`, through gpui-component's
//! `NativeMenu`; drawn by GPUI where the system has none), the chosen one
//! ticked. The choice comes back as a [`Pick`] action to the window's view.

use gpui::{div, prelude::*, px, Action, App, Role, SharedString, Window};
use gpui_component::native_menu::NativeMenu;

use crate::screen::Screen;
use crate::theme::Theme;

/// The choice `index` made in the pop-up `field` (of a list's `item`,
/// where there is one a row, as a torrent's files each have one).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct Pick {
    /// Which pop-up.
    pub field: &'static str,
    /// Which row's, where each row of a list has one; else 0.
    pub item: usize,
    /// The choice's place in its list.
    pub index: usize,
}

/// The pop-up `field`, showing `words`; a click opens `choices`, the one at
/// `chosen` ticked.
pub fn button(
    field: &'static str,
    words: SharedString,
    choices: Vec<SharedString>,
    chosen: Option<usize>,
    theme: Theme,
    screen: &Screen,
) -> gpui::Stateful<gpui::Div> {
    button_of(field, 0, words, choices, chosen, theme, screen)
}

/// The pop-up `field` of a list's row `item`, as [`button`].
pub fn button_of(
    field: &'static str,
    item: usize,
    words: SharedString,
    choices: Vec<SharedString>,
    chosen: Option<usize>,
    theme: Theme,
    screen: &Screen,
) -> gpui::Stateful<gpui::Div> {
    use crate::native::{CONTROL, RADIUS, TEXT};
    div()
        .id((field, item))
        .role(Role::ComboBox)
        .aria_label(words.clone())
        .h(px(CONTROL))
        .min_w(px(100.0))
        .pl(px(9.0))
        .pr(px(5.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(theme.control_border)
        .bg(theme.control)
        .text_size(px(TEXT))
        .whitespace_nowrap()
        .child(screen.label(words))
        // The system's up-and-down chevrons, at the trailing end.
        .child(
            gpui::svg()
                .path("icons/chevron-up-down.svg")
                .size(px(11.0))
                .flex_none()
                .text_color(theme.muted),
        )
        .on_click(move |event, window, cx| {
            open(field, item, &choices, chosen, event.position(), window, cx)
        })
}

/// The system's menu of `choices`, at `at`.
fn open(
    field: &'static str,
    item: usize,
    choices: &[SharedString],
    chosen: Option<usize>,
    at: gpui::Point<gpui::Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let menu = choices
        .iter()
        .enumerate()
        .fold(NativeMenu::new(), |menu, (index, words)| {
            menu.menu_with_check(
                words.clone(),
                chosen == Some(index),
                Box::new(Pick { field, item, index }),
            )
        });
    menu.show(at, window, cx);
}
