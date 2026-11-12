// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The extension's catalogs, by the folder each is in (`pt_BR`).
fn catalogs() -> BTreeMap<String, serde_json::Map<String, Value>> {
    let dir = repo().join("extension/_locales");
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(entry.path().join("messages.json"))
            .unwrap_or_else(|e| panic!("{name}/messages.json: {e}"));
        let json: Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}/messages.json: {e}"));
        let Value::Object(map) = json else {
            panic!("{name}/messages.json is not an object")
        };
        found.insert(name, map);
    }
    found
}

/// The app's languages as the browser names its catalogs: `pt-BR.json` is `pt_BR`.
fn app_languages() -> BTreeMap<String, BTreeMap<String, String>> {
    let dir = repo().join("desktop/crates/dm-desktop-base/i18n");
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .replace('-', "_");
        let words: BTreeMap<String, String> =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        found.insert(stem, words);
    }
    found
}

/// The gaps a catalog message names, `$NAME$`, lowercased; `$$` is a dollar sign.
fn gaps(message: &str) -> Vec<String> {
    let text = message.replace("$$", "");
    let parts: Vec<&str> = text.split('$').collect();
    assert!(parts.len() % 2 == 1, "an unclosed $ in {message:?}");
    parts
        .iter()
        .skip(1)
        .step_by(2)
        .map(|p| p.to_ascii_lowercase())
        .collect()
}

#[test]
fn every_word_is_listed_once() {
    let source = include_str!("../words.rs");
    let defined = source.matches(": Word = Word {").count();
    assert_eq!(defined, ALL.len(), "a word is missing from ALL");
    let keys: BTreeSet<&str> = ALL.iter().map(|w| w.key).collect();
    assert_eq!(keys.len(), ALL.len(), "two words share a key");
    for word in ALL {
        let mut named = gaps(word.en);
        named.sort();
        let mut places: Vec<String> = word.places.iter().map(|p| p.to_string()).collect();
        places.sort();
        assert_eq!(
            named, places,
            "{}: its gaps and its places differ",
            word.key
        );
        assert!(
            word.key.chars().all(|c| c.is_ascii_alphanumeric()),
            "{}: the browser takes letters and digits in a key",
            word.key
        );
    }
}

#[test]
fn the_english_fills_its_gaps_once_and_in_order() {
    assert_eq!(PANEL_NEVER_ON.english(&["e.com"]), "Never on e.com");
    assert_eq!(POPUP_AND.english(&["a.com", "b.com"]), "a.com and b.com");
    assert_eq!(
        POPUP_NOT_ANSWERING.english(&["timed out", "dm-desktop --register"]),
        "dm did not answer (timed out). Open dm (if it's already open, open it again), then \
         reopen this popup. If it still does not answer: dm-desktop --register"
    );
    // What fills a gap is not read for gaps of its own.
    assert_eq!(PANEL_NEVER_ON.english(&["$SITE$"]), "Never on $SITE$");
    // A gap with nothing given is empty; a word with no gaps is as written.
    assert_eq!(PANEL_NEVER_ON.english(&[]), "Never on ");
    assert_eq!(PANEL_HIDE.say(), "Hide");
    // A host has no browser to ask: the English.
    assert_eq!(
        POPUP_PANEL_ON.with(&["e.com"]),
        "Show “Download video” on e.com"
    );
}

#[test]
fn the_panel_says_the_backgrounds_answers_in_its_words() {
    assert_eq!(answered(PANEL_APP_HAS.en), PANEL_APP_HAS.en);
    assert_eq!(
        answered("the browser is downloading it (dm: no app)"),
        "the browser is downloading it (dm: no app)",
        "a reason is said as it came"
    );
}

