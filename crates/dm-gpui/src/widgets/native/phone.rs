// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A phone's own parts of a dialog: its top bar.

use gpui::{div, prelude::*, Div, FontWeight, Role, SharedString};

use super::TEXT;
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
                .rounded_full()
                .active(move |s| s.bg(theme.control))
                .on_click(back)
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
                .text_size(px(TEXT + 4.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(title),
        )
}
