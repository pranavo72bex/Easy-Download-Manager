// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The video (or sound) a web page carries, found by reading the page, for any site (the
//! maintainer, 2026-11-21: "sending url from any application should download by the dm …
//! general rather than tik tok specific … the core of our download engine").
//!
//! A link shared from an app (a short video, a post, a news clip) is a page, not a file. Where
//! such a page answers, the engine reads it here for the media it plays, in the order sites
//! publish it, most reliable first:
//!
//! 1. JSON-LD (`<script type="application/ld+json">`): a `VideoObject` or `AudioObject`'s
//!    `contentUrl`, as search engines read it.
//! 2. Open Graph and Twitter cards: `og:video:secure_url`, `og:video:url`, `og:video`,
//!    `twitter:player:stream`, `og:audio`, where they name a file (not a player's page).
//! 3. The page's own `<video src>`, `<source src>` and `<audio src>`.
//! 4. A player's data in the page's scripts: the values of keys players name their file by
//!    (`playAddr`, `downloadAddr`, `contentUrl`, `videoUrl`, `playUrl`, `hlsUrl`, …).
//! 5. Last, a quoted address in the page that ends as a media file does (`.mp4`, `.m3u8`, …).
//!
//! The page is fetched as a browser fetches it (redirects followed, a share address's included,
//! the cookies each step sets kept), and the media is then fetched as the page's player fetches
//! it: with those cookies, sent to the media's server alone, and the page as the referrer. Its
//! name is the page's title (`og:title`, the JSON-LD's `name`, the player data's caption or
//! title, else `<title>`). A page with none of these is left to the engine as before: asked
//! about as a web page (contract row B40). Media found is fetched by the engine as any address
//! is, so an HLS playlist is recorded, an MP4 segmented, a refusal said; it is not read as a
//! page again.

use crate::download::{Finished, Options, Update};
use crate::Error;

/// The most of a page read for its media; a short-video page was about 520 KB on 2026-11-21.
const PAGE_MOST: usize = 4 * 1024 * 1024;

/// The most redirects followed to the page.
const REDIRECTS_MOST: usize = 8;

/// The endings of a media file's address.
const MEDIA_ENDINGS: [&str; 9] = [
    ".mp4", ".m4v", ".mov", ".webm", ".mkv", ".m3u8", ".mpd", ".mp3", ".m4a",
];

/// The keys players name their media file by, in their page data.
const PLAYER_KEYS: [&str; 14] = [
    "playAddr",
    "downloadAddr",
    "contentUrl",
    "videoUrl",
    "video_url",
    "playUrl",
    "play_url",
    "mp4Url",
    "mp4_url",
    "hlsUrl",
    "hls_url",
    "streamUrl",
    "stream_url",
    "playbackUrl",
];

/// The keys a page's data names its post by, for the file's name.
const TITLE_KEYS: [&str; 4] = ["title", "desc", "caption", "description"];

tokio::task_local! {
    /// Set while media found on a page is fetched: what it answers is the file, never read as
    /// a page again (a player's file that answers with HTML is said, not followed round).
    static FROM_PAGE: ();
}

/// Whether this download is media a page named, already read from its page.
pub(crate) fn reading_media() -> bool {
    FROM_PAGE.try_with(|_| ()).is_ok()
}

/// What a page carries: its media's address, and the title it gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The media's address, absolute.
    pub media: String,
    /// The page's title for it, where it gives one.
    pub title: Option<String>,
}

