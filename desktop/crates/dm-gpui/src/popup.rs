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

/// The choice `index` made in the pop-up `field`.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct Pick {
    /// Which pop-up.
    pub field: &'static str,
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
    div()
        .id(field)
        .role(Role::ComboBox)
        .aria_label(words.clone())
        .h(px(22.0))
        .min_w(px(120.0))
        .px_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(theme.separator)
        .bg(theme.stripe)
        .child(screen.label(words))
        .child(div().text_color(theme.muted).child("⌄"))
        .on_click(move |event, window, cx| {
            open(field, &choices, chosen, event.position(), window, cx)
        })
}

/// The system's menu of `choices`, at `at`.
fn open(
    field: &'static str,
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
                Box::new(Pick { field, index }),
            )
        });
    menu.show(at, window, cx);
}
