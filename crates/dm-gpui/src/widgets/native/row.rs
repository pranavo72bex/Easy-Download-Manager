// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A form's row: its label and its control, beside one another or the label above.

use gpui::{div, prelude::*, Div};

use super::{text_width, LABEL_GAP};
use crate::widgets::space;
use crate::window::screen::Screen;
use crate::window::zoom::px;

/// A form's row: `label` right-aligned in a column `label_width` wide, its
/// control beside it.
pub fn row(
    label: Option<&str>,
    label_width: f32,
    screen: &Screen,
    control: impl IntoElement,
) -> Div {
    // A phone's form: the label above its control, the control from the same edge on every
    // line, a field as wide as the line, a small control at its own size (`row_short` puts
    // one at the right of its words). Beside one another the controls began at as many
    // places as there were label widths, and a long label left a field a sliver (gaps N31).
    // And a computer's where its label passes 40% of its window (rule 6, gaps row E32): beside
    // its field it left the field a sliver.
    let long =
        screen.width() > 0.0 && label.is_some_and(|label| text_width(label) > 0.4 * screen.width());
    if crate::platform::ON_A_PHONE || long {
        return div()
            .flex()
            .flex_col()
            .gap(px(space::XS))
            .when_some(label.map(str::to_string), |d, label| {
                d.child(screen.label(label))
            })
            // A block the line's width: a note's words wrap at the screen's edge (as flex
            // items they kept their one line's width and ran off it).
            .child(div().w_full().min_w_0().child(control));
    }
    // Mirrored where the language reads right to left: the label at the right, its words
    // against its field (gaps row E36).
    let rtl = crate::window::mirror::rtl();
    div()
        .flex()
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(LABEL_GAP))
        .child(
            // At least the column's width, and wider where a label is longer than measured,
            // so its words push the control along rather than begin outside the window.
            div()
                .min_w(px(label_width))
                .flex_none()
                .flex()
                .map(|d| {
                    if rtl {
                        d.justify_start()
                    } else {
                        d.justify_end()
                    }
                })
                .whitespace_nowrap()
                .when_some(label.map(str::to_string), |d, label| {
                    d.child(screen.label(label))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                // Its control against its label, right to left too.
                .map(crate::window::mirror::row)
                .items_center()
                .gap(px(LABEL_GAP))
                .child(control),
        )
}
