// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

const CONTENT: &str = include_str!("../../../../extension/content.js");
const DOCUMENT: &str = include_str!("../../../../extension/document.js");

/// The text of `script` from `start` up to the next `end` after it.
fn between<'a>(script: &'a str, start: &str, end: &str) -> &'a str {
    let from = script.find(start).unwrap_or_else(|| panic!("no {start}")) + start.len();
    let to = script[from..]
        .find(end)
        .unwrap_or_else(|| panic!("no {end} after {start}"));
    &script[from..from + to]
}

/// The single-quoted strings in a piece of JavaScript.
fn quoted(text: &str) -> Vec<&str> {
    text.split('\'').skip(1).step_by(2).collect()
}

fn js_bytes(n: usize) -> String {
    match n {
        n if n == 1024 * 1024 => "1024 * 1024".into(),
        n if n % (1024 * 1024) == 0 => format!("{} * 1024 * 1024", n / (1024 * 1024)),
        n => format!("{n}"),
    }
}

#[test]
fn the_plain_panel_says_what_the_full_one_says() {
    // Each of the plain panel's words through the browser's catalog, its English the
    // fallback.
    for word in LITE_WORDS {
        let asked = format!("t('{}', '{}')", word.key, word.en);
        assert!(CONTENT.contains(&asked), "content.js does not say {asked}");
    }
    // The background's answers, which it says in English, said in the browser's language as
    // the full panel says them (`words::answered`), the English matched and its key asked.
    for word in [
        crate::words::PANEL_APP_READING,
        crate::words::PANEL_APP_HAS,
        crate::words::PANEL_APP_DOWNLOADING,
    ] {
        let pair = format!("['{}', '{}']", word.key, word.en);
        assert!(CONTENT.contains(&pair), "content.js does not answer {pair}");
    }
    assert!(CONTENT.contains("answered(reply.said"));
    // No word of the catalog spelled out in content.js other than as `t`'s English, but the
    // HLS line's label, which goes to the app as the list's own name for it.
    let label = format!("label: '{}'", crate::words::PANEL_HLS.en);
    let script = CONTENT.replacen(&label, "", 1);
    for word in crate::words::ALL {
        let spelled = format!("'{}'", word.en);
        let asked = format!("t('{}', {spelled})", word.key);
        let answer = format!("['{}', {spelled}]", word.key);
        assert_eq!(
            script.matches(&spelled).count(),
            script.matches(&asked).count() + script.matches(&answer).count(),
            "content.js spells out {spelled} outside t()"
        );
    }
    // The panel's words, said by the full panel too.
    let full = [
        include_str!("../../content/handoff.rs"),
        include_str!("../../content/show.rs"),
    ]
    .concat();
    for name in ["PANEL_DOWNLOAD_VIDEO", "PANEL_READING", "PANEL_SENDING"] {
        assert!(full.contains(name), "the full panel does not say {name}");
    }
    for name in ["PANEL_NOT_TAKEN", "PANEL_UPDATED", "PANEL_SENT"] {
        assert!(full.contains(name), "the full panel does not say {name}");
    }
}

#[test]
fn the_plain_panel_counts_a_click_as_the_full_one_does() {
    let drawn = between(CONTENT, "const asDrawn = () => {", "};");
    assert!(drawn.contains(&format!(">= {DRAWN_OPACITY}")), "{drawn}");
    assert!(drawn.contains(&format!("r.width <= {}", MAX_DRAWN.0)));
    assert!(drawn.contains(&format!("r.height <= {}", MAX_DRAWN.1)));
    let full = include_str!("../../content/build.rs");
    assert!(full.contains("decide::DRAWN_OPACITY") && full.contains("decide::MAX_DRAWN"));
    // Reddit's player elements, as the plain panel reads them.
    let lines = between(CONTENT, "const playerLines = () => {", "return lines;");
    let named = quoted(lines);
    for player in LITE_PLAYERS {
        assert!(named.contains(player), "content.js does not read {player}");
    }
}

#[test]
fn the_plain_scripts_keep_to_the_rules_named_here() {
    let shown = between(CONTENT, "const shownInTab = new Set([", "]);");
    assert_eq!(quoted(shown), SHOWN_IN_TAB, "content.js shownInTab");
    assert!(CONTENT.contains(&format!("const GESTURE_FOR_MS = {GESTURE_FOR_MS};")));
    let kept = between(CONTENT, "while (keptSize > ", ") keptSize");
    assert_eq!(
        kept,
        format!(
            "{} || bodies.length > {PAGE_BODIES_COUNT}",
            js_bytes(PAGE_BODIES_BYTES)
        )
    );
    assert!(CONTENT.contains(&format!("const TEXT_ADDRESS = /{TEXT_ADDRESS}/gi;")));
    assert!(CONTENT.contains(&format!("text.replace(/{ADDRESS_TAIL}/, '')")));

    assert!(DOCUMENT.contains(&format!("const STREAM_URL = /{STREAM_URL}/i;")));
    assert!(DOCUMENT.contains(&format!("const PIECE_URL = /{PIECE_URL}/i;")));
    assert!(DOCUMENT.contains(&format!("const MAX_ANSWER = {};", js_bytes(MAX_ANSWER))));
    assert!(DOCUMENT.contains(&format!("const MEDIA_ANSWER = {};", js_bytes(MEDIA_ANSWER))));
    assert!(DOCUMENT.contains(&format!("const EARLY_ANSWERS = {EARLY_ANSWERS};")));
    assert!(DOCUMENT.contains(&format!("const EARLY_BYTES = {};", js_bytes(EARLY_BYTES))));
    assert!(DOCUMENT.contains(&format!("const MAX_STEPS = {MAX_STEPS};")));
    assert!(DOCUMENT.contains(&format!("Date.now() - marked < {MAGNET_PASS_MS}")));
}
