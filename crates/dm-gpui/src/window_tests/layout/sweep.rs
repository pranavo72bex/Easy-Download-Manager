// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The layout swept without a person looking (gaps row V15): the main window at 360, 600, 840
//! and 1280 points wide, and each dialog, at the normal text size and at 200%; any word cut
//! off with no "…" to say so, or laid past its window's edge, fails.

use super::*;

/// The widths the main window is swept at: a phone, a small tablet or a narrow window, a
/// laptop's half screen, a full one.
const WIDTHS: [f32; 4] = [360.0, 600.0, 840.0, 1280.0];

/// The dialogs swept, by the command that opens each and words it shows once open.
const DIALOGS: [(&str, &str); 3] = [
    ("downloads.options", "Language"),
    ("downloads.scheduler", "Stop at"),
    (
        "downloads.speed_limiter.settings",
        "Slower between set hours",
    ),
];

/// No word of the main window is cut unseen, at any of the widths, at 100% and 200% text.
#[test]
fn the_main_window_keeps_its_words_at_every_width_and_size() {
    // The text size is the process's.
    let _file = settings_file();
    let mut found = Vec::new();
    for size in [1.0, 2.0] {
        let world = world();
        let mut settings = world.settings();
        settings.text_size = size;
        let mut h = world.harness_with(settings);
        h.act(|_, cx| crate::window::zoom::start(size, cx));
        h.call(
            "downloads.add",
            serde_json::json!({ "urls": [world.url("file.zip")] }),
        )
        .unwrap();
        h.until("the download to complete", |h| {
            h.rows()
                .iter()
                .any(|r| r.state == dm_core::State::Completed)
        });
        // Not under the least width at this size: at 200% a window 360 points wide holds what
        // 180 would, narrower than any phone held upright (large text on a phone: gaps row V14).
        for width in WIDTHS
            .into_iter()
            .filter(|w| *w >= crate::window::narrow::LEAST_WIDTH * size)
        {
            h.resize(width, 720.0);
            // The list scrolls sideways where its columns are wider than the window: a column
            // its right edge cuts is scrolled to, not lost.
            let list_top = (width >= 600.0 && h.shows(dm_desktop_base::i18n::t("File Name")))
                .then(|| f32::from(h.bounds_of(dm_desktop_base::i18n::t("File Name")).top()));
            for (text, laid, seen) in h.words_cut() {
                let scrolled = list_top.is_some_and(|top| f32::from(laid.top()) >= top - 1.0)
                    && f32::from(seen.right()) >= width - 1.0;
                if !scrolled {
                    found.push(format!(
                        "{width} at {size}: {text:?} laid {laid:?} seen {seen:?}"
                    ));
                }
            }
        }
        h.act(|_, cx| crate::window::zoom::start(1.0, cx));
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// No word of a dialog is laid past its window's edge, at 100% and 200% text.
#[test]
fn every_dialog_keeps_its_words_inside_at_each_size() {
    // The text size is the process's.
    let _file = settings_file();
    let mut found = Vec::new();
    for size in [1.0, 2.0] {
        for (command, shown) in DIALOGS {
            let world = world();
            let mut settings = world.settings();
            settings.text_size = size;
            let mut h = world.harness_with(settings);
            h.act(|_, cx| crate::window::zoom::start(size, cx));
            h.command(command);
            h.until_shown(shown);
            for past in h.words_past_the_edges() {
                found.push(format!("{command} at {size}: {past}"));
            }
            h.act(|_, cx| crate::window::zoom::start(1.0, cx));
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}
