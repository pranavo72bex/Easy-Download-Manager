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
pub fn fitted(content: impl IntoElement) -> impl IntoElement {
    div().relative().w_full().child(content).child(
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
}
