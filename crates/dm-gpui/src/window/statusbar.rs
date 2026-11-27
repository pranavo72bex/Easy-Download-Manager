// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The status bar along the window's foot, as macOS programs keep one
//! (Finder's): what is downloading and how fast, what is finishing, what
//! waits for Start queue, and the speed limit, which a click opens the
//! speed limiter's presets from. The counts are the
//! application's (`rules::tally`); the sentences are translated ones,
//! so each language says the same.

use dm_desktop_base::format::rate;
use dm_desktop_base::i18n::{t, tf, tn};
use gpui::{div, prelude::*, Role};

use crate::window::a11y::Told;
use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// How long a sentence stands in the status bar.
pub(crate) const SAID_FOR: std::time::Duration = std::time::Duration::from_secs(12);

/// The status bar's height, in points.
pub const HEIGHT: f32 = 24.0;

/// The sentence dm said a moment ago, while the status bar still shows it.
pub fn fresh_said(view: &Downloads) -> Option<String> {
    view.status
        .said
        .as_ref()
        .filter(|(_, at)| at.elapsed() < SAID_FOR)
        .map(|(said, _)| said.clone())
}

/// What the status bar says, part by part.
pub fn parts(view: &Downloads) -> Vec<String> {
    let tally = dm_shell::app::rules::tally(&view.rows);
    // How many the place shown holds, first, as Finder's status bar counts a folder's items (the
    // maintainer's choice of 2026-11-26, "summary"); then how the downloads go.
    let held = view.list.shown.len();
    let mut parts = vec![tn("{} download||{} downloads", held as u64, &[&held])];
    let going = if tally.running == 0 && tally.finishing == 0 {
        t("Nothing downloading").to_string()
    } else if tally.running == 0 {
        tn(
            "{} finishing||{} finishing",
            tally.finishing as u64,
            &[&tally.finishing],
        )
    } else {
        tn(
            "{} downloading at {}||{} downloading at {}",
            tally.running as u64,
            &[&tally.running, &rate(tally.rate)],
        )
    };
    parts.push(format!("· {going}"));
    if tally.running > 0 && tally.finishing > 0 {
        parts.push(tn(
            "· {} finishing||· {} finishing",
            tally.finishing as u64,
            &[&tally.finishing],
        ));
    }
    // Rows added for later are counted, and said to wait for Start queue,
    // so a list of them is not taken for a stuck one.
    if tally.later > 0 {
        parts.push(tn(
            "· {} waiting for Start queue||· {} waiting for Start queue",
            tally.later as u64,
            &[&tally.later],
        ));
    }
    // A sentence said a moment ago.
    let mut said_wait = false;
    if let Some((said, at)) = &view.status.said {
        if at.elapsed() < SAID_FOR {
            parts.push(format!("· {said}"));
            said_wait = view.status.said_schedule_wait;
        }
    }
    // What holds the downloads back, so a queue that does nothing is not taken for a broken
    // one: the download limit spent, or the schedule shut (gaps row U37).
    let settings = &view.known.drawn;
    if let Some(wait) = view.backend.allowance_spent() {
        parts.push(tf(
            "· download limit of {} MB reached, held for {}",
            &[&settings.limit_mb, &dm_desktop_base::format::span(wait)],
        ));
    } else if !said_wait && dm_shell::app::schedule::holds_waiting(&settings.schedule, &view.rows) {
        parts.push(
            match dm_desktop_base::held::opens_next(&settings.schedule) {
                Some(at) => tf("· waiting for the schedule (opens at {})", &[&at]),
                None => t("· waiting for the schedule").to_string(),
            },
        );
    }
    let limit = view.known.drawn.advanced.speed_limit;
    parts.push(if limit > 0 {
        tf("· limited to {} KB/s", &[&limit])
    } else {
        format!("· {}", t("Speed limiter"))
    });
    parts
}

