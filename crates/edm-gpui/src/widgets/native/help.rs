// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A dialog's "?": its page of the guide, built into EDM and opened in the browser offline
//! (gaps row E11, `edm_desktop_base::help`).

use gpui::{div, prelude::*, Div, Role, Stateful};

use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The "?" that opens the guide's page `page` (`docs/guide/<page>.md`), told as "Help" to a
/// screen reader; where the page cannot be written, nothing opens.
pub fn help_button(page: &'static str, theme: Theme, screen: &Screen) -> Stateful<Div> {
    let words = edm_desktop_base::i18n::t("Help");
    div()
        .id(gpui::ElementId::Name(format!("help-{page}").into()))
        .role(Role::Button)
        .aria_label(words)
        // Reached by Tab, pressed by Space or Return, ringed, as `native::button` is.
        .tab_index(0)
        .focus_visible(move |s| s.border_2().border_color(theme.ring(false)))
        .flex_none()
        .size(px(super::CONTROL))
        .rounded_full()
        .border_1()
        .border_color(theme.separator)
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme.muted)
        .hover(|s| s.bg(theme.stripe))
        .cursor_pointer()
        .child(screen.label("?"))
        .on_click(move |_, _, cx| {
            let home = edm_shell::settings::home();
            if let Ok(path) = edm_desktop_base::help::write_page(&home, page) {
                cx.open_url(&format!("file://{}", path.to_string_lossy()));
            }
        })
}
