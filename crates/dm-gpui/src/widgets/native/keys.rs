// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The keyboard in a dialog: a control's ring where it has the keys, and Return, the default
//! button's or the focused control's own.

use gpui::{prelude::*, Div, Stateful};

use super::RADIUS;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// Whether Return goes to a dialog's default button: nothing has the keys, or the dialog itself
/// (`root`), or a text field. On a focused button, switch or pop-up, Return is that control's
/// own press, which GPUI makes as the key comes up: run as the dialog's default too, Return on
/// a focused Cancel added the download the person cancelled (gaps row U2).
pub fn return_is_default(
    window: &gpui::Window,
    root: Option<&gpui::FocusHandle>,
    cx: &gpui::App,
) -> bool {
    let Some(focused) = window.focused(cx) else {
        return true;
    };
    if root.is_some_and(|root| *root == focused) {
        return true;
    }
    // A text field's keys are in gpui-component's "Input" context.
    window
        .context_stack()
        .iter()
        .any(|context| context.contains("Input"))
}

/// A control reached by Tab, its focus drawn as a ring in the accent's text shade and never a
/// fill: a stripe's shade read 1.1 to 1 and told a person with the keys nothing (gaps row U2).
/// At rest its edge is there but clear, so the ring moves nothing when it comes. For a control
/// with no edge of its own: a check box's line, a radio's, a switch's, a chip, a link.
pub fn ringed(control: Stateful<Div>, theme: Theme) -> Stateful<Div> {
    control
        .tab_index(0)
        .border_2()
        .border_color(gpui::transparent_black())
        .rounded(px(RADIUS))
        .focus_visible(move |s| s.border_color(theme.ring(false)))
}

/// A control with a face of its own (a chip, a segment, a preset, a sheet's quick button): the
/// accent where it is the one chosen (`on`), `face` otherwise, answering the pointer and a
/// press as a push button does, and ringed when it has the keys (gaps row E40). Each drew its
/// own, or answered nothing.
pub fn faced(control: Stateful<Div>, face: gpui::Rgba, on: bool, theme: Theme) -> Stateful<Div> {
    use crate::window::theme::{HOVERED, PRESSED};
    let face = if on { theme.accent } else { face };
    ringed(control, theme)
        .bg(face)
        .when(on, |c| c.text_color(theme.on_accent))
        .hover(move |s| s.bg(theme.answering(face, HOVERED)))
        .active(move |s| s.bg(theme.answering(face, PRESSED)))
}

/// A drawn mark from `assets/icons/` (a chevron, a tick, Close), `size` points square in
/// `color`: the font's "✓", "›" and "✕" were each drawn at another weight and height (gaps row
/// E41).
pub fn icon(path: &'static str, size: f32, color: gpui::Rgba) -> gpui::Svg {
    gpui::svg()
        .path(path)
        .size(px(size))
        .flex_none()
        .text_color(color)
}
