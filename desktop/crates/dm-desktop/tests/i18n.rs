//! Every word the window translates is in every catalog, and every
//! translation can be drawn in the window's fonts: a missing one reads in
//! English, which is a gap; a glyph the fonts lack is a box, which is a bug.

use std::collections::{BTreeSet, HashMap};

use dm_desktop::i18n::{CATALOGS, LANGUAGES};

/// Every English string the window passes through `t`, `tf` and the
/// helpers that call them, read from its source, and the words it chooses
/// at run time, as the window itself lists them.
fn wanted() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(src).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        // The window's source, not its tests' made-up strings.
        let text = text.split("#[cfg(test)]").next().unwrap_or_default();
        // `words` words a status; `window` titles a dialog; `Said::new` is a
        // status-line sentence; `primary` is a dialog's main button.
        for opener in [
            "t(",
            "tf(",
            "words(",
            "window(",
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
    keys.extend(
        dm_desktop::app::run_time_words()
            .into_iter()
            .map(String::from),
    );
    keys
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
        let forms = dm_desktop::i18n::FORMS;
        for (english, translated) in &have {
            let first = english.split(forms).next().unwrap_or_default();
            for form in translated.split(forms) {
                assert_eq!(
                    dm_desktop::i18n::placeholders(first),
                    dm_desktop::i18n::placeholders(form),
                    "{code}: {english:?} → {translated:?} takes other values"
                );
            }
            if english.contains(forms) {
                // As many forms as the language's grammar has.
                let wanted = match code {
                    "ru" | "pl" => 3,
                    "id" | "tr" => 1,
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

#[test]
fn every_translation_is_drawn_in_the_windows_fonts() {
    let ctx = egui::Context::default();
    dm_desktop::theme::apply(&ctx, dm_desktop::theme::Theme::Light);
    // Fonts are put in place on the first pass.
    for _ in 0..2 {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            dm_desktop::theme::apply(ui.ctx(), dm_desktop::theme::Theme::Light);
        });
        out.textures_delta.clear();
    }
    let mut boxes = Vec::new();
    for (code, text) in CATALOGS {
        for translated in catalog(text).values() {
            for family in [
                egui::FontFamily::Proportional,
                dm_desktop::theme::semibold(),
            ] {
                let id = egui::FontId::new(13.0, family);
                let drawn = ctx.fonts_mut(|f| {
                    translated
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .all(|c| f.has_glyph(&id, c))
                });
                if !drawn {
                    boxes.push(format!("{code}: {translated:?}"));
                }
            }
        }
        for (_, name) in LANGUAGES {
            let id = egui::FontId::proportional(13.0);
            assert!(
                ctx.fonts_mut(|f| name
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .all(|c| f.has_glyph(&id, c))),
                "{name} cannot be drawn"
            );
        }
    }
    assert!(
        boxes.is_empty(),
        "drawn as boxes: {:?}",
        &boxes[..boxes.len().min(10)]
    );
}

/// "Everything" heads the category tree and "All" is its first child: one
/// word for both read as the same place twice.
#[test]
fn everything_and_all_are_two_words_in_every_language() {
    for (code, text) in CATALOGS {
        let have = catalog(text);
        assert_ne!(
            have.get("Everything"),
            have.get("All"),
            "{code} names the tree and its first place alike"
        );
    }
}
