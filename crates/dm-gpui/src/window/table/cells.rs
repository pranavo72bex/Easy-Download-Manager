// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Pieces of a row's cells (`row_cells`): the name's words, and the bar of how far it is.

use gpui::relative;

use super::*;
use crate::window::a11y::Told;

/// A row's name as drawn: its stem, cut short where it must be, then its extension whole; read
/// as the whole name, as it was one label.
pub(super) fn name_drawn(name: &SharedString, screen: &Screen) -> impl IntoElement {
    let (stem, extension) = name_parts(name);
    let drawn = div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(stem.to_string())),
        )
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .child(SharedString::from(extension.to_string())),
        );
    // Read as the whole name, as it was one label; in a box that can be narrower than the name,
    // so the name ends in "…" where its cell is narrow, recorded or not (gaps row E7).
    div()
        .min_w_0()
        .overflow_hidden()
        .child(screen.mark(name.clone(), drawn))
}

/// The bar under a running row's status, `f` of the way along.
pub(crate) fn progress_track(f: f64, told: bool, theme: Theme) -> AnyElement {
    let track = div()
        .mt(px(space::XXS))
        .w_full()
        // Thick enough to read at a glance across the cell (gaps row E4).
        .h(px(4.0))
        .rounded_full()
        .bg(theme.separator)
        // Filled from the right where the window is mirrored (gaps row U35).
        .when(crate::window::mirror::rtl(), |track| {
            track.flex().flex_row_reverse()
        })
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(theme.selection)
                .w(relative(f.clamp(0.0, 1.0) as f32)),
        );
    // How far it is, told to a screen reader as a bar's value.
    if told {
        track
            .id("bar")
            .progress_bar(t("Progress"), f)
            .into_any_element()
    } else {
        track.into_any_element()
    }
}

/// A download's state as a mark told by its shape (gaps row E4): a play mark while it runs,
/// pause bars when paused, a clock while it waits, a tick once done, a ring with "!" when it
/// failed; none for one cancelled. In the colour the row's words take.
pub(crate) fn state_mark(state: &dm_core::State, theme: Theme) -> Option<AnyElement> {
    use dm_core::State;
    let colour = if theme.is_chosen() {
        theme.text
    } else {
        match state {
            State::Failed(_) => theme.warning,
            State::Completed => theme.success,
            State::Running => theme.accent,
            _ => theme.muted,
        }
    };
    let picture = |path: &'static str| {
        gpui::svg()
            .path(path)
            .size(px(12.0))
            .flex_none()
            .text_color(colour)
            .into_any_element()
    };
    match state {
        State::Running => Some(picture("icons/resume.svg")),
        State::Paused => Some(picture("icons/stop.svg")),
        State::Queued => Some(picture("icons/place-unfinished.svg")),
        State::Completed => Some(picture("icons/place-finished.svg")),
        State::Failed(_) => Some(
            div()
                .flex_none()
                .size(px(12.0))
                .rounded_full()
                .border_1()
                .border_color(colour)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(colour)
                .child("!")
                .into_any_element(),
        ),
        State::Cancelled => None,
    }
}
