// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm knows about the chosen download, in a pane under the list of a wide window (View ›
//! Details, off at first; gaps row E8): its figure line and bar, where it is, where it came
//! from, and why it failed with what to do. A phone opens a card's detail itself.

use dm_core::{Entry, State};
use dm_desktop_base::format;
use dm_desktop_base::i18n::t;
use gpui::{div, prelude::*, Context, Div, SharedString};

use super::Downloads;
use crate::widgets::{native, space};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The pane's height: its figure, its bar and four lines of facts.
const HEIGHT: f32 = 150.0;

impl Downloads {
    /// The pane for the one download chosen; none where none or several are, or it is off.
    pub(crate) fn detail_pane(
        &self,
        theme: Theme,
        screen: &Screen,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<Div>> {
        if !self.layout.show_details {
            return None;
        }
        let chosen = self.chosen();
        let [row] = chosen.as_slice() else {
            return None;
        };
        Some(pane(row, theme, screen))
    }
}

/// How far, how fast, how long: the progress window's own figure (gaps rows E8, E9), the
/// status while its parts are joined.
fn figure(row: &Entry) -> String {
    let joining = row.state == State::Running && dm_shell::app::rules::fetched_all(row);
    crate::dialogs::progress::figure_words(row, joining)
}

/// A fact: its name in the hint's colour over its value, cut with "…" where it is long.
fn fact(name: &'static str, value: String, theme: Theme, screen: &Screen) -> Div {
    div()
        .flex()
        .gap(px(space::M))
        .min_w_0()
        .child(
            div()
                .flex_none()
                .w(px(90.0))
                .text_color(theme.muted)
                .child(screen.label(t(name))),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(screen.label(SharedString::from(value))),
        )
}

fn pane(row: &Entry, theme: Theme, screen: &Screen) -> gpui::Stateful<Div> {
    let fraction = row.progress.fraction().unwrap_or(0.0) as f32;
    let why = match &row.state {
        State::Failed(why) => Some((
            dm_desktop_base::rows::status_of(row),
            format::next_step(why).map(t),
        )),
        _ => None,
    };
    let folder = if row.path.is_empty() {
        dm_desktop_base::rows::folder_of(row)
    } else {
        row.path.clone()
    };
    div()
        .id("detail-pane")
        .flex_none()
        .h(px(HEIGHT))
        .px(px(space::L))
        .py(px(space::M))
        .border_t_1()
        .border_color(theme.separator)
        .bg(theme.window)
        .flex()
        .flex_col()
        .gap(px(space::S))
        .text_size(px(native::SMALL_TEXT))
        .child(
            div()
                .text_size(px(native::TEXT * 1.3))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .font_features(crate::widgets::native::figures())
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(screen.label(SharedString::from(figure(row)))),
        )
        .child(
            div().h(px(4.0)).rounded_full().bg(theme.separator).child(
                div()
                    .h_full()
                    .rounded_full()
                    .bg(theme.accent_text)
                    .w(gpui::relative(fraction.clamp(0.0, 1.0))),
            ),
        )
        .child(fact("Saved to", folder, theme, screen))
        .child(fact("Address", format::shown_url(&row.url), theme, screen))
        .when_some(why, |d, (said, step)| {
            d.child(
                div()
                    .text_color(theme.warning)
                    .child(screen.label(SharedString::from(said))),
            )
            .when_some(step, |d, step| d.child(screen.label(step)))
        })
}
