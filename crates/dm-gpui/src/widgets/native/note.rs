// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A section's heading, words under a control, and a dialog's failure note that a screen
//! reader hears.

use gpui::{div, prelude::*, Div, FontWeight, Role, SharedString, Stateful};

use super::SMALL_TEXT;
use crate::widgets::space;
use crate::window::a11y::Told;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// Words said under a control, or of a warning: the small system size. Free to be narrower
/// than its one line, so beside a form's labels it wraps at the window's edge (Back up's
/// opening words ran off it).
pub fn note(words: impl Into<SharedString>, color: gpui::Rgba, screen: &Screen) -> Div {
    div()
        .min_w_0()
        .text_size(px(SMALL_TEXT))
        .text_color(color)
        .child(screen.label(words))
}

/// A dialog's failure note, said to a screen reader as it appears (gaps row E24): a polite
/// live region that is always there, nameless and of no room while there is nothing to say,
/// so the system hears its name change rather than a node arrive. Pressing OK on a dialog that
/// refused made no sound.
pub fn says(
    id: impl Into<gpui::ElementId>,
    words: Option<SharedString>,
    theme: Theme,
    screen: &Screen,
) -> Stateful<Div> {
    let region = div()
        .id(id)
        .role(Role::Status)
        .aria_label(words.clone().unwrap_or_default())
        .said_aloud();
    match words {
        Some(words) => region.child(note(words, theme.warning, screen)),
        // Out of the flow, so a column's gap is not put beside it.
        None => region.absolute().size(px(1.0)),
    }
}

/// A section's heading, one kind in every dialog and page (gaps row E29): semibold, one gap
/// above it; a phone's in the accent colour, smaller and set further apart, as a phone's
/// settings head each group. Options, Statistics, the Scheduler, Save to and the playlist each
/// drew their own, with four top gaps among them.
pub fn heading(words: impl Into<SharedString>, theme: Theme, screen: &Screen) -> Div {
    div()
        // Its section a group of its own: twice a row's gap and more above it, and nearer the
        // first row under it than the rows are to each other (`better-layout`: a group apart
        // by space). At 10 above and the column's 8 below it stood midway between sections.
        .mt(px(space::XXL))
        .mb(px(-space::XS))
        .font_weight(FontWeight::SEMIBOLD)
        .when(crate::platform::ON_A_PHONE, |heading| {
            heading
                .mt(px(space::XL))
                .text_size(px(SMALL_TEXT))
                .text_color(theme.accent_text)
        })
        .child(screen.label(words))
}

/// `words` on one line, cut where they do not fit (in the middle for a path or an address,
/// `middle`, so its ending stays), and read whole on hover, as the list's cells are (gaps row
/// E35): a cut name or path had no way to be read.
pub fn cut(
    id: impl Into<gpui::ElementId>,
    words: impl Into<SharedString>,
    middle: bool,
    screen: &Screen,
) -> Stateful<Div> {
    let words: SharedString = words.into();
    let whole = words.clone();
    div()
        .id(id)
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .map(|d| {
            if middle {
                d.text_ellipsis_middle()
            } else {
                d.text_ellipsis()
            }
        })
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(whole.clone()).build(window, cx)
        })
        .child(screen.label(words))
}
