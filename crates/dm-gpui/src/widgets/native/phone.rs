// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A phone's own parts of a dialog: its top bar.

use gpui::{div, prelude::*, Div, FontWeight, Role, SharedString};

use crate::widgets::space;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// A phone's dialog's top bar: Back, and the dialog's name.
pub fn phone_bar(
    title: SharedString,
    theme: Theme,
    back: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Div {
    let back_words = dm_desktop_base::i18n::t("Back");
    div()
        .flex_none()
        .w_full()
        .h(px(space::TOUCH + space::XS))
        .flex()
        .items_center()
        .gap(px(space::XS))
        .px(px(space::XS))
        .border_b_1()
        .border_color(theme.separator)
        .child(
            div()
                .id("phone-back")
                .role(Role::Button)
                .aria_label(back_words)
                .size(px(space::TOUCH))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .map(|b| super::ringed(b, theme))
                .rounded_full()
                .active(move |s| s.bg(theme.control))
                .on_click(back)
                .map(|b| crate::widgets::press::first(b, space::TOUCH / 2.0, theme))
                .child(
                    gpui::svg()
                        .path("icons/back.svg")
                        .size(px(22.0))
                        .text_color(theme.text),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(super::FIGURE_TEXT))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(title),
        )
}

/// A dialog's `root` that closes as its window does, on a phone: a column under its own top
/// bar, `title` and Back (which closes it), so what follows can fill and scroll the rest. As
/// it was on a computer (gaps row N31).
pub fn phone_framed(
    root: gpui::Stateful<Div>,
    title: impl Into<SharedString>,
    theme: Theme,
) -> gpui::Stateful<Div> {
    if !crate::platform::ON_A_PHONE {
        return root;
    }
    root.flex()
        .flex_col()
        .child(phone_bar(title.into(), theme, |_, window, _| {
            window.remove_window()
        }))
}

/// A form's row whose control is a wide field (an address, a list of words): on a phone its
/// words above it and the field the line's whole width, so neither is cut; elsewhere
/// [`super::row`] (gaps row N31).
pub fn row_wide(
    label: Option<&str>,
    label_width: f32,
    screen: &crate::window::screen::Screen,
    control: impl IntoElement,
) -> Div {
    if !crate::platform::ON_A_PHONE {
        return super::row(label, label_width, screen, control);
    }
    div()
        .flex()
        .flex_col()
        .gap(px(space::XS))
        .when_some(label.map(str::to_string), |d, label| {
            d.child(screen.label(label))
        })
        .child(div().w_full().min_w_0().child(control))
}

/// A form's row whose control is small (a number's box, a choice): on a phone its words at
/// the left, wrapping where long, and the control at the right edge, as a phone's settings
/// are; elsewhere [`super::row`] (gaps row N31).
pub fn row_short(
    label: Option<&str>,
    label_width: f32,
    screen: &crate::window::screen::Screen,
    control: impl IntoElement,
) -> Div {
    if !crate::platform::ON_A_PHONE {
        return super::row::row_small(label, label_width, screen, control);
    }
    div()
        .flex()
        .items_center()
        .gap(px(space::L))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when_some(label.map(str::to_string), |d, label| {
                    d.child(screen.label(label))
                }),
        )
        .child(div().flex_none().child(control))
}
