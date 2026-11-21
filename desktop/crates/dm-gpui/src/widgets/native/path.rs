// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A field that holds a folder, shown as a person reads a path.

use gpui::{div, prelude::*, Entity, SharedString};
use gpui_component::input::InputState;

use super::{field, RADIUS, TEXT};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// A field that holds a folder: while it is not being typed in, it shows the path with the
/// home folder as `~` and cut in its middle where it is too long, so both where it starts and
/// the folder it ends in stay in sight (gaps row U17). Typing in it shows it whole, as it is.
/// Empty, it shows `hint` (the folder used when none is typed) the same way, in the
/// placeholder's colour, so its end ("(sorted by kind)") is not cut off.
pub fn path_field(
    input: &Entity<InputState>,
    name: &'static str,
    hint: &str,
    typing: bool,
    theme: Theme,
    cx: &gpui::App,
) -> impl IntoElement {
    let value = input.read(cx).value().to_string();
    let shown = (!typing).then(|| {
        if value.trim().is_empty() {
            (
                SharedString::from(dm_shell::settings::home_short(hint.trim())),
                theme.muted,
            )
        } else {
            (
                SharedString::from(dm_shell::settings::home_short(value.trim())),
                theme.text,
            )
        }
    });
    // On a phone the field draws its own text and hint over anything laid on it, so nothing is
    // laid on it there: the path, or its hint, shows as the field draws it.
    let shown = shown
        .filter(|(words, _)| !words.is_empty())
        .filter(|_| !crate::platform::ON_A_PHONE);
    field(input, name, theme).when_some(shown, |d, (shown, colour)| {
        d.child(
            div()
                .absolute()
                .top(px(1.0))
                .bottom(px(1.0))
                .left(px(1.0))
                .right(px(1.0))
                .rounded(px(RADIUS))
                .bg(theme.field)
                .px(px(5.0))
                .flex()
                .items_center()
                .overflow_hidden()
                .text_size(px(TEXT))
                .text_color(colour)
                .child(
                    div()
                        .w_full()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .child(shown),
                ),
        )
    })
}
