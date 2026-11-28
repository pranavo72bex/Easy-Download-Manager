// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
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
    let long =
        screen.width() > 0.0 && label.is_some_and(|label| text_width(label) > 0.4 * screen.width());
    row_beside(label, label_width, screen, control, long)
}

/// [`row`] for a small control (a number's box, a choice; `phone::row_short`): its label above
/// it only where the window leaves the control less than a number's box beside the labels'
/// column. A label past 40% of the window put "Download at most this many MB" over its box
/// in the Speed limiter, sized to hold it beside, its box at the left of the others' column.
pub(crate) fn row_small(
    label: Option<&str>,
    label_width: f32,
    screen: &Screen,
    control: impl IntoElement,
) -> Div {
    let room = screen.width() - 2.0 * super::MARGIN - label_width - LABEL_GAP;
    let long = screen.width() > 0.0 && label.is_some() && room < super::NUMBER_FIELD;
    row_beside(label, label_width, screen, control, long)
}

/// A form's row, its label above its control where `long`, else beside it.
fn row_beside(
    label: Option<&str>,
    label_width: f32,
    screen: &Screen,
    control: impl IntoElement,
    long: bool,
) -> Div {
    // A phone's form: the label above its control, the control from the same edge on every
    // line, a field as wide as the line, a small control at its own size (`row_short` puts
    // one at the right of its words). Beside one another the controls began at as many
    // places as there were label widths, and a long label left a field a sliver (gaps N31).
    // And a computer's where `long` (rule 6, gaps row E32): beside its field a long label left
    // the field a sliver.
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

/// A fact a window reports (a size, a count, where a file goes): its name in the hint's colour
/// from the window's edge in a column `name_width` wide, its value beside it, as the progress
/// window, View › Details and Statistics show theirs. Each drew its own, two with the names
/// right-aligned mid-window. On a phone the same where the names are short (an Android
/// settings row's name and value): stacked, three short facts took 380 dp of the progress window
/// (unslop review on the emulator); [`row`]'s, the name above its value, where a language's
/// names are wider than [`PHONE_FACT_NAME`].
pub fn fact(
    name: &str,
    name_width: f32,
    theme: crate::window::theme::Theme,
    screen: &Screen,
    value: impl IntoElement,
) -> Div {
    let value = div().flex_1().min_w_0().child(value);
    if crate::platform::ON_A_PHONE && name_width > PHONE_FACT_NAME {
        return row(Some(name), name_width, screen, value);
    }
    div()
        .flex()
        .map(crate::window::mirror::row)
        .items_start()
        .gap(px(LABEL_GAP))
        .child(
            div()
                .w(px(name_width))
                .flex_none()
                .text_color(theme.muted)
                .child(screen.label(name.to_string())),
        )
        .child(value)
}

/// The widest a fact's names may be, in points, for a phone to set each beside its value.
const PHONE_FACT_NAME: f32 = 140.0;

/// What a field's value means, under the field in the fields' column of a form whose labels are
/// `label_width` wide: nearer its field than the next row is (a hint stood midway between them),
/// as Options' hints stand (`options::pages`).
pub fn hint_under(
    words: impl Into<gpui::SharedString>,
    label_width: f32,
    theme: crate::window::theme::Theme,
    screen: &Screen,
) -> Div {
    row(
        None,
        label_width,
        screen,
        super::note(words, theme.muted, screen),
    )
    .mt(px(-super::note::HINT_PULL))
    .mb(px(super::note::AFTER_HINT))
}
