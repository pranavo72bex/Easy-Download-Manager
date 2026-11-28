// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download's bar as the progress window and the details pane draw it: one drawing for both
//! (gaps row E8), each drawing its own, one mirrored and told to a screen reader, one not.

use edm_desktop_base::i18n::t;
use gpui::{div, prelude::*, Stateful};

use crate::window::a11y::Told as _;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// A bar `height` points tall, `fraction` of it filled in the accent's colour on the stripe's
/// track; filled from the right where the window is mirrored (gaps row U35), and told to a
/// screen reader as a bar's value.
pub fn bar(
    id: impl Into<gpui::ElementId>,
    fraction: f32,
    height: f32,
    theme: Theme,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .progress_bar(t("Progress"), f64::from(fraction))
        .h(px(height))
        .flex_1()
        .flex()
        .map(crate::window::mirror::row)
        .rounded_full()
        .bg(theme.stripe)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(theme.accent_text)
                .w(gpui::relative(fraction.clamp(0.0, 1.0))),
        )
}
