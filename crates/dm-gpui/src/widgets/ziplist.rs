// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A ZIP's files as Download details and Properties list them: one box and one line each, drawn
//! one way (gaps row E35: the same list was drawn twice, its names cut with no way to read them).

use dm_desktop_base::format;
use dm_desktop_base::i18n::tf;
use gpui::{div, prelude::*, AnyElement, Div, Stateful};

use crate::widgets::{native, space};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The box the files are listed in, scrolling past 140 points, with "and N more" under them.
pub(crate) fn zip_box(
    lines: impl IntoIterator<Item = AnyElement>,
    more: usize,
    theme: Theme,
    screen: &Screen,
) -> Stateful<Div> {
    div()
        .id("zip-contents")
        .max_h(px(140.0))
        .overflow_y_scroll()
        .rounded(px(native::RADIUS))
        .border_1()
        .border_color(theme.separator)
        .bg(theme.background)
        .p(px(space::XS))
        .children(lines)
        .when(more > 0, |d| {
            d.child(native::note(
                tf("and {} more", &[&more.to_string()]),
                theme.muted,
                screen,
            ))
        })
}

/// File `i` of the box: its name, cut in the middle where long and whole on hover, and its size
/// where it is a file; what follows (Take out) is the caller's.
pub(crate) fn zip_line(i: usize, file: &dm_core::ZipEntry, theme: Theme, screen: &Screen) -> Div {
    let size = (!file.is_folder()).then(|| format::bytes(file.size));
    div()
        .flex()
        .items_center()
        .gap(px(space::M))
        .h(px(20.0))
        .child(native::cut(("zip-name", i), file.name.clone(), true, screen).flex_1())
        .when_some(size, |d, size| {
            d.child(div().text_color(theme.muted).child(screen.label(size)))
        })
}
