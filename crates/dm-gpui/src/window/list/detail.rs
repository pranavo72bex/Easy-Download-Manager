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
        // Where it is, or for one not saved yet where it will go, as the Folder column and File
        // properties say (`names::planned_folder`): the pane said "Saved to" over nothing.
        let settings = &self.known.drawn;
        let place = if !row.path.is_empty() {
            Some(row.path.clone())
        } else {
            let folder = dm_desktop_base::rows::folder_of(row);
            Some(folder).filter(|f| !f.is_empty()).or_else(|| {
                dm_shell::app::names::planned_folder(
                    row,
                    &settings.folders.download_dir,
                    &mut settings.categories(),
                )
            })
        };
        Some(pane(row, place.unwrap_or_default(), theme, screen))
    }
}

/// How far, how fast, how long: the progress window's own figure (gaps rows E8, E9), the
/// status while its parts are joined.
fn figure(row: &Entry) -> String {
    let joining = row.state == State::Running && dm_shell::app::rules::fetched_all(row);
    crate::dialogs::progress::figure_words(row, joining)
}

/// The facts' names, for the column they stand in.
const NAMES: [&str; 3] = ["Will be saved to", "Saved to", "Address"];

/// A fact, as every window reports one (`native::fact`), cut with "…" where it is long.
fn fact(name: &'static str, value: String, theme: Theme, screen: &Screen) -> Div {
    // As wide as the longest name in the language shown, where 90 points cut "Will be saved to".
    let names = NAMES.map(t);
    let column = names
        .iter()
        .map(|n| native::small_width(n))
        .fold(0.0, f32::max)
        .ceil();
    native::fact(
        t(name),
        column,
        theme,
        screen,
        div()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(screen.label(SharedString::from(value))),
    )
}

fn pane(row: &Entry, place: String, theme: Theme, screen: &Screen) -> gpui::Stateful<Div> {
    let fraction = row.progress.fraction().unwrap_or(0.0) as f32;
    let why = match &row.state {
        State::Failed(why) => Some((
            dm_desktop_base::rows::status_of(row),
            format::next_step(why).map(t),
        )),
        _ => None,
    };
    let saved = if row.path.is_empty() {
        "Will be saved to"
    } else {
        "Saved to"
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
                .text_size(px(native::FIGURE_TEXT))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .font_features(crate::widgets::native::figures())
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(screen.label(SharedString::from(figure(row)))),
        )
        // The progress window's bar, drawn by the same hand (gaps row E8): mirrored and told.
        .child(
            div()
                .flex()
                // As thick as the progress window's, whose figure and bar this pane repeats.
                .child(native::bar("detail-bar", fraction, 6.0, theme)),
        )
        .child(fact(saved, place, theme, screen))
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
