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
            .focus(move |s| s.border_2().border_color(theme.ring(false)))
            // Beside a page, its edge there unseen until then, so the ring does not move the
            // words (a phone's row keeps its line under it).
            .when(!narrow, |row| {
                row.border_2().border_color(gpui::transparent_black())
            })
            .aria_label(SharedString::from(t(words)))
            .aria_selected(on)
            .w_full()
            .px(px(space::M))
            .relative()
            .px(px(space::M))
            // Beside a page, close-set: all six pages, four with two lines, fit Windows' 520
            // points with room for its wider text (Sign-ins fell below the list's edge there).
            // Its unseen edge (below) is two of the four points above and below its words.
            .py(px(if narrow { space::M } else { space::XXS }))
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
            // The chosen page as the side panel's chosen place: a soft fill with the accent's bar
            // at its edge, its words in the text's colour. A filled accent drew a second default
            // button beside OK (`docs/ui-design.md` rule 2).
            .when(on, |d| {
                d.bg(theme.chosen_soft())
                    .child(crate::window::sidebar::chosen_bar(theme))
            })
            // The others answer the pointer as a faceless row does (`docs/ui-design.md`).
            .when(!on, |d| d.hover(move |s| s.bg(theme.stripe)))
            // Its picture and name, and what it holds in a line under the name, lined up with it.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(space::XXS))
                    .child(
                        div()
                            .flex()
                            .map(crate::window::mirror::row)
                            .items_center()
                            .gap(px(space::M))
                            .child(
                                gpui::svg()
                                    .path(picture(tab))
                                    .size(px(native::ICON))
                                    .flex_none()
                                    // The accent's text shade, as the side panel's places.
                                    .text_color(theme.accent_text),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .font_weight(if on {
                                        gpui::FontWeight::SEMIBOLD
                                    } else {
                                        gpui::FontWeight::MEDIUM
                                    })
                                    .child(screen.label(t(words))),
                            ),
                    )
                    .when(!summary.is_empty(), |d| {
                        d.child(
                            div()
                                // Under the name, past the picture.
                                .map(|line| {
                                    let past = px(native::ICON + space::M);
                                    if crate::window::mirror::rtl() {
                                        line.pr(past)
                                    } else {
                                        line.pl(past)
                                    }
                                })
                                .text_size(px(native::SMALL_TEXT))
                                // The text's colour on the soft fill, where the secondary one
                                // read under 4.5 to 1 (`the_soft_choice_keeps_its_words_readable`).
                                .text_color(if on { theme.text } else { theme.muted })
                                // Two lines where it needs them, as the room below allows (gaps
                                // row E35): cut at one, "Once it is unpacked" was lost.
                                .line_clamp(2)
                                // Longer still: ended with "…", not cut through a word.
                                .text_ellipsis()
                                .child(screen.label(summary)),
                        )
                    }),
            )
            .when(narrow, |row| {
                row.child(crate::widgets::native::icon(
                    crate::window::mirror::forward(),
                    crate::widgets::native::ICON_SMALL,
                    theme.muted,
                ))
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

/// A page's picture beside its name in the list, as the side panel's places have one.
fn picture(tab: Tab) -> &'static str {
    match tab {
        Tab::General => "icons/options.svg",
        Tab::FileTypes => "icons/grabber.svg",
        Tab::SaveTo => "icons/place-folder.svg",
        Tab::Downloads => "icons/place-all.svg",
        Tab::Advanced => "icons/page-advanced.svg",
        Tab::SiteLogins => "icons/page-sign-ins.svg",
    }
}
