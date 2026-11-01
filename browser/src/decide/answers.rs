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
    if body.len() > MAX_ANSWER_BYTES {
        return Vec::new();
    }
    let Ok(document) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut visited = 0usize;
    walk(
        &document,
        keys.is_empty(),
        keys,
        0,
        &mut visited,
        &mut found,
    );
    found
}

fn walk(
    value: &Value,
    taking: bool,
    keys: &[String],
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
            for item in items {
                walk(item, taking, keys, depth + 1, visited, found);
            }
        }
        Value::Object(map) => {
            for (name, item) in map {
                let under = taking || keys.iter().any(|key| key == name);
                walk(item, under, keys, depth + 1, visited, found);
            }
        }
        _ => {}
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
mod tests {
    use super::*;

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn the_stream_a_player_answer_names_is_found_under_its_key() {
        let body = r#"{
            "data": { "title": "A talk", "thumbnail": "https://img.example/t.jpg",
                      "playback": { "hlsUrl": "https:\/\/cdn.example\/v\/master.m3u8?sig=abc",
                                    "dash": "//cdn.example/v/manifest.mpd" },
                      "other": "https://cdn.example/not-listed.mp4" } }"#;
        assert_eq!(
            addresses_in(body, &keys(&["hlsUrl", "dash"])),
            vec![
                "https://cdn.example/v/master.m3u8?sig=abc",
                "https://cdn.example/v/manifest.mpd"
            ]
        );
    }

    #[test]
    fn with_no_keys_every_string_is_looked_at_and_only_streams_are_kept() {
        let body = r#"[ { "a": "https://cdn.example/v/one.mp4" },
                        { "b": ["https://cdn.example/v/two.webm", "https://cdn.example/page.html",
                                "see https://cdn.example/x.mp4", "ftp://cdn.example/f.mp4", 7, null] },
                        { "c": "https://cdn.example/v/one.mp4" } ]"#;
        assert_eq!(
            addresses_in(body, &[]),
            vec![
                "https://cdn.example/v/one.mp4",
                "https://cdn.example/v/two.webm"
            ]
        );
    }

    #[test]
    fn a_whole_subtree_under_a_key_is_read() {
        let body = r#"{ "sources": { "hd": [ { "file": "https://cdn.example/hd.mp4" } ],
                                     "sd": "https://cdn.example/sd.mp4" },
                         "ads": { "file": "https://cdn.example/ad.mp4" } }"#;
        assert_eq!(
            addresses_in(body, &keys(&["sources"])),
            vec!["https://cdn.example/hd.mp4", "https://cdn.example/sd.mp4"]
        );
    }

    #[test]
    fn what_is_never_caught_is_not_taken_from_an_answer() {
        let body =
            r#"{ "u": "https://ad.doubleclick.net/promo.mp4", "v": "https://cdn.example/ok.mp4" }"#;
        assert_eq!(addresses_in(body, &[]), vec!["https://cdn.example/ok.mp4"]);
    }

    #[test]
    fn a_body_that_is_not_json_or_is_too_large_gives_nothing() {
        assert!(addresses_in("<html>https://cdn.example/a.mp4</html>", &[]).is_empty());
        assert!(addresses_in("var u = 'https://cdn.example/a.mp4';", &[]).is_empty());
        assert!(addresses_in("", &[]).is_empty());
        let big = format!(
            r#"{{ "u": "https://cdn.example/a.mp4", "pad": "{}" }}"#,
            "x".repeat(MAX_ANSWER_BYTES)
        );
        assert!(addresses_in(&big, &[]).is_empty());
    }

    #[test]
    fn a_deep_or_wide_answer_is_cut_and_not_followed_forever() {
        let mut deep = String::from(r#""https://cdn.example/a.mp4""#);
        for _ in 0..100 {
            deep = format!("[{deep}]");
        }
        // serde_json refuses nesting past its own limit, and the walk stops at its own; either
        // way the address at the bottom is not found.
        assert!(addresses_in(&deep, &[]).is_empty());
        let many: Vec<String> = (0..200)
            .map(|n| format!(r#""https://cdn.example/{n}.mp4""#))
            .collect();
        let found = addresses_in(&format!("[{}]", many.join(",")), &[]);
        assert_eq!(found.len(), MOST_ADDRESSES);
    }

    #[test]
    fn an_address_that_is_not_media_is_not_one() {
        for text in [
            "https://cdn.example/",
            "https://cdn.example/a.mp4x",
            "https://cdn.example/mp4",
            "/relative/a.mp4",
            "javascript:alert(1)//a.mp4",
            "data:video/mp4;base64,AAAA",
        ] {
            assert_eq!(stream_address(text), None, "{text}");
        }
        assert_eq!(
            stream_address(" https://cdn.example/a/b.M3U8 ").as_deref(),
            Some("https://cdn.example/a/b.M3U8")
        );
    }
}