/// The words built from parts say what they said before they were in a catalog.
#[test]
fn the_english_is_what_the_extension_said_before() {
    use crate::decide::{absent_popup_text, absent_text, status_line, wider_note, with_site};
    use dm_protocol::{Hello, CHROME_EXTENSION_ID};
    let hello: Hello = serde_json::from_value(serde_json::json!({
        "protocolVersion": 1, "type": "engine.hello", "engineVersion": "3.0.1", "protocol": 2,
    }))
    .unwrap();
    assert_eq!(
        status_line(&hello),
        ("Connected to dm 3.0.1".to_string(), false)
    );
    let old: Hello = serde_json::from_value(serde_json::json!({
        "protocolVersion": 1, "type": "engine.hello", "engineVersion": "2.0.0",
    }))
    .unwrap();
    assert_eq!(
        status_line(&old),
        (
            "Connected to dm 2.0.0 · update the dm app: until then the browser keeps its downloads"
                .to_string(),
            true
        )
    );
    assert_eq!(
        absent_popup_text(
            "Specified native messaging host not found.",
            CHROME_EXTENSION_ID,
            false
        ),
        "dm isn't connected to this browser yet. No dm app on this computer yet? click Get the dm \
         app below, install it and open it once. Already installed: open dm once (if it's already \
         open, open it again) so it can connect, then reopen this popup. Advanced: dm-desktop \
         --register"
    );
    assert_eq!(
        absent_text("Specified native messaging host not found.", "abc", false),
        "dm isn't connected to this browser yet. No dm app on this computer yet? get it from \
         github.com/pranavo72bex/Easy-Download-Manager/releases, install it and open it once. \
         Already installed: open dm once (if it's already open, open it again) so it can \
         connect, then try again. Advanced: dm-desktop --register abc"
    );
    assert_eq!(
        absent_popup_text("timed out", "abc", false),
        "dm did not answer (timed out). Open dm (if it's already open, open it again), then \
         reopen this popup. If it still does not answer: dm-desktop --register abc"
    );
    assert_eq!(
        absent_text("timed out", "abc", true),
        "dm did not answer (timed out). Open dm (if it's already open, open it again), then try \
         again. If it still does not answer: dm-desktop --register"
    );
    let listed = with_site(&with_site(&[], "e.com", true), "dl.e.com", true);
    assert_eq!(
        wider_note(&listed, "a.dl.e.com").as_deref(),
        Some("That was set for all of dl.e.com and e.com, so it is changed there too")
    );
    assert_eq!(
        crate::decide::reference_line(1, "Clip", "1080p — 50.1 MB"),
        "1. Clip… MP4 file, quality 1080p HD — 50.1 MB"
    );
}

/// Every language the app speaks has a catalog, and every catalog has every
/// word, with the gaps the English has, declared as the browser's
/// placeholders in order. The English catalog is the English here.
#[test]
fn every_catalog_has_every_word() {
    let catalogs = catalogs();
    let mut wanted: BTreeSet<String> = app_languages().into_keys().collect();
    wanted.insert("en".into());
    let have: BTreeSet<String> = catalogs.keys().cloned().collect();
    assert_eq!(
        have, wanted,
        "the extension's catalogs are not the app's languages"
    );
    assert!(wanted.len() >= 14, "{wanted:?}");
    let keys: BTreeSet<&str> = ALL.iter().map(|w| w.key).collect();
    for (language, catalog) in &catalogs {
        let named: BTreeSet<&str> = catalog.keys().map(String::as_str).collect();
        assert_eq!(named, keys, "{language}: its keys are not the words'");
        for word in ALL {
            let entry = &catalog[word.key];
            let message = entry["message"]
                .as_str()
                .unwrap_or_else(|| panic!("{language}/{}: no message", word.key));
            assert!(!message.trim().is_empty(), "{language}/{}", word.key);
            assert_eq!(
                message.trim(),
                message,
                "{language}/{}: spaces at an end",
                word.key
            );
            if language == "en" {
                assert_eq!(message, word.en, "en/{}", word.key);
            }
            let mut said = gaps(message);
            said.sort();
            let mut places: Vec<String> = word.places.iter().map(|p| p.to_string()).collect();
            places.sort();
            assert_eq!(said, places, "{language}/{}: {message}", word.key);
            let declared = entry.get("placeholders").and_then(Value::as_object);
            for (at, place) in word.places.iter().enumerate() {
                let content = declared
                    .and_then(|d| d.get(*place))
                    .and_then(|p| p["content"].as_str());
                assert_eq!(
                    content,
                    Some(format!("${}", at + 1).as_str()),
                    "{language}/{}: placeholder {place}",
                    word.key
                );
            }
            assert_eq!(
                declared.map_or(0, |d| d.len()),
                word.places.len(),
                "{language}/{}: placeholders it does not use",
                word.key
            );
        }
        // The stores' limits.
        let description = catalog["extDescription"]["message"].as_str().unwrap();
        assert!(
            description.chars().count() <= 132,
            "{language}: {description}"
        );
        let name = catalog["extName"]["message"].as_str().unwrap();
        assert!(name.chars().count() <= 45, "{language}: {name}");
        assert!(name.starts_with("dm"), "{language}: {name}");
    }
}