/// The status bar.
pub fn statusbar(
    view: &mut Downloads,
    theme: Theme,
    screen: &Screen,
    cx: &mut gpui::Context<Downloads>,
) -> impl IntoElement {
    // Undo, for the moment after downloads were taken off the list.
    let undo = view.status.undo.live();
    let mut said = parts(view);
    let whole = said.join(" ");
    // The last part, the speed limit, is a control (gaps row U29): the same
    // words as the rest, with a pop-up button's up-and-down mark after them so it is seen to be
    // one before the pointer finds it, then in the link's colour under the pointer, as Undo; a
    // click pops up Downloads › Speed limiter's items.
    let limit = said.pop().unwrap_or_default();
    // Read to a screen reader as it is shown, without the dot before it.
    let named = limit.trim_start_matches('·').trim().to_string();
    let limiter = div()
        .id("speed-limiter")
        .role(Role::Button)
        .aria_label(named)
        .group("speed-limiter")
        .cursor_pointer()
        .hover(|s| s.text_color(theme.accent_text))
        .min_w_0()
        .flex()
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(crate::widgets::space::XXS))
        .on_click(cx.listener(|view, event: &gpui::ClickEvent, window, cx| {
            let speed_limit = view.backend.settings().advanced.speed_limit;
            crate::window::menus::speed_limiter_popup(speed_limit).show(
                event.position(),
                window,
                cx,
            );
        }))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(screen.label(limit)),
        )
        .child(
            gpui::svg()
                .path("icons/chevron-up-down.svg")
                .size(px(crate::widgets::native::ICON_SMALL))
                .flex_none()
                .text_color(theme.muted)
                .group_hover("speed-limiter", |s| s.text_color(theme.accent_text)),
        );
    // What finished or failed a moment ago, for a screen reader alone.
    let heard = view
        .status
        .heard
        .as_ref()
        .filter(|(_, at)| at.elapsed() < SAID_FOR)
        .map(|(sentence, _)| sentence.clone());
    div()
        .id("status")
        .role(Role::Status)
        .aria_label(whole)
        .relative()
        // Said aloud as it changes, and drawn nowhere: a point of its own,
        // not the bar, whose speeds change every second (gaps row U23).
        .when_some(heard, |bar, sentence| {
            bar.child(
                div()
                    .id("heard")
                    .role(Role::Status)
                    .aria_label(sentence)
                    .said_aloud()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(px(1.0)),
            )
        })
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        // Its parts from the right where the window is mirrored (gaps row U35).
        .map(crate::window::mirror::row)
        .items_center()
        .gap_1()
        // From the edge the side panel's pictures stand on (16), not 4 points short of it.
        .px(px(crate::widgets::space::XL))
        .border_t_1()
        .border_color(theme.separator)
        .bg(crate::window::theme::frame(theme.sidebar))
        .text_size(px(crate::widgets::native::SMALL_TEXT))
        .text_color(theme.muted)
        .font_features(crate::widgets::native::figures())
        // Each part cut with "…" where the bar is narrower than they are, never cut off unseen
        // at its edge (gaps row V15). The count and what is running keep their words whole: the
        // parts after them (a sentence said a moment ago, what waits, the speed limiter) give up
        // their room. Shrunk alike, a phone's bar at 200% read "6 downlo… · 1 downloading at 307… ·
        // Tap a download to watch its pr… · Speed li…" (unslop review).
        .children(said.into_iter().enumerate().map(|(i, part)| {
            div()
                .when(i < 2, |part| part.flex_shrink_0())
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(screen.label(part))
        }))
        .child(limiter)
        .when(undo, |bar| {
            bar.child(
                div()
                    .id("undo")
                    .role(Role::Button)
                    .aria_label(t("Undo"))
                    .map(|undo| {
                        if crate::window::mirror::rtl() {
                            undo.mr_2()
                        } else {
                            undo.ml_2()
                        }
                    })
                    // A button, not words in the line (gaps row U14).
                    .px_2()
                    .rounded(px(crate::widgets::space::CORNER_S))
                    .border_1()
                    .border_color(theme.separator)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.stripe))
                    .active(move |s| s.bg(theme.pressed_soft()))
                    .text_color(theme.accent_text)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.undo_remove(cx);
                        cx.notify();
                    }))
                    .child(screen.label(t("Undo"))),
            )
        })
}
