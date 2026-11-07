// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The site rules the app served in its hello, held for the extension's decisions.
//!
//! The extension is built with its own tables (`excluded.rs`); what the app serves is added to
//! them and never takes one away, so with nothing served, or a document this side does not read,
//! the extension decides exactly as it was built to. The worker sleeps and wakes: the web side keeps
//! the served rules in the browser's storage and installs them again on waking.

use dm_protocol::{Rules, RULES_FORMAT};
use std::cell::RefCell;

thread_local! {
    static SERVED: RefCell<Option<Rules>> = const { RefCell::new(None) };
}

/// Holds the rules the app served (or none: an app that served none, or a document in a format
/// this side does not read, leaves the built-in tables alone).
pub fn install(rules: Option<Rules>) {
    let usable = rules
        .filter(|rules| rules.format == RULES_FORMAT)
        .map(Rules::tidied);
    SERVED.with(|served| *served.borrow_mut() = usable);
}

/// Whether any rules are held.
pub fn installed() -> bool {
    SERVED.with(|served| served.borrow().is_some())
}

/// The rules held, as served, for storing.
pub fn held() -> Option<Rules> {
    SERVED.with(|served| served.borrow().clone())
}

/// Runs `read` on the rules held, or on empty ones where none are.
pub fn with_served<T>(read: impl FnOnce(&Rules) -> T) -> T {
    SERVED.with(|served| match &*served.borrow() {
        Some(rules) => read(rules),
        None => read(&Rules::default()),
    })
}

/// The `url_contains` parts of the answer rules for a page at `page_url`, which the page script
/// compares with the addresses its player asks for.
pub fn watched_for(page_url: &str) -> Vec<String> {
    let Some(site) = super::site_of(page_url) else {
        return Vec::new();
    };
    with_served(|rules| {
        rules
            .answers_for(&site)
            .into_iter()
            .map(|answer| answer.url_contains.clone())
            .collect()
    })
}

/// The finders for a page at `page_url`, as the page script reads them.
pub fn finders_for(page_url: &str) -> Vec<dm_protocol::Finder> {
    let Some(site) = super::site_of(page_url) else {
        return Vec::new();
    };
    with_served(|rules| rules.finders_for(&site).into_iter().cloned().collect())
}

/// The selector for the element a page at `page_url` keeps its video's title in, empty where
/// the rules name none (gaps row 124).
pub fn title_for(page_url: &str) -> String {
    let Some(site) = super::site_of(page_url) else {
        return String::new();
    };
    with_served(|rules| rules.title_for(&site).unwrap_or_default().to_string())
}

/// The private `fetch` paths for a page at `page_url`.
pub fn private_fetches_for(page_url: &str) -> Vec<String> {
    let Some(site) = super::site_of(page_url) else {
        return Vec::new();
    };
    with_served(|rules| {
        rules
            .private_fetches_for(&site)
            .into_iter()
            .cloned()
            .collect()
    })
}

/// The JSON keys of the answer rule for a page at `page_url` that watches `request_url`: the keys
/// whose strings are addresses (empty: every string). `None` where no rule of the page's site
/// names that request, which is how an answer nobody asked for is refused.
pub fn keys_for(page_url: &str, request_url: &str) -> Option<Vec<String>> {
    let site = super::site_of(page_url)?;
    let request = request_url.to_ascii_lowercase();
    with_served(|rules| {
        rules
            .answers_for(&site)
            .into_iter()
            .find(|answer| request.contains(answer.url_contains.as_str()))
            .map(|answer| answer.keys.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(json: &str) -> Rules {
        Rules::parse(json).expect("rules")
    }

    #[test]
    fn nothing_served_leaves_the_built_in_tables_alone_and_a_document_is_tidied_on_install() {
        install(None);
        assert!(!installed());
        assert!(with_served(|rules| rules.sites.is_empty()));
        install(Some(rules(
            r#"{ "format": 1, "revision": 1, "sites": [
                  { "host": "video.example", "answers": [
                      { "url_contains": "/api/play", "keys": ["hlsUrl"] } ] } ] }"#,
        )));
        assert!(installed());
        assert_eq!(
            watched_for("https://www.video.example/watch/1"),
            vec!["/api/play"]
        );
        assert_eq!(watched_for("https://m.video.example/x"), vec!["/api/play"]);
        assert!(watched_for("https://other.example/").is_empty());
        assert!(watched_for("not an address").is_empty());
        assert_eq!(
            keys_for(
                "https://video.example/watch/1",
                "https://video.example/API/Play?id=3"
            ),
            Some(vec!["hlsUrl".to_string()])
        );
        assert_eq!(
            keys_for(
                "https://video.example/watch/1",
                "https://video.example/api/other"
            ),
            None
        );
        assert_eq!(
            keys_for("https://other.example/", "https://other.example/api/play"),
            None
        );
        install(None);
        assert!(watched_for("https://video.example/").is_empty());
    }

    #[test]
    fn a_document_in_a_format_this_side_does_not_read_is_not_held() {
        install(Some(Rules {
            format: RULES_FORMAT + 1,
            ..Rules::default()
        }));
        assert!(!installed());
        assert_eq!(held(), None);
    }
}