/// The media page `html` at `page` carries, by the order the module's comment gives.
pub fn find(html: &str, page: &str) -> Option<Found> {
    let base = url::Url::parse(page).ok();
    let absolute = |raw: &str| -> Option<String> {
        let raw = unescaped(raw.trim());
        let joined = match &base {
            Some(base) => base.join(&raw).ok()?,
            None => url::Url::parse(&raw).ok()?,
        };
        matches!(joined.scheme(), "https" | "http").then(|| joined.to_string())
    };
    let published = json_ld_media(html)
        .and_then(|m| absolute(&m))
        .or_else(|| meta_media(html).and_then(|m| absolute(&m)))
        .or_else(|| tag_media(html).and_then(|m| absolute(&m)));
    if let Some(media) = published {
        return Some(Found {
            media,
            title: title_of(html),
        });
    }
    // A player's data: its caption is the one nearest before the media in that data (a page's
    // data holds other titles too, its menus' among them).
    if let Some((at, media)) = player_media(html).and_then(|(at, m)| Some((at, absolute(&m)?))) {
        let title = meta(html, &["og:title", "twitter:title"])
            .and_then(clean_title)
            .or_else(|| title_before(html, at))
            .or_else(|| title_of(html));
        return Some(Found { media, title });
    }
    let media = quoted_media(html).and_then(|m| absolute(&m))?;
    Some(Found {
        media,
        title: title_of(html),
    })
}

/// JSON's `\/` and `/` escapes, and HTML's `&amp;`, undone in an address.
fn unescaped(raw: &str) -> String {
    raw.replace("\\u002F", "/")
        .replace("\\u002f", "/")
        .replace("\\u0026", "&")
        .replace("\\/", "/")
        .replace("&amp;", "&")
}

/// Whether `address` ends as a media file does (its path, its query left aside).
fn names_media(address: &str) -> bool {
    let path = address.split(['?', '#']).next().unwrap_or(address);
    let path = path.to_ascii_lowercase();
    MEDIA_ENDINGS.iter().any(|ending| path.ends_with(ending))
}

/// Every `<tag …>` in `html` whose name is `name`, as its attribute text.
fn tags<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{name}");
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = lower.get(from..).and_then(|rest| rest.find(&open)) {
        let start = from + at;
        let after = start + open.len();
        // The name whole: `<video` and not `<videos`.
        let next = lower.as_bytes().get(after).copied().unwrap_or(b'>');
        let Some(end) = lower.get(start..).and_then(|rest| rest.find('>')) else {
            break;
        };
        if next.is_ascii_whitespace() || next == b'>' || next == b'/' {
            if let Some(tag) = html.get(after..start + end) {
                found.push(tag);
            }
        }
        from = start + end + 1;
    }
    found
}

/// The value of attribute `name` in a tag's attribute text.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower.get(from..).and_then(|rest| rest.find(name)) {
        let start = from + at;
        let before_ok = start == 0
            || lower
                .as_bytes()
                .get(start - 1)
                .is_some_and(|b| b.is_ascii_whitespace());
        let rest = tag.get(start + name.len()..)?.trim_start();
        if before_ok {
            if let Some(value) = rest.strip_prefix('=') {
                let value = value.trim_start();
                let (quote, body) = match value.chars().next()? {
                    q @ ('"' | '\'') => (Some(q), value.get(1..)?),
                    _ => (None, value),
                };
                let end = match quote {
                    Some(q) => body.find(q)?,
                    None => body
                        .find(|c: char| c.is_whitespace() || c == '>')
                        .unwrap_or(body.len()),
                };
                return body.get(..end).map(|v| v.replace("&amp;", "&"));
            }
        }
        from = start + name.len();
    }
    None
}

/// The contents of every `<script>` whose type is `kind`.
fn scripts<'a>(html: &'a str, kind: &str) -> Vec<&'a str> {
    let lower = html.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = lower.get(from..).and_then(|rest| rest.find("<script")) {
        let start = from + at;
        let Some(open_end) = lower.get(start..).and_then(|r| r.find('>')) else {
            break;
        };
        let open_end = start + open_end;
        let Some(close) = lower.get(open_end..).and_then(|r| r.find("</script")) else {
            break;
        };
        let close = open_end + close;
        let tag = html.get(start..open_end).unwrap_or_default();
        if attribute(tag, "type").is_some_and(|t| t.eq_ignore_ascii_case(kind)) {
            if let Some(body) = html.get(open_end + 1..close) {
                found.push(body);
            }
        }
        from = close;
    }
    found
}

