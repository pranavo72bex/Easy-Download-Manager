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
        .h(px(3.0))
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
