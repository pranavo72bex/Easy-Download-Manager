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
            let list_top = (width >= 600.0 && h.shows(dm_desktop_base::i18n::t("File name")))
                .then(|| f32::from(h.bounds_of(dm_desktop_base::i18n::t("File name")).top()));
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

/// No word of a dialog is laid past its window's edge in German, whose words run longest, nor
/// in Hebrew, read right to left (gaps row V15).
#[test]
fn every_dialog_keeps_its_words_inside_in_german_and_hebrew() {
    use dm_desktop_base::i18n::t;
    // The language is the process's.
    let _file = settings_file();
    let mut found = Vec::new();
    for language in ["de", "he"] {
        for (command, shown) in DIALOGS {
            let world = world();
            let mut settings = world.settings();
            settings.general.language = language.into();
            let mut h = world.harness_with(settings);
            dm_desktop_base::i18n::set(language);
            h.command(command);
            h.until_shown(t(shown));
            for past in h.words_past_the_edges() {
                found.push(format!("{command} in {language}: {past}"));
            }
        }
    }
    dm_desktop_base::i18n::set("en");
    assert!(found.is_empty(), "{found:#?}");
}

/// No word of the main window is cut unseen in German or Hebrew, at any of the widths.
#[test]
fn the_main_window_keeps_its_words_in_german_and_hebrew() {
    use dm_desktop_base::i18n::t;
    // The language is the process's.
    let _file = settings_file();
    let mut found = Vec::new();
    for language in ["de", "he"] {
        let world = world();
        let mut settings = world.settings();
        settings.general.language = language.into();
        let mut h = world.harness_with(settings);
        dm_desktop_base::i18n::set(language);
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
        for width in WIDTHS {
            h.resize(width, 720.0);
            let heading = t("File name");
            let list_top =
                (width >= 600.0 && h.shows(heading)).then(|| f32::from(h.bounds_of(heading).top()));
            for (text, laid, seen) in h.words_cut() {
                // A column the list's sideways scroll cuts, at either edge where it is mirrored.
                let scrolled = list_top.is_some_and(|top| f32::from(laid.top()) >= top - 1.0)
                    && (f32::from(seen.right()) >= width - 1.0 || f32::from(seen.left()) <= 1.0);
                if !scrolled {
                    found.push(format!(
                        "{width} in {language}: {text:?} laid {laid:?} seen {seen:?}"
                    ));
                }
            }
        }
    }
    dm_desktop_base::i18n::set("en");
    assert!(found.is_empty(), "{found:#?}");
}

/// Where the window is mirrored, a person's sideways move of the list is kept as frames go by
/// (gaps rows U35, V15); a move a resize left is not taken for one (the Hebrew sweep above).
#[test]
fn a_mirrored_lists_sideways_move_is_kept() {
    use dm_desktop_base::i18n::t;
    // The language is the process's.
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.general.language = "he".into();
    let mut h = world.harness_with(settings);
    dm_desktop_base::i18n::set("he");
    add_later(&mut h, &world, &["file.zip"]);
    h.until("the row drawn", |h| h.shows("file.zip"));
    h.resize(840.0, 720.0);
    let heading = t("Size");
    let at_first = h.bounds_of(heading).left();
    h.wheel_sideways(heading, -60.0);
    let moved = h.bounds_of(heading).left();
    assert_ne!(moved, at_first, "the wheel did not move the list");
    // Frames go by: the move is kept.
    for _ in 0..3 {
        h.update(|_, cx| cx.notify());
        let _ = h.words_cut();
    }
    assert_eq!(h.bounds_of(heading).left(), moved, "the move was not kept");
    dm_desktop_base::i18n::set("en");
}

/// The dialogs of a download, Properties and its progress window, keep their words inside at
/// 100% and 200% text and in German and Hebrew (gaps row V15).
#[test]
fn a_downloads_dialogs_keep_their_words_inside() {
    use dm_desktop_base::i18n::t;
    // The language and the text size are the process's.
    let _file = settings_file();
    let mut found = Vec::new();
    for (language, size) in [("en", 1.0), ("en", 2.0), ("de", 1.0), ("he", 1.0)] {
        for (command, shown) in [
            ("file.properties", "Address"),
            ("file.show_progress", "Downloaded"),
        ] {
            let world = world();
            let mut settings = world.settings();
            settings.general.language = language.into();
            settings.text_size = size;
            let mut h = world.harness_with(settings);
            dm_desktop_base::i18n::set(language);
            h.act(|_, cx| crate::window::zoom::start(size, cx));
            h.view(|view| view.backend.add(&world.url("stalled.bin")).unwrap());
            h.until("the row", |h| h.rows().len() == 1);
            let id = h.rows()[0].id;
            h.call("ui.select", serde_json::json!({ "ids": [id] }))
                .unwrap();
            h.command(command);
            h.until_shown(t(shown));
            for past in h.words_past_the_edges() {
                found.push(format!("{command} in {language} at {size}: {past}"));
            }
            h.act(|_, cx| crate::window::zoom::start(1.0, cx));
        }
    }
    dm_desktop_base::i18n::set("en");
    assert!(found.is_empty(), "{found:#?}");
}

/// In Hebrew a dialog reads from the right: its labels at the right of their fields and its
/// buttons in the other order (gaps row E36).
#[test]
fn a_dialog_is_mirrored_right_to_left() {
    use dm_desktop_base::i18n::t;
    // The language is the process's.
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.general.language = "he".into();
    let mut h = world.harness_with(settings);
    dm_desktop_base::i18n::set("he");
    h.command("downloads.speed_limiter.settings");
    h.until_shown(t("Speed limit (KB/s, 0 for none)"));
    let label = h
        .dialog_words(t("Speed limit in those hours (KB/s)"))
        .unwrap();
    let field = h.dialog_words("256").unwrap();
    let ok = h.dialog_words(t("OK")).unwrap();
    let cancel = h.dialog_words(t("Cancel")).unwrap();
    dm_desktop_base::i18n::set("en");
    // The label at the right, its field against it at its left.
    assert!(
        field.right() <= label.left() && label.left() - field.right() < gpui::px(24.0),
        "the field is not at its label's left: {field:?}, {label:?}"
    );
    // macOS and GNOME put the default rightmost, Windows first: mirrored, the other way.
    assert_eq!(
        ok.left() < cancel.left(),
        !cfg!(windows),
        "the buttons were not mirrored: OK {ok:?}, Cancel {cancel:?}"
    );
}
