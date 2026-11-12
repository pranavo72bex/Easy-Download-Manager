// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every word the window translates is in every catalog, and no catalog keeps a word nothing
//! says: a missing one reads in English, which is a gap. Moved from the egui window's tests;
//! its check that every translation has a glyph in its own fonts is not, as this window draws
//! in the system's fonts.

use std::collections::{BTreeSet, HashMap};

use dm_desktop_base::i18n::{CATALOGS, LANGUAGES};

/// Every `.rs` file under `dir`, in folders too: the window's dialogs are a
/// module of their own, and a scan of the top folder alone would silently
/// stop checking their words. A module's `tests.rs` is its tests, whose
/// made-up strings are not the window's.
fn rust_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs")
            && path.file_name().is_some_and(|n| n != "tests.rs")
        {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Every English string the window passes through `t`, `tf` and the
/// helpers that call them, read from its source, and the words it chooses
/// at run time ([`run_time_words`]).
fn wanted() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    // The GPUI window's source and what it says through dm-desktop-base; and, while it is
    // still in the tree, the egui window's, whose words the catalogs keep until it goes.
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let base = here.join("../dm-desktop-base/src");
    assert!(base.is_dir(), "{} is not there", base.display());
    let egui = here.join("../dm-desktop/src");
    let files = rust_files(&here.join("src"))
        .into_iter()
        .chain(rust_files(&base))
        .chain(if egui.is_dir() {
            rust_files(&egui)
        } else {
            Vec::new()
        });
    if egui.is_dir() {
        // The egui window's text sizes, chosen at run time (`dialogs/controls.rs`).
        keys.extend(["Small", "Medium", "Large"].map(String::from));
    }
    for file in files {
        let text = std::fs::read_to_string(file).unwrap();
        // The window's source, not its tests' made-up strings.
        let text = text.split("#[cfg(test)]").next().unwrap_or_default();
        // `words` words a status; `window` titles a dialog; `Said::new` is a
        // status-line sentence; `primary` is a dialog's main button.
        for opener in [
            "t(",
            "tf(",
            "words(",
            "window(ctx,",
            "Said::new(",
            "Said::naming(",
            "primary(ui,",
            "tn(",
            "Said::counting(",
            "Said::counting_naming(",
        ] {
            let mut rest = text;
            while let Some(at) = rest.find(opener) {
                // Only a call: not the tail of another name ("restart(").
                let before = rest[..at].chars().last();
                rest = &rest[at + opener.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let Some(literal) = rest.trim_start().strip_prefix('"') else {
                    continue;
                };
                let mut key = String::new();
                let mut chars = literal.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => key.push('\n'),
                            Some(other) => key.push(other),
                            None => {}
                        },
                        c => key.push(c),
                    }
                }
                keys.insert(key);
            }
        }
    }
    keys.extend(run_time_words().into_iter().map(String::from));
    keys
}

/// The words the window chooses at run time and hands to `t` as a value, not a literal the
/// source scan can find: from the lists they are kept in.
fn run_time_words() -> Vec<&'static str> {
    use dm_desktop_base as base;
    let mut words: Vec<&'static str> = Vec::new();
    words.extend(base::options::TABS.iter().map(|(_, w)| *w));
    words.extend(base::options::THEMES.iter().map(|(_, w)| *w));
    words.extend(base::file_info::QUALITIES.iter().map(|(_, w)| *w));
    words.extend(base::held::DAYS);
    words.extend(base::toolbar::TOOL_STYLES.iter().map(|(_, w)| *w));
    words.extend(dm_shell::app::commands::TOOLBAR.iter().map(|(_, w)| *w));
    words.extend(
        dm_shell::app::commands::Command::ALL
            .iter()
            .map(|c| c.label()),
    );
    words.extend(base::columns::Column::ALL.iter().map(|c| c.english()));
    words.extend(dm_shell::app::power::WhenDone::ALL.iter().map(|w| w.verb()));
    words.extend(base::i18n::BUILT_IN_NAMES);
    words.extend(base::format::MONTHS);
    words.push(crate::drops::DROP_HINT);
    words.extend(base::format::REASONS.iter().map(|(_, plain)| *plain));
    words.extend(base::format::NEXT_STEPS.iter().map(|(_, step)| *step));
    words.extend(base::format::RETRY_CAUSES);
    // What a download may say of itself on its row (`rows::note_in`).
    words.extend(dm_core::NOTES);
    words.extend(dm_shell::app::places::place_menu_words());
    words
}

fn catalog(text: &str) -> HashMap<String, String> {
    serde_json::from_str(text).expect("a catalog that does not parse")
}

