// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' list of pages: beside the page at a computer's width, a screen of its own at a
//! phone's (gaps row C21).

use dm_desktop_base::i18n::t;
use dm_desktop_base::options::{self, Tab};
use gpui::{div, prelude::*, Context, Role, SharedString};

use super::Options;
use crate::widgets::{native, space};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

impl Options {
    /// A page's entry in the list of pages: its name and what it holds (gaps row C21), chosen
    /// by a click, Space or Return, the arrows moving from page to page.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn page_tab(
        &self,
        tab: Tab,
        words: &'static str,
        summary: String,
        narrow: bool,
        theme: Theme,
        screen: &Screen,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        // Narrow, the list is a screen of its own and no page is shown beside it as chosen.
        let on = self.tab == tab && !narrow;
        div()
            .id(SharedString::from(words))
            .role(Role::Tab)
            // Reached by Tab, chosen by Space or Return (gaps row U2).
            .tab_index(0)
            // Focus draws a ring, never a fill: a fill drawn over the chosen page's left its white
            // words on a light grey after every click (1.3 to 1), and a phone's chip lost its
            // colour (gaps row C21).
            .focus(move |s| s.border_2().border_color(theme.ring(on)))
            .aria_label(SharedString::from(t(words)))
            .aria_selected(on)
            .w_full()
            .px(px(space::M))
            .py(px(if narrow { space::M } else { space::S }))
            .flex()
            .items_center()
            .gap(px(space::M))
            .rounded(px(native::RADIUS))
            .cursor_pointer()
            .when(narrow, |row| {
                row.border_b_1()
                    .border_color(theme.separator)
                    .rounded_none()
            })
            .when(on, |d| d.bg(theme.accent).text_color(theme.on_accent))
            // The others answer the pointer as a faceless row does (`docs/ui-design.md`).
            .when(!on, |d| d.hover(move |s| s.bg(theme.stripe)))
            // Its name, and what it holds in a line under it.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(space::XS))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(screen.label(t(words))),
                    )
                    .when(!summary.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(native::SMALL_TEXT))
                                .when(!on, |s| s.text_color(theme.muted))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(screen.label(summary)),
                        )
                    }),
            )
            .when(narrow, |row| {
                row.child(div().flex_none().text_color(theme.muted).child("›"))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.show(tab, cx)))
            // On the pages' strip, the arrows move from page to page, the keyboard's
            // place with them, as a tab view's do (gaps row U2).
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    let by = match event.keystroke.key.as_str() {
                        "left" | "up" => -1,
                        "right" | "down" => 1,
                        _ => return,
                    };
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    cx.stop_propagation();
                    let at = options::TABS
                        .iter()
                        .position(|(t, _)| *t == tab)
                        .unwrap_or(0);
                    let last = options::TABS.len() - 1;
                    // Not round the end, where the next place to go is another control.
                    if (by < 0 && at == 0) || (by > 0 && at == last) {
                        return;
                    }
                    this.step_page(by, cx);
                    if by > 0 {
                        window.focus_next(cx);
                    } else {
                        window.focus_prev(cx);
                    }
                }),
            )
    }
}
