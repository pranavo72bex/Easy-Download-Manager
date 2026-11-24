// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The playlists and feeds followed, in the side panel under Grabber projects (gaps row W10):
//! each by its title, its address on hover; a click asks Check now, Stop following or Close
//! (`Downloads::follow_asked`).

use gpui::{div, prelude::*, Context, Role, SharedString};

use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// Each followed one's row, drawn as a saved search's is.
pub(super) fn rows(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> Vec<gpui::AnyElement> {
    view.known
        .drawn
        .follows
        .iter()
        .enumerate()
        .map(|(i, follow)| {
            let follow = follow.clone();
            let name: SharedString = follow.name().to_string().into();
            let address: SharedString = follow.address.clone().into();
            div()
                .id(("followed", i))
                .role(Role::ListItem)
                .aria_label(name.clone())
                .map(|d| super::reachable(d, theme))
                .mx_2()
                .px_2()
                .h(px(super::PLACE_ROW))
                .rounded(px(crate::widgets::native::RADIUS))
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(crate::widgets::native::TEXT))
                .hover(|s| s.bg(theme.stripe))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(address.clone()).build(window, cx)
                })
                .child(
                    gpui::svg()
                        .path("icons/place-queue.svg")
                        .size(px(crate::widgets::native::ICON))
                        .flex_none()
                        .text_color(theme.accent_text),
                )
                .child(screen.label(name))
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.follow_asked(follow.clone(), cx);
                }))
                .into_any_element()
        })
        .collect()
}