/// A JSON-LD `VideoObject` or `AudioObject`'s `contentUrl`, anywhere in the page's JSON-LD.
fn json_ld_media(html: &str) -> Option<String> {
    fn search(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                let kind = map.get("@type").map(|t| t.to_string()).unwrap_or_default();
                if kind.contains("VideoObject") || kind.contains("AudioObject") {
                    if let Some(url) = map.get("contentUrl").and_then(|u| u.as_str()) {
                        return Some(url.to_string());
                    }
                }
                map.values().find_map(search)
            }
            serde_json::Value::Array(items) => items.iter().find_map(search),
            _ => None,
        }
    }
    scripts(html, "application/ld+json")
        .into_iter()
        .filter_map(|body| serde_json::from_str::<serde_json::Value>(body.trim()).ok())
        .find_map(|value| search(&value))
}

/// The `content` of the first `<meta>` whose `property` or `name` is one of `keys`, in the
/// keys' order.
fn meta(html: &str, keys: &[&str]) -> Option<String> {
    let metas = tags(html, "meta");
    keys.iter().find_map(|key| {
        metas.iter().find_map(|tag| {
            let named = attribute(tag, "property")
                .or_else(|| attribute(tag, "name"))
                .is_some_and(|n| n.eq_ignore_ascii_case(key));
            named
                .then(|| attribute(tag, "content"))
                .flatten()
                .filter(|c| !c.trim().is_empty())
        })
    })
}

/// Open Graph's or Twitter's media, where it names a file and not a player's page.
fn meta_media(html: &str) -> Option<String> {
    let video_type = meta(html, &["og:video:type"]).unwrap_or_default();
    let typed_file = video_type.starts_with("video/") || video_type.contains("mpegurl");
    meta(
        html,
        &[
            "og:video:secure_url",
            "og:video:url",
            "og:video",
            "twitter:player:stream",
            "og:audio:secure_url",
            "og:audio",
        ],
    )
    .filter(|m| typed_file || names_media(m))
}

/// The page's own `<video src>`, `<source src>` (of a media type) or `<audio src>`.
fn tag_media(html: &str) -> Option<String> {
    let direct = ["video", "audio"]
        .iter()
        .flat_map(|name| tags(html, name))
        .find_map(|tag| attribute(tag, "src"))
        .filter(|src| !src.starts_with("blob:"));
    direct.or_else(|| {
        tags(html, "source").into_iter().find_map(|tag| {
            let kind = attribute(tag, "type")
                .unwrap_or_default()
                .to_ascii_lowercase();
            let src = attribute(tag, "src")?;
            let media =
                kind.starts_with("video/") || kind.starts_with("audio/") || names_media(&src);
            (media && !src.starts_with("blob:")).then_some(src)
        })
    })
}

/// The quoted string that follows `"key":` at `at` in `text`.
fn quoted_after(text: &str, at: usize) -> Option<String> {
    let rest = text.get(at..)?.trim_start().strip_prefix(':')?.trim_start();
    let body = rest.strip_prefix('"')?;
    let mut value = String::new();
    let mut escaped = false;
    for c in body.chars() {
        match (escaped, c) {
            (true, c) => {
                value.push('\\');
                value.push(c);
                escaped = false;
            }
            (false, '\\') => escaped = true,
            (false, '"') => return Some(value),
            (false, c) => value.push(c),
        }
    }
    None
}

/// Every value of `"key": "…"` in the page's text, in order.
fn values_of(html: &str, key: &str) -> Vec<String> {
    values_at(html, key).into_iter().map(|(_, v)| v).collect()
}

/// [`values_of`], each with where its key stands in the page.
fn values_at(html: &str, key: &str) -> Vec<(usize, String)> {
    let needle = format!("\"{key}\"");
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = html.get(from..).and_then(|rest| rest.find(&needle)) {
        let end = from + at + needle.len();
        if let Some(value) = quoted_after(html, end) {
            found.push((from + at, value));
        }
        from = end;
    }
    found
}