#[test]
fn every_translated_word_is_in_every_catalog() {
    let wanted = wanted();
    assert!(
        wanted.len() > 150,
        "the source scan found only {}",
        wanted.len()
    );
    let mut missing = Vec::new();
    for (code, text) in CATALOGS {
        let have = catalog(text);
        for key in &wanted {
            if !have.contains_key(key) {
                missing.push(format!("{code}: {key:?}"));
            }
        }
        // Each `{}` of the English is in the translation as often: in
        // each of its forms, where it has forms for a count.
        let forms = dm_desktop_base::i18n::FORMS;
        for (english, translated) in &have {
            let first = english.split(forms).next().unwrap_or_default();
            for form in translated.split(forms) {
                assert_eq!(
                    dm_desktop_base::i18n::placeholders(first),
                    dm_desktop_base::i18n::placeholders(form),
                    "{code}: {english:?} → {translated:?} takes other values"
                );
            }
            if english.contains(forms) {
                // As many forms as the language's grammar has.
                let wanted = match code {
                    "ru" | "pl" => 3,
                    "id" | "tr" | "zh-CN" | "ja" | "ko" => 1,
                    _ => 2,
                };
                assert_eq!(
                    translated.split(forms).count(),
                    wanted,
                    "{code}: {english:?} → {translated:?} has the wrong number of forms"
                );
            } else {
                assert!(
                    !translated.contains(forms),
                    "{code}: {english:?} → {translated:?} has forms the English has not"
                );
            }
            assert!(
                !translated.trim().is_empty(),
                "{code}: {english:?} is empty"
            );
        }
    }
    assert!(
        missing.is_empty(),
        "{} missing, first: {:#?}",
        missing.len(),
        &missing[..missing.len().min(20)]
    );
    assert_eq!(
        LANGUAGES.len(),
        CATALOGS.len() + 1,
        "English and one catalog each"
    );
}

#[test]
fn no_catalog_keeps_words_the_window_no_longer_says() {
    let wanted = wanted();
    for (code, text) in CATALOGS {
        let stale: Vec<String> = catalog(text)
            .into_keys()
            .filter(|k| !wanted.contains(k))
            .collect();
        assert!(stale.is_empty(), "{code} has words nothing says: {stale:?}");
    }
}

/// Where `text` (what follows a call's opening bracket) closes the call: the
/// first `)` outside brackets and string literals of its own.
fn call_end(text: &str) -> usize {
    let mut depth = 0usize;
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => {
                while let Some((_, c)) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => return at,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    text.len()
}

/// Each string literal in `text`, with what is written just before it; not
/// those in a list (`&["app"]`), which are a panel's file endings, not words.
fn literals(text: &str) -> Vec<(String, &str)> {
    let mut found = Vec::new();
    let mut listed = 0usize;
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '[' => listed += 1,
            ']' => listed = listed.saturating_sub(1),
            _ => {}
        }
        if c != '"' {
            continue;
        }
        let mut literal = String::new();
        while let Some((_, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, escaped)) = chars.next() {
                        literal.push(escaped);
                    }
                }
                '"' => break,
                c => literal.push(c),
            }
        }
        if listed == 0 {
            found.push((literal, text[..at].trim_end()));
        }
    }
    found
}

/// The GPUI window's own words go through the catalogs: a literal handed
/// straight to where a person sees or hears it (a status sentence, a
/// problem, a screen reader's name, a menu item, a system panel's title or
/// filter) reads in English whatever the language (gaps row U21). A
/// literal there must be the English a `t`, `tf` or `tn` call is given,
/// which the scan above then holds to be in every catalog.
#[test]
fn the_gpui_window_hands_no_english_straight_to_a_person() {
    let gpui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // Where words go to be seen or heard.
    const SHOWN: [&str; 9] = [
        ".aria_label(",
        ".say(",
        "problems.push(",
        ".note(",
        "set_title(",
        "add_filter(",
        "MenuItem::action(",
        "MenuItem::os_submenu(",
        "Menu::new(",
    ];
    // dm's own name, the same in every language.
    const SAME_EVERYWHERE: [&str; 1] = ["dm"];
    let mut english = Vec::new();
    for file in rust_files(&gpui) {
        let text = std::fs::read_to_string(&file).unwrap();
        let text = text.split("#[cfg(test)]").next().unwrap_or_default();
        for sink in SHOWN {
            for (at, _) in text.match_indices(sink) {
                let rest = &text[at + sink.len()..];
                for (literal, before) in literals(&rest[..call_end(rest)]) {
                    // A literal of placeholders alone ("{}%") has no words.
                    let mut words = literal.clone();
                    while let (Some(open), Some(close)) = (words.find('{'), words.find('}')) {
                        if close < open {
                            break;
                        }
                        words.replace_range(open..=close, "");
                    }
                    if SAME_EVERYWHERE.contains(&literal.as_str())
                        || !words.chars().any(char::is_alphabetic)
                    {
                        continue;
                    }
                    let translated = ["t(", "tf(", "tn("].iter().any(|call| {
                        before.strip_suffix(call).is_some_and(|head| {
                            !head
                                .chars()
                                .last()
                                .is_some_and(|c| c.is_alphanumeric() || c == '_')
                        })
                    });
                    if !translated {
                        let name = file.file_name().unwrap_or_default().to_string_lossy();
                        english.push(format!("{name}: {sink} {literal:?}"));
                    }
                }
            }
        }
    }
    assert!(
        english.is_empty(),
        "English handed straight to a person: {english:#?}"
    );
}

/// "Everything" heads the category tree and "All" is its first child: one
/// word for both read as the same place twice.
#[test]
fn everything_and_all_are_two_words_in_every_language() {
    for (code, text) in CATALOGS {
        let have = catalog(text);
        assert_ne!(
            have.get("All Downloads"),
            have.get("All"),
            "{code} names the tree and its first place alike"
        );
    }
}
