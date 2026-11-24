// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window in another language: its words, its text size, and Arabic read right to left.

use super::*;

/// The series' error is in the language in force, says nothing until From and To are typed,
/// and keeps its room so the buttons stay put.
#[test]
fn the_series_error_waits_for_both_ends_and_speaks_german() {
    use dm_desktop_base::i18n::t;
    // The language is the process's.
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.general.language = "de".into();
    let mut h = world.harness_with(settings);
    dm_desktop_base::i18n::set("de");
    h.command("program.add_series");
    h.until("the series", |h| !h.dialogs().is_empty());
    let start = |h: &mut Harness| h.dialog_words(t("Start now")).expect("Start now");
    let before = start(&mut h);
    h.fill(t("Address with *"), &world.url("part.bin"));
    let star = t("The address needs exactly one * to fill in.");
    assert_ne!(star, "The address needs exactly one * to fill in.");
    assert!(!h.shows(star), "said before From/To");
    h.fill(t("From"), "5");
    assert!(!h.shows(star), "said before To");
    h.fill(t("To"), "1");
    h.until("the error, in German", |h| h.shows(star));
    assert_eq!(start(&mut h), before, "the dialog jumped");
    dm_desktop_base::i18n::set("en");
}

/// View › Text size makes everything larger, smaller, or normal again.
#[test]
fn view_text_size_zooms_the_window() {
    // The text size is the process's.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    for item in ["View", "Text size", "Larger"] {
        h.click(item);
    }
    h.until("120%", |_| {
        (crate::window::zoom::factor() - 1.2).abs() < 0.01
    });
    for item in ["View", "Text size", "Normal size"] {
        h.click(item);
    }
    h.until("100%", |_| {
        (crate::window::zoom::factor() - 1.0).abs() < 0.01
    });
}

#[test]
fn view_language_puts_the_window_in_another_language_and_back() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    assert!(h.shows("Transfer rate"));
    for item in ["View", "Language", "Deutsch"] {
        h.click(item);
    }
    h.until("German", |h| {
        h.view(|view| view.backend.settings().general.language) == "de"
    });
    h.until("the list's headings in German", |h| {
        h.shows("Übertragungsrate") && !h.shows("Transfer rate")
    });
    for item in ["Ansicht", "Sprache", "English"] {
        h.click(item);
    }
    h.until("not back in English", |h| h.shows("Transfer rate"));
    assert_eq!(
        h.view(|view| view.backend.settings().general.language),
        "en"
    );
}