/// Where the app says the same English, the extension says what the app says.
#[test]
fn the_catalogs_say_what_the_app_says() {
    let catalogs = catalogs();
    for (language, app) in app_languages() {
        let catalog = &catalogs[&language];
        for word in ALL.iter().filter(|w| w.places.is_empty()) {
            if let Some(theirs) = app.get(word.en) {
                assert_eq!(
                    catalog[word.key]["message"].as_str(),
                    Some(theirs.as_str()),
                    "{language}/{}: the app says {theirs:?}",
                    word.key
                );
            }
        }
    }
}

#[test]
fn the_manifests_name_the_extension_from_the_catalogs() {
    for manifest in [
        include_str!("../../../extension/manifest.json"),
        include_str!("../../../extension/manifest.firefox.json"),
    ] {
        let manifest: Value = serde_json::from_str(manifest).unwrap();
        assert_eq!(manifest["default_locale"], "en");
        assert_eq!(manifest["name"], format!("__MSG_{}__", EXT_NAME.key));
        assert_eq!(
            manifest["description"],
            format!("__MSG_{}__", EXT_DESCRIPTION.key)
        );
    }
}

/// The popup's own words are each marked with their key, in English as the
/// word says it; nothing else in it is words but dm's name and its mark.
#[test]
fn the_popups_words_are_each_in_the_catalog() {
    let html = include_str!("../../../extension/popup.html");
    let body = &html[html.find("<body>").unwrap()..html.find("<script").unwrap()];
    let mut marked = 0;
    let mut rest = String::new();
    let mut at = body;
    while let Some(start) = at.find("data-i18n=\"") {
        rest.push_str(&at[..start]);
        let after = &at[start + 11..];
        let key = &after[..after.find('"').unwrap()];
        let open = after.find('>').unwrap();
        let text = &after[open + 1..];
        let text = &text[..text.find('<').unwrap()];
        let word = ALL
            .iter()
            .find(|w| w.key == key)
            .unwrap_or_else(|| panic!("popup.html: no word {key}"));
        assert_eq!(text, word.en, "popup.html: {key}");
        marked += 1;
        at = &after[open + 1 + text.len()..];
    }
    rest.push_str(at);
    assert_eq!(marked, 8);
    // The text left between tags.
    let left: Vec<&str> = rest
        .split('>')
        .filter_map(|part| part.split('<').next())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect();
    assert_eq!(
        left,
        ["&#8595;", "dm"],
        "popup.html: words outside the catalog"
    );
}

/// The panel's markup names its buttons in English; the page script names
/// them again in the browser's language, by [`crate::panel::NAMED`].
#[test]
fn the_panels_button_names_are_each_in_the_catalog() {
    use crate::panel::{BODY, NAMED};
    let mut named = BTreeSet::new();
    for (selector, attributes, word) in NAMED {
        let class = selector.trim_start_matches('.');
        let tag = BODY
            .split('<')
            .find(|t| t.contains(&format!("class=\"{class}\"")))
            .unwrap_or_else(|| panic!("no {selector} in BODY"));
        for attribute in *attributes {
            assert!(
                tag.contains(&format!("{attribute}=\"{}\"", word.en)),
                "{selector} {attribute}"
            );
            named.insert(word.en);
        }
    }
    // Every name BODY gives is one of those, or dm's own.
    for attribute in ["title=\"", "aria-label=\""] {
        for part in BODY.split(attribute).skip(1) {
            let value = &part[..part.find('"').unwrap()];
            assert!(value == "dm" || named.contains(value), "BODY: {value}");
        }
    }
}

