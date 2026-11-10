// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The rules of the two scripts that stay plain JavaScript (gaps row 169): `extension/content.js`
//! must work where a page's policy forbids WebAssembly (Firefox applies it to content scripts;
//! decisions, 2026-10-17), and `extension/document.js` runs in the page's own world, under the
//! page's policy. They are named here, once, and each script is held to them by a test below, as
//! the message names are: a rule changed here and not there fails the build.

// What the panel says, the full one (`content/handoff.rs`) and the plain one in `content.js`
// (where the full one's WebAssembly cannot load) alike.

/// The panel's button.
pub const SAY_DOWNLOAD_VIDEO: &str = "Download video";
/// While the app reads the video's qualities.
pub const SAY_READING: &str = "Reading qualities…";
/// While the choice is handed to the app.
pub const SAY_SENDING: &str = "Sending to dm…";
/// The app refused it.
pub const SAY_NOT_TAKEN: &str = "dm could not take it";
/// The page's script is older than the extension it talks to.
pub const SAY_UPDATED: &str = "dm was updated: reload this page to use it";

// What only the plain panel says.

/// Before what the app said of it.
pub const LITE_SENT: &str = "Sent to dm. ";
/// The app gave no answer to the list of qualities.
pub const LITE_NO_ANSWER: &str = "dm did not answer";
/// Asking for the list failed.
pub const LITE_FAILED: &str =
    "dm did not answer. Try again, or right-click the video and choose Download media with dm.";
/// The line for the HLS playlist the page's own player names.
pub const LITE_HLS: &str = "HLS stream, best quality";

/// The elements whose `src` names a video's HLS playlist, picture and sound together (Reddit's
/// player), read by the plain panel where the MP4 files the player loads are the picture alone.
pub const LITE_PLAYERS: &[&str] = &["shreddit-player", "shreddit-player-2"];

/// A panel counts a click only while it is as dm drew it: no more see-through than this ...
pub const DRAWN_OPACITY: f64 = 0.99;
/// ... and no larger than this, width and height in CSS pixels (a page that stretched the panel
/// over the window would have a person's next click land on it).
pub const MAX_DRAWN: (f64, f64) = (480.0, 640.0);

/// The kinds of file a browser shows in the tab and never downloads: a plain click on a link to
/// one starts no download for the downloads list to catch, so `content.js` asks the app first.
pub const SHOWN_IN_TAB: &[&str] = &[
    "pdf", "mp4", "m4v", "webm", "ogv", "mov", "mp3", "m4a", "wav", "ogg", "opus", "flac", "aac",
];

/// How long after a person's own click or key press a magnet a page opens with script counts as
/// theirs (security review, 2026-11-07).
pub const GESTURE_FOR_MS: u32 = 1500;

/// The page's own answers about media kept in `content.js` for the panel's list: the newest,
/// at most this much text ...
pub const PAGE_BODIES_BYTES: usize = 12 * 1024 * 1024;
/// ... and this many.
pub const PAGE_BODIES_COUNT: usize = 40;

/// An address written in a page's text or inline scripts, for "Download all/selected links", and
/// the punctuation trimmed off its end ([`ADDRESS_TAIL`]).
pub const TEXT_ADDRESS: &str = r#"\bhttps?:\/\/(?:[^\s:@/<>"']+(?::[^\s@/<>"']*)?@)?[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+(?::\d{1,5})?(?:[/?#][^\s<>"'`]*)?"#;
/// The punctuation trimmed off the end of an address found in text (a sentence's full stop, a
/// closing bracket).
pub const ADDRESS_TAIL: &str = r#"[).,;:!?\]}>'"]+$"#;

/// A manifest, a playlist or a whole media file a player asks for, which `document.js` tells the
/// extension of. What is a stream is decided in Rust (`decide/streams.rs`); this and
/// [`PIECE_URL`] only keep the traffic small.
pub const STREAM_URL: &str = r"\.(m3u8|mpd|webm|mp4|m4a|aac|f4m)(\?|#|$)";
/// The pieces of a stream `document.js` does not tell of: a player asks for hundreds.
pub const PIECE_URL: &str =
    r"\.(ts|m4s|m4f|cmfv|cmfa)(\?|#|$)|[?&](range|bytestart|byterange|sq|segment)=";

/// The most of an answer the app's rules name that `document.js` passes on.
pub const MAX_ANSWER: usize = 1024 * 1024;
/// The most of any answer `document.js` reads for media.
pub const MEDIA_ANSWER: usize = 2 * 1024 * 1024;

/// The answers kept before the rules for the site arrive: at most this many ...
pub const EARLY_ANSWERS: usize = 24;
/// ... and this much.
pub const EARLY_BYTES: usize = 4 * 1024 * 1024;

/// The most steps a property path or a selector chain of the app's rules takes in the page.
pub const MAX_STEPS: usize = 32;

/// How long after `content.js` opened a magnet the page's own way `document.js` leaves the next
/// one alone, so the browser opens it rather than offering it to dm again.
pub const MAGNET_PASS_MS: u32 = 3000;

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: &str = include_str!("../../../extension/content.js");
    const DOCUMENT: &str = include_str!("../../../extension/document.js");

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
        for words in [
            SAY_DOWNLOAD_VIDEO,
            SAY_READING,
            SAY_SENDING,
            SAY_NOT_TAKEN,
            SAY_UPDATED,
            LITE_SENT,
            LITE_NO_ANSWER,
            LITE_FAILED,
            LITE_HLS,
        ] {
            assert!(
                CONTENT.contains(&format!("'{words}'")),
                "content.js: {words}"
            );
        }
        // The full panel says the shared words from here, not words of its own (in its code; a
        // comment may quote them).
        let full = include_str!("../content/handoff.rs");
        let code: String = full
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for (name, words) in [
            ("SAY_DOWNLOAD_VIDEO", SAY_DOWNLOAD_VIDEO),
            ("SAY_READING", SAY_READING),
            ("SAY_SENDING", SAY_SENDING),
            ("SAY_NOT_TAKEN", SAY_NOT_TAKEN),
            ("SAY_UPDATED", SAY_UPDATED),
        ] {
            assert!(full.contains(name), "handoff.rs does not use {name}");
            assert!(
                !code.contains(&format!("\"{words}\"")),
                "handoff.rs spells out {words}"
            );
        }
    }

    #[test]
    fn the_plain_panel_counts_a_click_as_the_full_one_does() {
        let drawn = between(CONTENT, "const asDrawn = () => {", "};");
        assert!(drawn.contains(&format!(">= {DRAWN_OPACITY}")), "{drawn}");
        assert!(drawn.contains(&format!("r.width <= {}", MAX_DRAWN.0)));
        assert!(drawn.contains(&format!("r.height <= {}", MAX_DRAWN.1)));
        let full = include_str!("../content/build.rs");
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
}
