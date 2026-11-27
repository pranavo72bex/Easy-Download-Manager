// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A dialog's window as tall as what it holds, as the system's own dialogs
//! are: a line that appears (a warning, a sign-in's fields) grows the
//! window rather than pushing its buttons past the bottom edge.

use gpui::{canvas, div, prelude::*, px, size};

/// `content` at its own height, the window made that tall once it is laid
/// out (on the next frame, where it differs by more than a point).
/// On a phone, whose dialog fills the screen and cannot grow, `content` scrolls in the room
/// under its top bar instead, so its foot and buttons stay in reach (gaps row N31).
pub fn fitted(content: impl IntoElement) -> gpui::AnyElement {
    if crate::platform::ON_A_PHONE {
        return div()
            .id("fitted")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .child(content)
            .into_any_element();
    }
    div()
        .relative()
        .w_full()
        .child(content)
        .child(
            canvas(
                |bounds, window, _| {
                    let want = bounds.size.height;
                    let have = window.viewport_size();
                    if (want - have.height).abs() > px(1.0) {
                        let width = have.width;
                        window.on_next_frame(move |window, _| window.resize(size(width, want)));
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .into_any_element()
}

/// `body` and, under it, `foot` (a dialog's buttons), [`fitted`]. On a phone the foot stays at the
/// screen's foot above a hairline, as Options' does, and the body scrolls over it: in Download
/// details and File properties the buttons were the last of what scrolled, and a phone showed
/// only their tops at the screen's edge (unslop review on the emulator, 2026-11-27).
pub fn fitted_with_foot(
    body: gpui::Div,
    foot: impl IntoElement,
    theme: crate::window::theme::Theme,
) -> gpui::AnyElement {
    if !crate::platform::ON_A_PHONE {
        return fitted(body.child(foot));
    }
    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .child(fitted(body))
        .child(
            div()
                .flex_none()
                .w_full()
                .px(px(crate::widgets::native::MARGIN))
                .py(px(crate::widgets::space::M))
                .border_t_1()
                .border_color(theme.separator)
                .child(foot),
        )
        .into_any_element()
}