/// A player's media in the page's scripts, by the keys players name it by, and where it stands.
fn player_media(html: &str) -> Option<(usize, String)> {
    PLAYER_KEYS.iter().find_map(|key| {
        values_at(html, key).into_iter().find(|(_, value)| {
            let value = unescaped(value);
            value.starts_with("https://") || value.starts_with("http://") || value.starts_with("//")
        })
    })
}

/// How far before a player's media its caption is looked for.
const CAPTION_WITHIN: usize = 20_000;

/// The caption or title nearest before `at` in the page's data, within [`CAPTION_WITHIN`].
fn title_before(html: &str, at: usize) -> Option<String> {
    let from = at.saturating_sub(CAPTION_WITHIN);
    TITLE_KEYS
        .iter()
        .flat_map(|key| values_at(html, key))
        .filter(|(place, _)| *place < at && *place >= from)
        .filter_map(|(place, value)| Some((place, clean_title(value)?)))
        .filter(|(_, t)| t.chars().count() >= 3 && !t.starts_with("http"))
        .max_by_key(|(place, _)| *place)
        .map(|(_, title)| title)
}

/// A title as it is shown: escapes undone, its spaces one apart; `None` where it is empty.
fn clean_title(raw: String) -> Option<String> {
    let text = unescaped(&raw)
        .replace("\\n", " ")
        .replace("&#39;", "'")
        .replace("&quot;", "\"");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// A quoted address anywhere in the page that ends as a media file does.
fn quoted_media(html: &str) -> Option<String> {
    let text = unescaped(html);
    let mut from = 0;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find("https://")) {
        let start = from + at;
        let end = text
            .get(start..)
            .and_then(|rest| rest.find(['"', '\'', ' ', '<', '>', '\\']))
            .map_or(text.len(), |e| start + e);
        let candidate = text.get(start..end).unwrap_or_default();
        if names_media(candidate) {
            return Some(candidate.to_string());
        }
        from = end.max(start + 1);
    }
    None
}

