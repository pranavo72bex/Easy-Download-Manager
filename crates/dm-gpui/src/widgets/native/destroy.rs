// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A button that destroys, and the line that offers Undo after one (`docs/ui-design.md` rule
//! 3): drawn one way in every dialog.

use gpui::{div, prelude::*, Div, SharedString, Stateful};

use super::{button, LABEL_GAP};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// What a destroying button just did, in a line, and Undo beside it: the one way a dialog
/// offers a way back (the list's Remove says it in the status bar the same way).
pub fn undo_line(
    id: &'static str,
    said: String,
    theme: Theme,
    screen: &Screen,
    undo: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Div {
    div()
        .flex()
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(LABEL_GAP))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(theme.muted)
                .child(screen.label(said)),
        )
        .child(
            button(
                id,
                dm_desktop_base::i18n::t("Undo").into(),
                false,
                theme,
                screen,
            )
            .on_click(undo),
        )
}

/// A plain button that destroys (Delete, Clear, Remove): its words in the error colour, never a
/// filled red button (`docs/ui-design.md` rule 3); the one way every dialog draws one.
pub fn destructive(
    id: impl Into<gpui::ElementId>,
    words: SharedString,
    theme: Theme,
    screen: &Screen,
) -> Stateful<Div> {
    button(id, words, false, theme, screen).text_color(theme.warning)
}