/// The string literals in Rust source, outside comments (a raw string's too).
fn literals(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\'' if chars.get(i + 2) == Some(&'\'') => i += 3,
            '\'' if chars.get(i + 1) == Some(&'\\') => {
                i += 2;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
            }
            'r' if matches!(chars.get(i + 1), Some('#') | Some('"'))
                && (i == 0 || !chars[i - 1].is_alphanumeric()) =>
            {
                let hashes = chars[i + 1..].iter().take_while(|c| **c == '#').count();
                let open = i + 1 + hashes;
                if chars.get(open) != Some(&'"') {
                    i += 1;
                    continue;
                }
                let close: String = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                let rest: String = chars[open + 1..].iter().collect();
                let end = rest.find(&close).unwrap();
                found.push(rest[..end].to_string());
                i = open + 1 + rest[..end].chars().count() + close.chars().count();
            }
            '"' => {
                let mut text = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                        // A line continuation: the break and the next line's indent go.
                        if chars.get(i) == Some(&'\n') {
                            while chars.get(i + 1).is_some_and(|c| c.is_whitespace()) {
                                i += 1;
                            }
                            i += 1;
                            continue;
                        }
                    }
                    if let Some(c) = chars.get(i) {
                        text.push(*c);
                    }
                    i += 1;
                }
                found.push(text);
                i += 1;
            }
            _ => i += 1,
        }
    }
    found
}

/// No word a person reads in the popup, the menus or the panel is written
/// out in their code: each comes from the catalog. A literal of two words or
/// more is words, unless it is one of the few below that are not shown (a
/// class list, a log line, the background's reason a test compares with).
#[test]
fn no_word_a_person_reads_is_written_in_the_code() {
    const NOT_SHOWN: &[&str] = &[
        // Class lists.
        "item all",
        "wrap dead",
        "wrap done",
        "panel compact",
        "status bad",
        "row off",
        // Selectors.
        "video, audio",
        ".part .{class}",
        // The console's, for a developer.
        "dm: the panel's message was not delivered:",
        // How the background's own reason starts, compared with and not shown here.
        "the browser is downloading it",
    ];
    let sources = [
        ("popup.rs", include_str!("../popup.rs")),
        ("content.rs", include_str!("../content.rs")),
        ("content/build.rs", include_str!("../content/build.rs")),
        ("content/dom.rs", include_str!("../content/dom.rs")),
        ("content/events.rs", include_str!("../content/events.rs")),
        ("content/handoff.rs", include_str!("../content/handoff.rs")),
        (
            "content/keyboard.rs",
            include_str!("../content/keyboard.rs"),
        ),
        ("content/rules.rs", include_str!("../content/rules.rs")),
        ("content/show.rs", include_str!("../content/show.rs")),
        ("content/visible.rs", include_str!("../content/visible.rs")),
        ("content/bodies.rs", include_str!("../content/bodies.rs")),
    ];
    // The menus' and the panel's decisions also give the background its reasons, which its
    // notifications say: held here only to the catalog's words.
    let shared = [
        ("decide/menu.rs", include_str!("../decide/menu.rs")),
        (
            "decide/media_panel.rs",
            include_str!("../decide/media_panel.rs"),
        ),
    ];
    for (file, source) in sources.iter().chain(&shared) {
        // The tests' own text is not the code's.
        let code = source.split("#[cfg(test)]").next().unwrap_or(source);
        let prose = sources.iter().any(|(name, _)| name == file);
        for literal in literals(code).into_iter().filter(|_| prose) {
            // A format's gaps (`{label}`) are not words.
            let text: String = literal
                .split('{')
                .map(|part| part.split_once('}').map_or(part, |(_, after)| after))
                .collect::<Vec<_>>()
                .join(" ");
            let words = text
                .split_whitespace()
                .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2)
                .count();
            let markup = literal.contains('<');
            if words >= 2 && !markup && !NOT_SHOWN.contains(&literal.as_str()) {
                panic!("{file}: {literal:?} is written in the code, not taken from the catalog");
            }
        }
        // A lowercase single word ("to") is also a class or a key.
        let checked = ALL
            .iter()
            .filter(|w| w.en.contains(' ') || !w.en.chars().all(|c| c.is_ascii_lowercase()));
        for word in checked {
            assert!(
                !literals(code).iter().any(|l| l == word.en),
                "{file} spells out {:?}",
                word.en
            );
        }
    }
}
