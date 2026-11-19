// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Streams a page builds itself: the addresses in the JSON answer of a request a site's player made.
//!
//! The page script (`extension/document.js`) hands the extension the answer of a request the app's
//! rules for the site name (`SiteProfile`), read at most [`MAX_ANSWER_BYTES`] and only when it is
//! JSON. What reads that answer is here, so it is tested with no browser: a JSON document only (never
//! a script, never a page), a bounded walk, and only addresses that are plainly a stream or a media
//! file and not on the list of what is never caught.

use super::*;

/// The most of an answer that is read: the page script reads no more, and this refuses the same.
pub const MAX_ANSWER_BYTES: usize = 1024 * 1024;

/// The page script telling the extension the answer to a request the rules name.
pub const PAGE_ANSWER_MESSAGE: &str = "dm.page-answer";

/// The page script asking which requests of its site's answers are read.
pub const PAGE_RULES_MESSAGE: &str = "dm.page-rules";

/// The most addresses taken from one answer.
const MOST_ADDRESSES: usize = 50;
/// The deepest nesting walked, and the most values visited: an answer can be built to be slow.
const DEEPEST: usize = 24;
const MOST_VALUES: usize = 50_000;

/// What a media address ends in.
const MEDIA_EXTENSIONS: &[&str] = &["m3u8", "mpd", "mp4", "m4v", "m4a", "webm", "mp3", "mov"];

/// The stream addresses in `body`, an answer to a request of a player.
///
/// Strings are taken from the values under one of `keys` (anywhere below it), or every string
/// where `keys` is empty. A string counts when it is an `http(s)` address (or protocol-relative,
/// taken as `https`) whose path ends in a media extension, and is not on the never-caught list.
/// Nothing is returned for a body that is too large or is not JSON.
pub fn addresses_in(body: &str, keys: &[String]) -> Vec<String> {
    let rule = dm_protocol::AnswerRule {
        keys: keys.to_vec(),
        ..Default::default()
    };
    addresses_by(body, &rule, "")
}

/// [`addresses_in`] under a site's whole answer rule, for an answer to a page at `page_url`:
/// nothing from an answer whose `video_id` is another video than the one the page's address
/// names, and only the first member under a key of `first_of` (gaps row 214).
pub fn addresses_by(body: &str, rule: &dm_protocol::AnswerRule, page_url: &str) -> Vec<String> {
    if body.len() > MAX_ANSWER_BYTES {
        return Vec::new();
    }
    let Ok(document) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    if !of_the_pages_video(&document, &rule.video_id, page_url) {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut visited = 0usize;
    let walking = Walk {
        keys: &rule.keys,
        first_of: &rule.first_of,
    };
    walking.walk(
        &document,
        rule.keys.is_empty(),
        false,
        0,
        &mut visited,
        &mut found,
    );
    found
}

/// Whether an answer is for the video the page's address names. True where the rule names no id
/// path, the answer has nothing there, or the address names no video by number.
fn of_the_pages_video(document: &Value, id_path: &str, page_url: &str) -> bool {
    let id_path = id_path.trim();
    if id_path.is_empty() {
        return true;
    }
    let mut at = document;
    for step in id_path.split('.') {
        match at.get(step) {
            Some(next) => at = next,
            None => return true,
        }
    }
    let id = match at {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        _ => return true,
    };
    let path = page_url
        .split_once("://")
        .map_or(page_url, |(_, rest)| rest)
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let segments: Vec<&str> = path.split('/').skip(1).filter(|s| !s.is_empty()).collect();
    let numbered = |s: &&str| s.len() >= 2 && s.chars().all(|c| c.is_ascii_digit());
    !segments.iter().any(numbered) || segments.contains(&id.as_str())
}

struct Walk<'a> {
    keys: &'a [String],
    first_of: &'a [String],
}

impl Walk<'_> {
    fn walk(
        &self,
        value: &Value,
        taking: bool,
        first_only: bool,
        depth: usize,
        visited: &mut usize,
        found: &mut Vec<String>,
    ) {
        *visited += 1;
        if depth > DEEPEST || *visited > MOST_VALUES || found.len() >= MOST_ADDRESSES {
            return;
        }
        match value {
            Value::String(text) => {
                if taking {
                    if let Some(address) = stream_address(text) {
                        if !found.contains(&address) {
                            found.push(address);
                        }
                    }
                }
            }
            Value::Array(items) => {
                let take = if first_only { 1 } else { items.len() };
                for item in items.iter().take(take) {
                    self.walk(item, taking, false, depth + 1, visited, found);
                }
            }
            Value::Object(map) => {
                let take = if first_only { 1 } else { map.len() };
                for (name, item) in map.iter().take(take) {
                    let under = taking || self.keys.iter().any(|key| key == name);
                    let first = self.first_of.iter().any(|key| key == name);
                    self.walk(item, under, first, depth + 1, visited, found);
                }
            }
            _ => {}
        }
    }
}

/// `text` as an address a stream is fetched from, or `None`.
fn stream_address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.len() > 4096 || text.contains(char::is_whitespace) {
        return None;
    }
    let address = if let Some(rest) = text.strip_prefix("//") {
        format!("https://{rest}")
    } else if text.starts_with("https://") || text.starts_with("http://") {
        text.to_string()
    } else {
        return None;
    };
    let after_scheme = address.split_once("://")?.1;
    let path = after_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .split_once('/')
        .map_or("", |(_, path)| path);
    let name = path.rsplit('/').next().unwrap_or("");
    let extension = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    if !MEDIA_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    if excluded::address(&address).is_some() {
        return None;
    }
    Some(address)
}

#[cfg(test)]
mod tests;
