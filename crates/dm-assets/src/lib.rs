// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's files that are not code, compiled in from one folder, `assets/` (gaps row X9):
//! a program that cannot find its own files has none to show, so they travel inside it.
//!
//! - `icons/`: the window's pictures ([`ICONS`], [`icon`]).
//! - `i18n/`: the words in each language dm speaks ([`CATALOGS`]).
//! - `rules/`: the site rules shipped in the app ([`RULES`]).
//! - `app-icons/`: the program's own picture, for the installers and Windows' resources; the
//!   tray's is compiled in ([`APP_ICON_32`]).
//!
//! Beside them, read by the packaging alone: `macos/` (what is merged into dm.app's
//! `Info.plist`).

/// The pictures named, each at `icons/<name>.svg`, compiled in.
macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$((
            concat!("icons/", $name, ".svg"),
            include_bytes!(concat!("../../../assets/icons/", $name, ".svg")) as &[u8],
        )),*]
    };
}

/// Every picture of the window, by the path a view asks for.
pub const ICONS: &[(&str, &[u8])] = icons!(
    // A side-panel group's heading: open (up, to close it) or shut (down, to open it).
    "chevron-up",
    "chevron-down",
    // The toolbar's.
    "add",
    "resume",
    "stop",
    "stop-all",
    "delete",
    "delete-completed",
    // A row's own Details button (its progress window).
    "details",
    "options",
    "scheduler",
    "start-queue",
    "stop-queue",
    "grabber",
    // Each file kind's, before a row's name (B45).
    "kind-videos",
    "kind-audio",
    "kind-documents",
    "kind-apps",
    "kind-archives",
    "kind-pictures",
    "kind-other",
    // A pop-up button's, as the system draws one (up and down).
    "chevron-up-down",
    // The top bar's search box.
    "search",
    // The top bar's button that shows or hides the categories, as the
    // system's sidebar button.
    "sidebar",
    // The floating drop target's.
    "drop",
    // The side panel's places that are not a kind: All, Unfinished, Finished, a queue, a
    // person's own category.
    "place-all",
    "place-unfinished",
    "place-finished",
    "place-queue",
    "place-folder",
    // A row's own Open button (gaps row U13).
    "open",
    // A phone's dialog: its top bar's Back.
    "back",
    // A phone's floating Add URL button.
    "plus",
    // A phone's card menu: Share, and Copy link.
    "share",
    "link",
);

/// The picture at `path` (`icons/<name>.svg`), if dm has it.
pub fn icon(path: &str) -> Option<&'static [u8]> {
    ICONS
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, bytes)| *bytes)
}

/// dm's own picture at 32 by 32 pixels, a PNG: the tray's on Windows and Linux.
pub const APP_ICON_32: &[u8] = include_bytes!("../../../assets/app-icons/32x32.png");

/// The catalogs, as written: each language's English-to-translation map, in JSON.
pub const CATALOGS: [(&str, &str); 15] = [
    ("es", include_str!("../../../assets/i18n/es.json")),
    ("pt-BR", include_str!("../../../assets/i18n/pt-BR.json")),
    ("fr", include_str!("../../../assets/i18n/fr.json")),
    ("de", include_str!("../../../assets/i18n/de.json")),
    ("it", include_str!("../../../assets/i18n/it.json")),
    ("nl", include_str!("../../../assets/i18n/nl.json")),
    ("pl", include_str!("../../../assets/i18n/pl.json")),
    ("ru", include_str!("../../../assets/i18n/ru.json")),
    ("id", include_str!("../../../assets/i18n/id.json")),
    ("tr", include_str!("../../../assets/i18n/tr.json")),
    ("zh-CN", include_str!("../../../assets/i18n/zh-CN.json")),
    ("ja", include_str!("../../../assets/i18n/ja.json")),
    ("ko", include_str!("../../../assets/i18n/ko.json")),
    ("ar", include_str!("../../../assets/i18n/ar.json")),
    ("he", include_str!("../../../assets/i18n/he.json")),
];

/// The site rules shipped inside the app (`rules.json`), served to the extension and the
/// engine until a newer document reaches dm's home folder.
pub const RULES: &str = include_str!("../../../assets/rules/rules.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_picture_is_an_svg_found_by_its_path() {
        for (path, bytes) in ICONS {
            assert!(
                path.starts_with("icons/") && path.ends_with(".svg"),
                "{path}"
            );
            let text = std::str::from_utf8(bytes).expect(path);
            assert!(text.contains("<svg"), "{path} is not an SVG");
            assert_eq!(icon(path), Some(*bytes));
        }
        assert_eq!(icon("icons/none.svg"), None);
    }

    #[test]
    fn every_catalog_is_a_json_object_and_the_rules_are_json() {
        for (language, text) in CATALOGS {
            let trimmed = text.trim_start_matches('\u{feff}').trim_start();
            assert!(
                trimmed.starts_with('{'),
                "{language}'s catalog is not an object"
            );
        }
        assert!(RULES.trim_start().starts_with('{'));
        assert!(APP_ICON_32.starts_with(b"\x89PNG"));
    }
}
