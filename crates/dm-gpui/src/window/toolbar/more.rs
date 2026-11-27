// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The toolbar's "More" (gaps row E3): where the buttons do not fit across the window, those
//! at the end go under it, in their order, rather than on to a second row.

use dm_desktop_base::i18n::t;
use dm_shell::app::commands::Command;
use gpui::{div, prelude::*, svg, Role};

use crate::window::list::Downloads;
use crate::window::menus::Run;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// Between two buttons, and the toolbar's own room at each end, in points.
const GAP: f32 = 4.0;
const ENDS: f32 = 16.0;
/// A button's room each side of its picture or word.
const SIDES: f32 = 16.0;
/// The arrow beside Start Queue and Stop Queue, as drawn (it was counted 14, drawn 16).
const ARROW: f32 = super::QUEUE_ARROW;
/// What rounding and the toolbar's border take, so a button that just fits is not cut.
const SLACK: f32 = 8.0;

/// How wide `command`'s button is drawn, in points at 100%: its picture or its widest word,
/// whichever is wider, its room, and the queue arrow beside it.
pub(super) fn button_width(command: Command, words: &str, picture: bool, worded: bool) -> f32 {
    let widest = if !worded {
        0.0
    } else if command == Command::DownloadNow {
        ["Resume", "Retry", "Start now"]
            .into_iter()
            .map(|word| crate::widgets::native::small_width(t(word)))
            .fold(0.0, f32::max)
    } else {
        crate::widgets::native::small_width(words)
    };
    let face = if picture { widest.max(18.0) } else { widest };
    let arrow = if matches!(command, Command::StartQueue | Command::StopQueue) {
        ARROW
    } else {
        0.0
    };
    face + SIDES + arrow
}

/// How wide More is drawn, in points at 100%: its picture or its word, whichever is wider.
pub(super) fn more_width(picture: bool, worded: bool) -> f32 {
    let word = if worded {
        crate::widgets::native::small_width(t("More"))
    } else {
        0.0
    };
    let face = if picture || !worded { 18.0 } else { 0.0 };
    word.max(face) + SIDES
}

/// How many of the buttons `widths` wide are drawn across `room` points: all where they fit,
/// otherwise as many as fit beside More (`more` wide), never fewer than one.
pub(super) fn fitting(widths: &[f32], room: f32, more: f32) -> usize {
    let across = |n: usize| -> f32 {
        widths[..n].iter().sum::<f32>() + GAP * n.saturating_sub(1) as f32 + ENDS + SLACK
    };
    if across(widths.len()) <= room {
        return widths.len();
    }
    (1..widths.len())
        .rev()
        .find(|&n| across(n) + GAP + more <= room)
        .unwrap_or(1)
}

/// The More button, drawn as the others are (its picture, its word under it, as the toolbar's
/// style has them): a menu of the `rest`, each greyed where it cannot act now.
pub(super) fn more_button(
    rest: Vec<(Command, &'static str)>,
    view: &Downloads,
    theme: Theme,
    (picture, worded): (bool, bool),
    screen: &crate::window::screen::Screen,
) -> impl IntoElement {
    let items: Vec<(Command, &'static str, bool)> = rest
        .into_iter()
        .map(|(command, words)| (command, t(words), view.enabled(command)))
        .collect();
    let said = t("More");
    div()
        .id("toolbar-more")
        .role(Role::Button)
        .aria_label(said)
        .flex()
        .flex_col()
        .items_center()
        .gap(px(crate::widgets::space::LINE))
        .px_2()
        .py_1()
        .rounded(px(crate::widgets::native::RADIUS))
        .text_size(px(crate::widgets::native::SMALL_TEXT))
        .hover(move |s| s.bg(theme.stripe))
        .active(move |s| s.bg(theme.pressed_soft()))
        // The dots where pictures are shown, and where only words are, its word alone.
        .when(picture || !worded, |b| {
            b.child(
                svg()
                    .path("icons/more.svg")
                    .size(px(crate::widgets::native::ICON_TOOL))
                    .text_color(theme.text),
            )
        })
        .when(worded, |b| b.child(screen.label(said)))
        .when(!worded, |b| {
            b.tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(gpui::SharedString::from(said))
                    .build(window, cx)
            })
        })
        .on_click(move |event, window, cx| {
            let menu = items.iter().fold(
                gpui_component::native_menu::NativeMenu::new(),
                |menu, &(command, words, enabled)| {
                    menu.menu_with_disabled(words, !enabled, Box::new(Run(command)))
                },
            );
            menu.show(event.position(), window, cx);
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All the buttons where they fit; otherwise as many as fit beside More, and at least
    /// one (gaps row E3).
    #[test]
    fn the_buttons_that_do_not_fit_go_under_more() {
        let (widths, more) = ([60.0; 10], 34.0);
        // 10 × 60 + 9 gaps + the ends: 660 points.
        assert_eq!(fitting(&widths, 700.0, more), 10);
        // Narrower: seven and More (7 × 60 + 7 × 4 + 34 + 24 = 506).
        assert_eq!(fitting(&widths, 510.0, more), 7);
        assert_eq!(fitting(&widths, 500.0, more), 6);
        // A wider More leaves room for fewer.
        assert_eq!(fitting(&widths, 510.0, 60.0), 6);
        // Narrower than any: the first alone, and More.
        assert_eq!(fitting(&widths, 40.0, more), 1);
    }
}
