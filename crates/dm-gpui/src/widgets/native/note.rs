// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Words under a control, and a dialog's failure note that a screen reader hears.

use gpui::{div, prelude::*, Div, Role, SharedString, Stateful};

use super::SMALL_TEXT;
use crate::window::a11y::Told;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// Words said under a control, or of a warning: the small system size.
pub fn note(words: impl Into<SharedString>, color: gpui::Rgba, screen: &Screen) -> Div {
    div()
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
