// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A switch for an on/off setting, as the systems draw one now (gaps row C20).

use gpui::{div, prelude::*, Div, Role, SharedString, Stateful};

use super::ringed;
use super::{on_a_phone, CONTROL};
use crate::widgets::space;
use crate::window::a11y::Told as _;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// An on/off setting as the systems draw one now: its words across the row and a switch at the
/// row's end, its knob at the right and its track in the accent colour when on (gaps row C20,
/// the maintainer, 2026-11-23: "toggle button is better than checkbox"). Reached by Tab and
/// flipped by Space or a click, as a check box is; a screen reader hears a switch, on or off.
pub fn switch(
    id: impl Into<gpui::ElementId>,
    words: SharedString,
    on: bool,
    theme: Theme,
    screen: &Screen,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(words.clone())
        .off_where_greyed()
        .map(|c| ringed(c, theme))
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        // Out by its ring's border on each side, so its words and its track stand where the
        // other rows' do (gaps row E50: they were 2 points in from the pop-ups' rows).
        .mx(gpui::px(-2.0))
        .min_h(px(on_a_phone(space::TOUCH, CONTROL)))
        .flex()
        .items_center()
        .gap(px(space::M))
        .cursor_pointer()
        .child(div().flex_1().min_w_0().child(screen.label(words)))
        .child(switch_track(on, theme))
}

/// [`switch`] where a dialog's setting sits in a line of its own or a sentence ("Limit this
/// download to [500] KB/s"): the switch first and its words after it, in a check box's place;
/// on a phone as [`switch`] is drawn.
pub fn switch_inline(
    id: impl Into<gpui::ElementId>,
    words: SharedString,
    on: bool,
    theme: Theme,
    screen: &Screen,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(words.clone())
        .off_where_greyed()
        .map(|c| ringed(c, theme))
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .flex()
        // The switch at the right where the language reads right to left (gaps row E36).
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(space::S))
        .cursor_pointer()
        .map(|row| {
            // A phone's: its words across the line and the switch at the end, as every setting
            // there (Options' switches; gaps row N31). A computer's: the switch first.
            if crate::platform::ON_A_PHONE {
                row.w_full()
                    .min_h(px(space::TOUCH))
                    .gap(px(space::M))
                    .child(div().flex_1().min_w_0().child(screen.label(words)))
                    .child(switch_track(on, theme))
            } else {
                // Its words wrap within the window: as an item of their own width the
                // Scheduler's German "wake" ran past its edge in Windows' wider type.
                row.child(switch_track(on, theme))
                    .child(div().flex_1().min_w_0().child(screen.label(words)))
            }
        })
}

/// The track and knob of a switch.
pub fn switch_track(on: bool, theme: Theme) -> Div {
    let (width, height) = (on_a_phone(44.0, 32.0), on_a_phone(26.0, 18.0));
    let knob = height - 2.0 * space::XXS;
    div()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .rounded(px(height / 2.0))
        .p(px(space::XXS))
        .flex()
        .items_center()
        .when(on, |t| t.justify_end().bg(theme.accent))
        // Off: the track's edge in the tick box's colour, at 3 to 1 as a control's outline is
        // read; the separator's grey alone read 1.2 to 1 on a dialog.
        .when(!on, |t| {
            t.justify_start()
                .bg(theme.separator)
                .border_1()
                .border_color(theme.muted)
        })
        .child(
            div()
                .size(px(knob))
                .rounded(px(knob / 2.0))
                .bg(gpui::white())
                .shadow_sm(),
        )
}
