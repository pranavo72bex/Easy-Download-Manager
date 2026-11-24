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
        .focus(move |s| s.border_color(theme.ring(false)))
}
