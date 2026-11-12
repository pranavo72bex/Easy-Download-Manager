// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The status bar along the window's foot, as macOS programs keep one
//! (Finder's): what is downloading and how fast, what is finishing, what
//! waits for Start queue, and the speed limit, which a click opens the
//! speed limiter's presets from. The counts are the
//! application's (`rules::tally`); the sentences are the egui window's own,
//! so each language says the same.

use dm_desktop_base::format::rate;
use dm_desktop_base::i18n::{t, tf, tn};
use gpui::{div, prelude::*, Role};

use crate::list::Downloads;
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// How long a sentence stands in the status bar.
pub(crate) const SAID_FOR: std::time::Duration = std::time::Duration::from_secs(12);

/// The status bar's height, in points.
pub const HEIGHT: f32 = 24.0;

/// What the status bar says, part by part.
pub fn parts(view: &Downloads) -> Vec<String> {
    let tally = dm_shell::app::rules::tally(&view.rows);
    let mut parts = vec![if tally.running == 0 && tally.finishing == 0 {
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
    }];
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
    // A sentence said a moment ago, as the egui window says its own.
    if let Some((said, at)) = &view.said {
        if at.elapsed() < SAID_FOR {
            parts.push(format!("· {said}"));
        }
    }
    let limit = view.drawn.speed_limit;
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
    let undo = view.undo.live();
    let mut said = parts(view);
    let whole = said.join(" ");
    // The last part, the speed limit, is a control (gaps row U29): the same
    // words as the rest until the pointer is on it, then in the link's
    // colour, as Undo; a click pops up Downloads › Speed limiter's items.
    let limit = said.pop().unwrap_or_default();
    // Read to a screen reader as it is shown, without the dot before it.
    let named = limit.trim_start_matches('·').trim().to_string();
    let limiter = div()
        .id("speed-limiter")
        .role(Role::Button)
        .aria_label(named)
        .cursor_pointer()
        .hover(|s| s.text_color(theme.selection))
        .on_click(cx.listener(|view, event: &gpui::ClickEvent, window, cx| {
            let speed_limit = view.backend.settings().speed_limit;
            crate::menus::speed_limiter_popup(speed_limit).show(event.position(), window, cx);
        }))
        .child(screen.label(limit));
    div()
        .id("status")
        .role(Role::Status)
        .aria_label(whole)
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_3()
        .border_t_1()
        .border_color(theme.separator)
        .bg(crate::theme::frame(theme.sidebar))
        .text_size(px(crate::native::SMALL_TEXT))
        .text_color(theme.muted)
        .children(said.into_iter().map(|part| screen.label(part)))
        .child(limiter)
        .when(undo, |bar| {
            bar.child(
                div()
                    .id("undo")
                    .role(Role::Button)
                    .aria_label(t("Undo"))
                    .ml_2()
                    .text_color(theme.selection)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.undo_remove();
                        cx.notify();
                    }))
                    .child(screen.label(t("Undo"))),
            )
        })
}
