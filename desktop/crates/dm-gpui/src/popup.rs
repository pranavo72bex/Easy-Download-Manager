// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A pop-up button, as macOS's own: what is chosen, and a click opens the
//! system's own menu of the choices (an `NSMenu`, through gpui-component's
//! `NativeMenu`; drawn by GPUI where the system has none), the chosen one
//! ticked. The choice comes back as a [`Pick`] action to the window's view.
//! The system's menu waits for a person, so a click the control port gives
//! a pop-up holds its choices instead ([`held`]), and the port's next click
//! on one of their words picks it, as a person's would.

use gpui::{div, prelude::*, Action, App, Role, SharedString, Window};
use gpui_component::native_menu::NativeMenu;

use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

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

/// A pop-up the control port clicked: its window, which pop-up, and its
/// choices, waiting for the port to click one.
#[derive(Clone)]
pub struct Held {
    pub window: gpui::AnyWindowHandle,
    pub field: &'static str,
    pub item: usize,
    pub choices: Vec<SharedString>,
}

thread_local! {
    /// Whether the click being given is the control port's.
    static SCRIPTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The pop-up the port clicked last, until one of its choices is.
    static HELD: std::cell::RefCell<Option<Held>> = const { std::cell::RefCell::new(None) };
}

/// Gives the control port's click: pop-ups hold their choices instead of
/// opening the system's menu, which would wait for a person.
pub fn scripted<R>(click: impl FnOnce() -> R) -> R {
    SCRIPTED.with(|s| s.set(true));
    let done = click();
    SCRIPTED.with(|s| s.set(false));
    done
}

/// The pop-up the port clicked, taken.
pub fn held() -> Option<Held> {
    HELD.with(|h| h.borrow_mut().take())
}

/// The pop-up the port clicked, put back (its choice was not among them).
pub fn hold(held: Held) {
    HELD.with(|h| *h.borrow_mut() = Some(held));
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
        // Reached by Tab, opened by Space or Return (gaps row U2).
        .tab_index(0)
        .focus(move |s| s.border_2().border_color(theme.accent))
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
            if SCRIPTED.with(|s| s.get()) {
                hold(Held {
                    window: window.window_handle(),
                    field,
                    item,
                    choices: choices.clone(),
                });
                return;
            }
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