/// Where the main window draws the first words that begin with `words` (a heading carries its
/// sort arrow after its name).
fn in_main_starting(h: &mut Harness, words: &str) -> Option<Bounds<Pixels>> {
    let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
    texts["texts"].as_array().unwrap().iter().find_map(|t| {
        let text = t["text"].as_str().unwrap_or_default();
        (text.starts_with(words) && t["window"] == "main").then(|| {
            let r: Vec<f32> = t["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            Bounds::from_corners(point(px(r[0]), px(r[1])), point(px(r[2]), px(r[3])))
        })
    })
}

/// Arabic, read right to left, mirrors the window (gaps row U35): the side panel on the right,
/// a row's name against the right of its column (the columns from the right, Size left of the
/// name), and the toolbar's buttons from the right. English keeps each where it always was.
#[test]
fn a_language_read_right_to_left_mirrors_the_window_and_english_does_not() {
    use dm_desktop_base::i18n::{self, t};
    use dm_shell::app::commands::Command;
    // The language is the process's.
    let _file = settings_file();
    let world = world();
    for code in ["en", "ar"] {
        let rtl = code == "ar";
        assert_eq!(i18n::Direction::of(code).is_rtl(), rtl);
        let mut settings = world.settings();
        settings.general.language = code.into();
        i18n::set(code);
        let mut h = world.harness_with(settings);
        i18n::set(code);
        add_later(&mut h, &world, &["file.zip"]);
        h.until("the row drawn", |h| in_main(h, "file.zip").is_some());
        let main = h.window;
        let middle = inside(&mut h, main).width / 2.0;

        // The side panel: on the left in English, on the right in Arabic.
        let panel = in_main(&mut h, t("Unfinished")).expect("the side panel's Unfinished");
        assert_eq!(
            panel.center().x > middle,
            rtl,
            "{code}: the side panel at {panel:?}, the window's middle at {middle:?}"
        );

        // A row's name against its column's leading edge, by its picture, as the heading is:
        // the left in English, the right in Arabic; the Size column on its far side.
        let name = in_main(&mut h, "file.zip").expect("the row's name");
        let heading = in_main_starting(&mut h, Column::Name.title()).expect("the name's heading");
        let size = in_main_starting(&mut h, Column::Size.title()).expect("the size's heading");
        if rtl {
            assert!(
                name.right() < heading.right() && heading.right() - name.right() < px(40.0),
                "{code}: the name {name:?} is not against the right of {heading:?}"
            );
            assert!(
                size.right() < name.left(),
                "{code}: Size {size:?} not left of {name:?}"
            );
        } else {
            assert!(
                name.left() > heading.left() && name.left() - heading.left() < px(40.0),
                "{code}: the name {name:?} is not against the left of {heading:?}"
            );
            assert!(
                size.left() > name.right(),
                "{code}: Size {size:?} not right of {name:?}"
            );
        }

        // The toolbar: its first two buttons (Resume's word changes with the rows chosen, so
        // not that one) from the left in English, from the right in Arabic.
        let shown: Vec<&'static str> = h.view(|view| {
            dm_shell::app::commands::toolbar_shown(&view.backend.settings())
                .into_iter()
                .filter(|(command, _)| *command != Command::DownloadNow)
                .map(|(_, words)| t(words))
                .take(2)
                .collect()
        });
        assert_eq!(shown.len(), 2, "{code}: two toolbar buttons");
        let first = in_main(&mut h, shown[0]).expect("the first button");
        let second = in_main(&mut h, shown[1]).expect("the second button");
        assert_eq!(
            first.center().x > second.center().x,
            rtl,
            "{code}: {:?} at {first:?}, {:?} at {second:?}",
            shown[0],
            shown[1]
        );
    }
    i18n::set("en");
}

/// No word of the progress window or the Scheduler starts outside its window's left edge or
/// runs past its right one, in English and in German, at the size each opens at (gaps row E2:
/// "When this download is done" began outside the left edge, the Scheduler's "Stop at" minutes
/// ran past the right one).
#[test]
fn the_progress_window_and_the_scheduler_keep_their_words_inside_in_english_and_german() {
    use dm_desktop_base::i18n::t;
    // The language and the text size are the process's.
    let _file = settings_file();
    for language in ["en", "de"] {
        let world = world();
        let mut settings = world.settings();
        settings.general.language = language.into();
        let mut h = world.harness_with(settings);
        dm_desktop_base::i18n::set(language);
        h.call(
            "downloads.add",
            serde_json::json!({ "urls": [world.url("file.zip")], "later": true }),
        )
        .unwrap();
        h.until("the row", |h| h.rows().len() == 1);
        let id = h.rows()[0].id;
        h.update(|view, cx| view.show_progress(id, cx));
        h.until_shown(t("When this download is done"));
        let past = h.words_past_the_edges();
        assert!(past.is_empty(), "progress window, {language}: {past:#?}");
        h.key("escape", &[]);
        h.until("the progress window to close", |h| h.dialogs().is_empty());
        h.command("downloads.scheduler");
        h.until_shown(t("Stop at"));
        let past = h.words_past_the_edges();
        assert!(past.is_empty(), "Scheduler, {language}: {past:#?}");
    }
    dm_desktop_base::i18n::set("en");
}

/// In German, where the list's headings are longest, no word of the main window is laid out
/// past the edge of what shows it: a column's heading ends in "…" in its column, and did not
/// begin outside it ("ertragungsrate" for "Übertragungsrate", gaps row E2).
#[test]
fn no_word_of_the_main_window_is_cut_unseen_in_german() {
    // The language and the text size are the process's.
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.general.language = "de".into();
    let mut h = world.harness_with(settings);
    dm_desktop_base::i18n::set("de");
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
    h.until_shown("Übertragungsrate");
    let cut = h.words_cut_unseen();
    dm_desktop_base::i18n::set("en");
    assert!(cut.is_empty(), "{cut:#?}");
}
