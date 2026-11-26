// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' list of pages: beside the page at a computer's width, a screen of its own at a
//! phone's (gaps row C21).

use dm_desktop_base::i18n::t;
use dm_desktop_base::options::{self, Tab};
use gpui::{div, prelude::*, Context, Div, Role, SharedString, Stateful, Window};

use super::Options;
use crate::widgets::{native, space};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

impl Options {
    /// The list of pages, each with its picture, name and what it holds: beside a page, rows
    /// close-set as the main window's side panel's places; at a phone's width, a screen of its own.
    pub(super) fn page_list(
        &mut self,
        tabs: &[(Tab, &'static str)],
        narrow: bool,
        theme: Theme,
        screen: &Screen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let entries: Vec<_> = tabs
            .iter()
            .map(|(tab, words)| {
                let summary = crate::options::pages::summary(self, *tab, theme, window, cx);
                self.page_tab(*tab, words, summary, narrow, theme, screen, cx)
            })
            .collect();
        div()
            .id("pages")
            .role(Role::TabList)
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .when(narrow, |l| {
                l.flex_1().min_h_0().w_full().px(px(native::MARGIN))
            })
            .when(!narrow, |l| {
                l.flex_1()
                    .min_h_0()
                    .gap(px(space::XXS))
                    .px(px(space::M))
                    .pb(px(space::M))
            })
            .children(entries)
    }

    /// A page's entry in the list of pages, chosen by a click, Space or Return, the arrows
    /// moving from page to page. Beside a page, a source list's row: its picture and name on one
    /// line, as the main window's side panel draws a place, what it holds said at the top of the
    /// page. At a phone's width, where the list is a screen of its own, a row with
    /// what it holds under its name and an arrow on (gaps row C21).
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
        let entry = div()
            .id(SharedString::from(words))
            .role(Role::Tab)
            .aria_label(SharedString::from(t(words)))
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
            );
        if narrow {
            self.phone_row(entry, words, summary, theme, screen)
        } else {
            self.source_row(entry, tab, words, theme, screen)
        }
    }

    /// Beside a page: the side panel's row (`window::sidebar`), its height, margins, picture and
    /// chosen look, so Options and the main window read as one.
    fn source_row(
        &self,
        entry: gpui::Stateful<gpui::Div>,
        tab: Tab,
        words: &'static str,
        theme: Theme,
        screen: &Screen,
    ) -> gpui::Stateful<gpui::Div> {
        let on = self.tab == tab;
        // No tooltip of what it holds: the page says it under its name, and the tooltip, shown
        // as a click left the pointer on the row, hid the page's first heading.
        crate::window::sidebar::source_row(entry, on, picture(tab), theme)
            .child(crate::window::sidebar::row_words(screen.label(t(words))))
    }

    /// At a phone's width, the list a screen of its own: each page's name, what it holds under
    /// it, and an arrow on to it.
    fn phone_row(
        &self,
        entry: gpui::Stateful<gpui::Div>,
        words: &'static str,
        summary: String,
        theme: Theme,
        screen: &Screen,
    ) -> gpui::Stateful<gpui::Div> {
        entry
            .tab_index(0)
            .focus_visible(move |s| s.border_2().border_color(theme.ring(false)))
            .w_full()
            .px(px(space::M))
            .py(px(space::M))
            .flex()
            .items_center()
            .gap(px(space::M))
            .cursor_pointer()
            .border_b_1()
            .border_color(theme.separator)
            .hover(move |s| s.bg(theme.stripe))
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
                                .text_color(theme.muted)
                                .line_clamp(2)
                                .text_ellipsis()
                                .child(screen.label(summary)),
                        )
                    }),
            )
            .child(native::icon(
                crate::window::mirror::forward(),
                native::ICON_SMALL,
                theme.muted,
            ))
    }
}

/// The page shown, at its top beside the list: its name and what it holds, as the list said
/// them under each name before (`docs/behaviour-contract.md` B43).
pub(super) fn page_heading(
    words: &'static str,
    summary: String,
    theme: Theme,
    screen: &Screen,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(space::XXS))
        // One room under the name, whatever comes first under it (`Page::section`).
        .pb(px(space::M))
        .child(
            div()
                .text_size(px(native::SUBTITLE_TEXT))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(screen.label(t(words))),
        )
        .when(!summary.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(native::SMALL_TEXT))
                    .text_color(theme.muted)
                    .child(screen.label(summary)),
            )
        })
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

/// Beside a page, the list of pages as the dialog's side panel: the main window's side panel's
/// colour, its height, a line at its edge, Find a setting (`search`) at its top.
pub(super) fn side_panel(search: Div, list: Option<Stateful<Div>>, theme: Theme) -> Div {
    div()
        .flex_none()
        .w(px(super::PAGES_WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(crate::window::theme::frame(theme.sidebar))
        .map(|panel| {
            if crate::window::mirror::rtl() {
                panel.border_l_1()
            } else {
                panel.border_r_1()
            }
        })
        .border_color(theme.separator)
        .child(
            div()
                .px(px(space::M))
                .pt(px(space::L))
                .pb(px(space::M))
                .child(search),
        )
        .children(list)
}