/// The page's title for its media: `og:title`, a JSON-LD `name`, the player data's title or
/// caption, else `<title>`.
fn title_of(html: &str) -> Option<String> {
    let clean = clean_title;
    if let Some(title) = meta(html, &["og:title", "twitter:title"]).and_then(clean) {
        return Some(title);
    }
    let from_ld = scripts(html, "application/ld+json")
        .into_iter()
        .filter_map(|body| serde_json::from_str::<serde_json::Value>(body.trim()).ok())
        .find_map(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .and_then(clean);
    if from_ld.is_some() {
        return from_ld;
    }
    let from_data = TITLE_KEYS.iter().find_map(|key| {
        values_of(html, key)
            .into_iter()
            .filter_map(clean)
            .find(|t| t.chars().count() >= 3 && !t.starts_with("http"))
    });
    if from_data.is_some() {
        return from_data;
    }
    let title = tags(html, "title").first().and_then(|_| {
        let lower = html.to_ascii_lowercase();
        let open = lower.find("<title")?;
        let start = open + lower.get(open..)?.find('>')? + 1;
        let end = start + lower.get(start..)?.find("</title")?;
        html.get(start..end).map(str::to_string)
    });
    title.and_then(clean)
}

/// The file's name: the title, cut to a length a file system takes, with the media's own ending
/// (`.mp4` where its address names none).
pub fn file_name(found: &Found) -> String {
    let title: String = found
        .title
        .clone()
        .unwrap_or_else(|| "video".into())
        .chars()
        .filter(|c| !"/\\:*?\"<>|".contains(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        title = "video".into();
    }
    if title.chars().count() > 100 {
        title = title
            .chars()
            .take(100)
            .collect::<String>()
            .trim_end()
            .to_string();
    }
    let path = found.media.split(['?', '#']).next().unwrap_or(&found.media);
    let lower = path.to_ascii_lowercase();
    let ending = MEDIA_ENDINGS
        .iter()
        .find(|e| lower.ends_with(*e))
        .map_or(".mp4", |e| match *e {
            // A playlist's recording is saved as an MP4.
            ".m3u8" | ".mpd" => ".mp4",
            e => e,
        });
    format!("{title}{ending}")
}

/// The `Cookie` header the cookies a response set make: each `name=value`.
fn cookies_set(headers: &reqwest::header::HeaderMap) -> String {
    headers
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|cookie| cookie.split(';').next())
        .map(str::trim)
        .filter(|pair| pair.contains('=') && !pair.starts_with('='))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The page at `url`, fetched as a browser fetches it: its HTML, the cookies set on the way,
/// and the page's own address. `None` where it answers with no page (a file, a refusal).
async fn page_at(
    client: &reqwest::Client,
    url: &str,
) -> Result<Option<(String, String, String)>, Error> {
    let mut at = url.trim().to_string();
    let mut jar: Vec<String> = Vec::new();
    for _ in 0..REDIRECTS_MOST {
        let mut answer = client
            .get(&at)
            .header(reqwest::header::USER_AGENT, dm_net::agents::BROWSER_AGENT)
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,*/*;q=0.8",
            )
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .send()
            .await
            .map_err(|e| Error::Internal(format!("the page did not answer: {e}")))?;
        let set = cookies_set(answer.headers());
        if !set.is_empty() {
            jar.push(set);
        }
        if answer.status().is_redirection() {
            let Some(next) = answer
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|l| l.to_str().ok())
                .and_then(|l| answer.url().join(l).ok())
            else {
                return Ok(None);
            };
            // Never from a secure address to an insecure one.
            if answer.url().scheme() == "https" && next.scheme() != "https" {
                return Ok(None);
            }
            at = next.to_string();
            continue;
        }
        let html = crate::videopage::is_page(
            answer
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|t| t.to_str().ok()),
        );
        if !answer.status().is_success() || !html {
            return Ok(None);
        }
        let page = answer.url().to_string();
        let mut body = Vec::new();
        while let Some(chunk) = answer
            .chunk()
            .await
            .map_err(|e| Error::Internal(format!("the page could not be read: {e}")))?
        {
            body.extend_from_slice(&chunk);
            if body.len() > PAGE_MOST {
                break;
            }
        }
        return Ok(Some((
            String::from_utf8_lossy(&body).into_owned(),
            jar.join("; "),
            page,
        )));
    }
    Ok(None)
}

/// The media the page at `url` carries, downloaded as its player fetches it; `None` where the
/// page carries none dm can find (the engine then goes on as before).
pub(crate) async fn download_from(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Option<Finished>, Error> {
    if reading_media() {
        return Ok(None);
    }
    let Some((html, cookies, page)) = page_at(client, url).await? else {
        return Ok(None);
    };
    let Some(found) = find(&html, &page) else {
        return Ok(None);
    };
    // The page's own address again is no media: the page would be read round.
    if found.media.split('#').next() == page.split('#').next() {
        return Ok(None);
    }
    let sign_in = (!cookies.is_empty())
        .then(|| crate::cookies::SignIn::new(&found.media, &cookies))
        .flatten();
    let options = Options {
        referrer: Some(page),
        sign_in: options.sign_in.clone().or(sign_in),
        save_name: options
            .save_name
            .clone()
            .or_else(|| Some(file_name(&found))),
        ..options.clone()
    };
    FROM_PAGE
        .scope(
            (),
            crate::download::once_more(client, &found.media, &options, report),
        )
        .await
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "https://site.example/watch/42";

    #[test]
    fn json_ld_names_the_video_first() {
        let html = r#"<html><head><title>Site</title>
            <script type="application/ld+json">{"@context":"https://schema.org","@type":"VideoObject","name":"A talk","contentUrl":"https://cdn.example/talk.mp4"}</script>
            <meta property="og:video" content="https://cdn.example/other.mp4"></head></html>"#;
        let found = find(html, PAGE).expect("found");
        assert_eq!(found.media, "https://cdn.example/talk.mp4");
        assert_eq!(found.title.as_deref(), Some("A talk"));
        assert_eq!(file_name(&found), "A talk.mp4");
    }

    #[test]
    fn open_graph_names_a_file_but_not_a_players_page() {
        let file = r#"<meta property="og:title" content="Clip &amp; more"><meta property="og:video:secure_url" content="https://cdn.example/c.mp4?sig=1&amp;x=2">"#;
        let found = find(file, PAGE).expect("found");
        assert_eq!(found.media, "https://cdn.example/c.mp4?sig=1&x=2");
        assert_eq!(found.title.as_deref(), Some("Clip & more"));
        let player = r#"<meta property="og:video" content="https://site.example/embed/42">"#;
        assert_eq!(find(player, PAGE), None);
        let typed = r#"<meta property="og:video" content="https://cdn.example/play/42"><meta property="og:video:type" content="video/mp4">"#;
        assert_eq!(
            find(typed, PAGE).map(|f| f.media),
            Some("https://cdn.example/play/42".to_string())
        );
    }

    #[test]
    fn a_video_tag_and_its_sources_are_read_relative_to_the_page() {
        let html = r#"<video controls><source src="/media/clip.webm" type="video/webm"></video>"#;
        assert_eq!(
            find(html, PAGE).map(|f| f.media),
            Some("https://site.example/media/clip.webm".to_string())
        );
        let direct = r#"<video src="https://cdn.example/v.mp4" autoplay></video>"#;
        assert_eq!(
            find(direct, PAGE).map(|f| f.media),
            Some("https://cdn.example/v.mp4".to_string())
        );
        assert_eq!(
            find(r#"<video src="blob:https://site/1"></video>"#, PAGE),
            None
        );
    }

    #[test]
    fn a_players_data_in_a_script_is_read_with_its_caption() {
        // As a short-video site's page holds it: escaped slashes, a caption under "desc".
        let html = r#"<title>Site - Make Your Day</title><script id="data" type="application/json">{"nav":{"title":"Company"},"item":{"id":"77","desc":"A cat  on a skateboard","video":{"playAddr":"https://v16.cdn.example/video/abc?a=1&b=2"}}}</script>"#;
        let found = find(html, PAGE).expect("found");
        assert_eq!(found.media, "https://v16.cdn.example/video/abc?a=1&b=2");
        assert_eq!(found.title.as_deref(), Some("A cat on a skateboard"));
        assert_eq!(file_name(&found), "A cat on a skateboard.mp4");
    }

    #[test]
    fn a_quoted_media_address_is_the_last_resort() {
        let html = r#"<script>var player = setup({file: "https://cdn.example/hls/master.m3u8"});</script><title>News clip</title>"#;
        let found = find(html, PAGE).expect("found");
        assert_eq!(found.media, "https://cdn.example/hls/master.m3u8");
        assert_eq!(file_name(&found), "News clip.mp4");
    }

    #[test]
    fn a_page_with_no_media_names_none() {
        assert_eq!(
            find("<html><title>Just text</title><p>hello</p></html>", PAGE),
            None
        );
        assert_eq!(
            find(r#"<a href="https://site.example/page.html">x</a>"#, PAGE),
            None
        );
        // Not a web address.
        assert_eq!(find(r#"{"playAddr":"javascript:alert(1)"}"#, PAGE), None);
    }

    #[test]
    fn a_long_title_is_cut_and_marks_a_file_system_refuses_are_left_out() {
        let found = Found {
            media: "https://cdn.example/a.mp3".into(),
            title: Some(format!("{}/a:b*c?\nnext", "x".repeat(200))),
        };
        let name = file_name(&found);
        assert!(name.ends_with(".mp3"), "{name}");
        assert!(!name.contains(['/', ':', '*', '?', '\n']), "{name}");
        assert!(name.chars().count() <= 104, "{name}");
    }

    #[test]
    fn the_cookies_set_make_a_cookie_header() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            "token=abc; Path=/; Secure".parse().unwrap(),
        );
        headers.append(
            reqwest::header::SET_COOKIE,
            "id=1%7Cx; Max-Age=1".parse().unwrap(),
        );
        assert_eq!(cookies_set(&headers), "token=abc; id=1%7Cx");
    }
}
