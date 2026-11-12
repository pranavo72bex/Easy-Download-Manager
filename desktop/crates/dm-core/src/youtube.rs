// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A YouTube watch page read by dm itself, with no video helper.
//!
//! What a browser-like client does for such a page, watched request by request
//! (`docs/decisions.md`, "How YouTube is got today"): the watch page as a
//! browser, for the visitor id, the player script's address and its
//! signature timestamp; the player API as a TV client, which answers with
//! plain `https` addresses for every picture and sound; the player script;
//! and, as reference A does it, the pieces of that script that answer its
//! checks cut out (`player`) and run in the computer's own JavaScript engine
//! (`crate::javascript`), which turns each address's `n` value into the one
//! the server accepts and deciphers a locked stream's signature. Nothing is
//! fetched for that or shipped with dm.
//! The result is written as the JSON a reader of a page prints, so
//! `videopage::download_page` fetches and joins the streams from it. Anything this
//! cannot read (a video wanting a sign-in, a live stream, a changed client) is
//! failed, with the reason said in words.
//!
//! Test knob: `DM_YOUTUBE_HOST` names a stand-in for `www.youtube.com` (a
//! test's server).

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde_json::{json, Value};

mod itags;
mod player;
use crate::stream::Quality;
use crate::Error;

/// The TV client this reads with today: its answers carry plain
/// addresses and want no proof-of-origin token.
const TV_CLIENT_NAME: &str = "TVHTML5";
const TV_CLIENT_VERSION: &str = "5.20260707";
const TV_CLIENT_ID: &str = "7";
const TV_AGENT: &str = "Mozilla/5.0 (SMART-TV; Linux; Tizen 2.4.0) AppleWebKit/538.1 (KHTML, like Gecko) Version/2.4.0 TV Safari/538.1";
/// The browser the watch page is asked as.
const PAGE_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";
/// The most one request for a stream asks for, as a player does for
/// these addresses (`http_chunk_size`): the server slows a response past it.
const CHUNK: u64 = 10 << 20;
/// How long the whole reading may take.
const WITHIN: std::time::Duration = std::time::Duration::from_secs(60);
/// How long the program that answers the player's checks may run.
const SOLVING: std::time::Duration = std::time::Duration::from_secs(30);

/// The moment a video address starts at, in whole seconds: `t=` or
/// `start=` in its query or its `#t=` fragment, as YouTube writes them
/// ("34", "34s", "1m30s", "1h2m3s"). `None` without one, or at 0.
pub fn start_of(url: &str) -> Option<u64> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    let fragment = parsed.fragment().unwrap_or_default().to_string();
    let from_fragment = url::form_urlencoded::parse(fragment.as_bytes())
        .find(|(k, _)| k == "t")
        .map(|(_, v)| v.into_owned());
    let given = parsed
        .query_pairs()
        .find(|(k, _)| k == "t" || k == "start")
        .map(|(_, v)| v.into_owned())
        .or(from_fragment)?;
    let seconds = crate::clip::Span::seconds(&given)?;
    (seconds >= 1.0 && seconds.is_finite()).then_some(seconds as u64)
}

/// A playlist's id in a playlist address (`/playlist?list=…`). A watch
/// address with a list beside its video is that video's: `None`.
pub fn playlist_id(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("m.").unwrap_or(host);
    if host != "youtube.com" || parsed.path() != "/playlist" {
        return None;
    }
    parsed
        .query_pairs()
        .find(|(k, _)| k == "list")
        .map(|(_, v)| v.into_owned())
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// One video of a playlist, as its page lists it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlaylistEntry {
    /// The video's id.
    pub id: String,
    /// The video's title.
    pub title: String,
    /// How long it plays, where the list says.
    pub seconds: Option<u64>,
    /// Its own page, for another site's playlist; a YouTube video's is made
    /// from its id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

impl PlaylistEntry {
    /// The video's page: its own, or YouTube's watch address.
    pub fn url(&self) -> String {
        match &self.page {
            Some(page) => page.clone(),
            None => format!("https://www.youtube.com/watch?v={}", self.id),
        }
    }
}

/// A playlist as its page lists it: its title and its videos, in order.
/// `cut_short` says the list went on past what was read (a thousand
/// videos, or a continuation YouTube would not answer).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    /// The playlist's id.
    pub id: String,
    /// The playlist's title.
    pub title: String,
    /// Its videos, in the playlist's order.
    pub entries: Vec<PlaylistEntry>,
    /// Whether the list went on past what was read.
    pub cut_short: bool,
}

/// The most videos read of one playlist: ten pages of a hundred.
const PLAYLIST_MOST: usize = 1000;

/// A playlist page read by dm itself: the page's own data lists its first
/// hundred videos, and the browse API the rest, a hundred at a time, as
/// the page itself asks for them.
pub async fn read_playlist(
    client: &reqwest::Client,
    url: &str,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Playlist, Error> {
    let id =
        playlist_id(url).ok_or_else(|| Error::Internal("not a YouTube playlist address".into()))?;
    tokio::time::timeout(WITHIN, read_playlist_within(client, &id, sign_in))
        .await
        .map_err(|_| Error::Internal("the playlist took too long to read".into()))?
}

async fn read_playlist_within(
    client: &reqwest::Client,
    id: &str,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Playlist, Error> {
    let origin = origin();
    let address = format!("{origin}/playlist?list={id}&hl=en");
    let mut cookie = String::from("PREF=hl=en&tz=UTC; SOCS=CAI");
    if let Some(theirs) = crate::cookies::header_with(sign_in, &address) {
        cookie = format!("{theirs}; {cookie}");
    }
    let page = crate::redirects::send(
        client
            .get(&address)
            .header(reqwest::header::USER_AGENT, PAGE_AGENT)
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(reqwest::header::COOKIE, &cookie),
    )
    .await?;
    let page = text_capped(Error::refused_or(page).await?, PAGE_MOST).await?;
    let data = initial_data(&page).ok_or_else(|| {
        Error::Internal(
            "the playlist page holds no list (private, removed, or a sign-in wanted)".into(),
        )
    })?;
    let mut playlist = Playlist {
        id: id.to_string(),
        title: playlist_title(&data).unwrap_or_else(|| format!("Playlist {id}")),
        ..Playlist::default()
    };
    let mut token = gather(&data, &mut playlist.entries);
    if playlist.entries.is_empty() {
        return Err(Error::Internal(
            "the playlist page lists no videos (private, removed, or a sign-in wanted)".into(),
        ));
    }
    // The page's own client version, for the browse API; a stand-in gives none.
    let version = client_version(&page).unwrap_or_else(|| "2.20261020.00.00".to_string());
    while let Some(next) = token.take() {
        if playlist.entries.len() >= PLAYLIST_MOST {
            playlist.cut_short = true;
            break;
        }
        let body = json!({
            "context": {"client": {"clientName": "WEB", "clientVersion": version, "hl": "en", "gl": "US"}},
            "continuation": next,
        });
        let answer = crate::redirects::send(
            client
                .post(format!("{origin}/youtubei/v1/browse?prettyPrint=false"))
                .header(reqwest::header::USER_AGENT, PAGE_AGENT)
                .header(reqwest::header::COOKIE, &cookie)
                .header("X-YouTube-Client-Name", "1")
                .header("X-YouTube-Client-Version", &version)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string()),
        )
        .await;
        let Ok(answer) = answer else {
            playlist.cut_short = true;
            break;
        };
        let Ok(text) = text_capped(answer, PAGE_MOST).await else {
            playlist.cut_short = true;
            break;
        };
        let Ok(more) = serde_json::from_str::<Value>(&text) else {
            playlist.cut_short = true;
            break;
        };
        let before = playlist.entries.len();
        token = gather(&more, &mut playlist.entries);
        if playlist.entries.len() == before {
            // A page that adds nothing would loop for ever.
            break;
        }
    }
    Ok(playlist)
}

/// The most of a page or an API answer that is read: YouTube's are a
/// megabyte or two; a body that goes on past this is not what was asked.
const PAGE_MOST: usize = 16 << 20;

/// The response's text, read no further than `most` bytes.
async fn text_capped(mut response: reqwest::Response, most: usize) -> Result<String, Error> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > most {
            return Err(Error::Internal(
                "the page is larger than any YouTube page".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The page's `ytInitialData`.
fn initial_data(html: &str) -> Option<Value> {
    const MARK: &str = "ytInitialData = ";
    let at = html.find(MARK)?;
    let object = json_object_at(html[at + MARK.len()..].trim_start())?;
    serde_json::from_str(object).ok()
}

/// The page's innertube client version (`ytcfg.set`), where it says.
fn client_version(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(at) = rest.find("ytcfg.set(") {
        let after = &rest[at + "ytcfg.set(".len()..];
        if let Some(object) = json_object_at(after.trim_start()) {
            if let Ok(cfg) = serde_json::from_str::<Value>(object) {
                if let Some(version) = cfg
                    .get("INNERTUBE_CLIENT_VERSION")
                    .or_else(|| cfg.pointer("/INNERTUBE_CONTEXT/client/clientVersion"))
                    .and_then(Value::as_str)
                {
                    return Some(version.to_string());
                }
            }
        }
        rest = after;
    }
    None
}

/// The playlist's title, from wherever the page's data keeps it.
fn playlist_title(data: &Value) -> Option<String> {
    let text = |v: &Value| -> Option<String> {
        v.as_str()
            .map(str::to_string)
            .or_else(|| v.get("simpleText")?.as_str().map(str::to_string))
            .or_else(|| {
                let runs = v.get("runs")?.as_array()?;
                let joined: String = runs
                    .iter()
                    .filter_map(|r| r.get("text")?.as_str())
                    .collect();
                (!joined.is_empty()).then_some(joined)
            })
            .or_else(|| text_of(v.get("content")?))
    };
    fn text_of(v: &Value) -> Option<String> {
        v.as_str().map(str::to_string)
    }
    [
        "/metadata/playlistMetadataRenderer/title",
        "/header/playlistHeaderRenderer/title",
        "/header/pageHeaderRenderer/pageTitle",
        "/microformat/microformatDataRenderer/title",
    ]
    .iter()
    .find_map(|path| data.pointer(path).and_then(text))
    .map(|t| t.trim().to_string())
    .filter(|t| !t.is_empty())
}

/// Every video the data lists, in order, added to `entries` (one a video,
/// however often the data names it), and the token for the next page,
/// where the data has one.
fn gather(data: &Value, entries: &mut Vec<PlaylistEntry>) -> Option<String> {
    let mut token = None;
    let mut seen: std::collections::HashSet<String> =
        entries.iter().map(|e| e.id.clone()).collect();
    // A video's id as YouTube writes them: eleven of these characters.
    let sound_id = |id: &str| {
        id.len() == 11
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    };
    // A video that cannot be played is not offered: private, deleted.
    let unplayable = |title: &str, playable: Option<bool>| {
        playable == Some(false) || matches!(title, "[Private video]" | "[Deleted video]")
    };
    walk(data, &mut |key, value| {
        match key {
            // The page's newer shape (2026): each video a "lockup", its id
            // under `contentId`, its title in its metadata, its length the
            // badge on its picture ("12:15").
            "lockupViewModel" => {
                let id = value.get("contentId").and_then(Value::as_str).unwrap_or("");
                let video = value
                    .get("contentType")
                    .and_then(Value::as_str)
                    .is_none_or(|t| t == "LOCKUP_CONTENT_TYPE_VIDEO");
                if !sound_id(id) || !video || seen.contains(id) {
                    return;
                }
                let title = value
                    .pointer("/metadata/lockupMetadataViewModel/title/content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let mut badge = None;
                if let Some(image) = value.get("contentImage") {
                    walk(image, &mut |k, v| {
                        if k == "thumbnailBadgeViewModel" && badge.is_none() {
                            badge = v
                                .get("text")
                                .and_then(Value::as_str)
                                .filter(|t| t.contains(':'))
                                .and_then(crate::clip::Span::seconds);
                        }
                    });
                }
                if unplayable(&title, None) {
                    return;
                }
                seen.insert(id.to_string());
                entries.push(PlaylistEntry {
                    id: id.to_string(),
                    title: if title.is_empty() {
                        id.to_string()
                    } else {
                        title
                    },
                    seconds: badge.map(|s| s as u64),
                    page: None,
                });
            }
            "playlistVideoRenderer" => {
                let id = value.get("videoId").and_then(Value::as_str).unwrap_or("");
                if !sound_id(id) || seen.contains(id) {
                    return;
                }
                if value.get("isPlayable").and_then(Value::as_bool) == Some(false) {
                    return;
                }
                let title = value
                    .pointer("/title/runs/0/text")
                    .or_else(|| value.pointer("/title/simpleText"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let seconds = value
                    .get("lengthSeconds")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
                if unplayable(&title, None) {
                    return;
                }
                seen.insert(id.to_string());
                entries.push(PlaylistEntry {
                    id: id.to_string(),
                    title: if title.is_empty() {
                        id.to_string()
                    } else {
                        title
                    },
                    seconds,
                    page: None,
                });
            }
            // The next page's token, wherever the page's shape puts it.
            "continuationCommand" if token.is_none() => {
                token = value
                    .get("token")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            _ => {}
        }
    });
    token
}

/// Every object in `data`, depth first, with the key it hangs under.
fn walk(data: &Value, seen: &mut dyn FnMut(&str, &Value)) {
    match data {
        Value::Object(map) => {
            for (key, value) in map {
                seen(key, value);
                walk(value, seen);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, seen);
            }
        }
        _ => {}
    }
}

/// The video's id in a watch, short, shorts, embed or live address, on YouTube, its
/// no-cookie host or YouTube Music.
pub fn video_id(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("m.").unwrap_or(host);
    let host = host.strip_prefix("music.").unwrap_or(host);
    let id = match host {
        "youtu.be" => parsed.path().trim_start_matches('/').to_string(),
        "youtube.com" | "youtube-nocookie.com" => {
            if parsed.path() == "/watch" {
                parsed
                    .query_pairs()
                    .find(|(k, _)| k == "v")
                    .map(|(_, v)| v.into_owned())?
            } else {
                let mut parts = parsed.path().trim_start_matches('/').split('/');
                match (parts.next(), parts.next()) {
                    (Some("shorts" | "embed" | "live" | "v"), Some(id)) => id.to_string(),
                    _ => return None,
                }
            }
        }
        _ => return None,
    };
    let id: String = id.chars().take_while(|c| *c != '&' && *c != '?').collect();
    (id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some(id)
}

/// Where YouTube is: its own host, or a test's stand-in.
fn origin() -> String {
    std::env::var("DM_YOUTUBE_HOST")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://www.youtube.com".to_string())
}

/// What the watch page says that the player API needs.
#[derive(Debug, Default, PartialEq)]
struct PageFacts {
    visitor: Option<String>,
    player_js: Option<String>,
    sts: Option<u64>,
}

/// Reads the page's `ytcfg.set({...})` for the visitor id, the player
/// script and the signature timestamp; each may be absent.
fn page_facts(html: &str) -> PageFacts {
    let mut facts = PageFacts::default();
    let mut rest = html;
    while let Some(at) = rest.find("ytcfg.set(") {
        let after = &rest[at + "ytcfg.set(".len()..];
        if let Some(object) = json_object_at(after.trim_start()) {
            if let Ok(cfg) = serde_json::from_str::<Value>(object) {
                let text = |key: &str| cfg.get(key).and_then(Value::as_str).map(str::to_string);
                facts.visitor = facts.visitor.or_else(|| text("VISITOR_DATA")).or_else(|| {
                    cfg.pointer("/INNERTUBE_CONTEXT/client/visitorData")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
                facts.player_js = facts.player_js.or_else(|| text("PLAYER_JS_URL"));
                facts.sts = facts.sts.or_else(|| {
                    cfg.get("STS")
                        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                });
            }
        }
        rest = after;
    }
    facts
}

/// Whether `path` is YouTube's own player script, as its pages name it:
/// `/s/player/<id>/…/<name>.js`, with nothing in it that leaves that path.
fn player_path(path: &str) -> bool {
    path.starts_with("/s/player/")
        && path.ends_with(".js")
        && !path.contains("..")
        && !path.contains("//")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
}

/// The JSON object `text` starts with, by its braces (strings respected).
fn json_object_at(text: &str) -> Option<&str> {
    if !text.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if in_string {
            match c {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => in_string = false,
                _ => escaped = false,
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The signature timestamp in the player script, where the page gave none.
fn sts_in_player(js: &str) -> Option<u64> {
    for key in ["signatureTimestamp:", "sts:"] {
        let mut rest = js;
        while let Some(at) = rest.find(key) {
            let after = &rest[at + key.len()..];
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if digits.len() == 5 {
                return digits.parse().ok();
            }
            rest = after;
        }
    }
    None
}

/// One stream as the player API describes it.
#[derive(Debug, Clone, PartialEq)]
struct Format {
    itag: u64,
    /// The address, with its `n` value still to be solved; or the cipher
    /// it is locked behind.
    url: Option<String>,
    cipher: Option<String>,
    mime: String,
    container: String,
    codec: String,
    video: bool,
    width: u64,
    height: u64,
    bitrate: u64,
    size: Option<u64>,
    fps: u64,
    /// A sound track that is not the video's own (a dub, set by
    /// `set_dubs_aside`), or a loudness or voice-boosted variant: never
    /// chosen, kept in the list.
    secondary: bool,
    /// Which of the video's sound tracks this is, where it has more than one.
    voice: Voice,
    /// Where the stream's first boxes and its index of pieces are
    /// (`initRange`, `indexRange`), for cutting a part of it (`clip`).
    init: Option<(u64, u64)>,
    index: Option<(u64, u64)>,
}

/// Which of a video's sound tracks a stream is. A video made in one
/// language and dubbed into others lists a track per language; which one the
/// player API marks as the one to play (`audioIsDefault`) follows the
/// language dm asks in (English), so a Spanish video's English dub is marked
/// over its own Spanish (issue #32). The address names each track's part
/// (`xtags`: `acont=original`, `dubbed`, `dubbed-auto`, `descriptive`) and
/// the original's track id ends `.4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Voice {
    /// No track named: the video's one sound, or a picture.
    Only,
    /// The language the video was made in.
    Original,
    /// Not named the original, but the one the player would play.
    Default,
    /// Any other: a dub, an automatic dub, a described track.
    Other,
}

impl Voice {
    fn of(f: &Value, acont: Option<&str>) -> Voice {
        let default = f
            .pointer("/audioTrack/audioIsDefault")
            .and_then(Value::as_bool);
        let original_id = f
            .pointer("/audioTrack/id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.ends_with(".4"));
        match acont {
            Some("original") => Voice::Original,
            Some(_) if default == Some(true) => Voice::Default,
            Some(_) => Voice::Other,
            None if f.get("audioTrack").is_none() => Voice::Only,
            None if original_id => Voice::Original,
            None if default != Some(false) => Voice::Default,
            None => Voice::Other,
        }
    }
}

/// Sets every sound track but the video's own aside: the original where one
/// is named, else the one the player would play. Where that would leave no
/// sound at all, nothing is set aside.
fn set_dubs_aside(formats: &mut [Format]) {
    let named_original = formats.iter().any(|f| f.voice == Voice::Original);
    let own = |voice: Voice| match voice {
        Voice::Only | Voice::Original => true,
        Voice::Default => !named_original,
        Voice::Other => false,
    };
    let sound_left = formats
        .iter()
        .any(|f| !f.video && !f.secondary && own(f.voice));
    if !sound_left {
        return;
    }
    for f in formats.iter_mut().filter(|f| !own(f.voice)) {
        f.secondary = true;
    }
}

/// The stream's `xtags` (`acont=original:lang=es:vb=1`) as pairs, from its
/// address or from the address inside the cipher it is locked behind.
fn xtags(url: Option<&str>, cipher: Option<&str>) -> Vec<(String, String)> {
    let inside_cipher = || {
        url::form_urlencoded::parse(cipher?.as_bytes())
            .find(|(k, _)| k == "url")
            .map(|(_, v)| v.into_owned())
    };
    let Some(address) = url.map(str::to_string).or_else(inside_cipher) else {
        return Vec::new();
    };
    let Ok(parsed) = url::Url::parse(&address) else {
        return Vec::new();
    };
    let Some((_, tags)) = parsed.query_pairs().find(|(k, _)| k == "xtags") else {
        return Vec::new();
    };
    tags.split(':')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl Format {
    fn of(f: &Value) -> Option<Format> {
        // A locked (DRM) stream, one streamed as a whole only, a live one.
        if f.get("drmFamilies").is_some()
            || f.get("type").and_then(Value::as_str) == Some("FORMAT_STREAM_TYPE_OTF")
            || f.get("targetDurationSec").is_some()
        {
            return None;
        }
        let mime = f.get("mimeType")?.as_str()?.to_string();
        let (kind, rest) = mime.split_once('/')?;
        let container = rest.split(';').next()?.trim().to_ascii_lowercase();
        let codec = rest
            .split("codecs=")
            .nth(1)
            .map(|c| c.trim_matches(|c| c == '"' || c == ' ').to_string())
            .unwrap_or_default();
        let video = match kind {
            "video" => true,
            "audio" => false,
            _ => return None,
        };
        let number = |key: &str| {
            f.get(key)
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        };
        let url = f.get("url").and_then(Value::as_str).map(str::to_string);
        let cipher = f
            .get("signatureCipher")
            .or_else(|| f.get("cipher"))
            .and_then(Value::as_str)
            .map(str::to_string);
        if url.is_none() && cipher.is_none() {
            return None;
        }
        let tags = xtags(url.as_deref(), cipher.as_deref());
        let tag = |key: &str| tags.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        let voice = Voice::of(f, tag("acont"));
        // The loudness-evened copy, and the voice-boosted one (`vb=1`), of a
        // track: the same bitrate as the plain one at times, so taken by
        // the highest-bitrate pick unless set aside.
        let drc =
            f.get("isDrc").and_then(Value::as_bool).unwrap_or(false) || tag("drc") == Some("1");
        let boosted = tag("vb") == Some("1");
        let range = |key: &str| {
            let r = f.get(key)?;
            let edge = |k: &str| {
                r.get(k)
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            };
            let (start, end) = (edge("start")?, edge("end")?);
            (end >= start).then_some((start, end))
        };
        let itag = number("itag")?;
        // A picture the answer lists by its format number alone is read from the table.
        let known = itags::describe(itag).filter(|k| video && k.kind != itags::Kind::Audio);
        let height = number("height")
            .or_else(|| known.map(|k| u64::from(k.height)))
            .unwrap_or(0);
        let width = number("width").unwrap_or(if number("height").is_none() {
            height * 16 / 9
        } else {
            0
        });
        Some(Format {
            itag,
            url,
            cipher,
            container: if !video && container == "mp4" {
                "m4a".to_string()
            } else {
                container
            },
            codec,
            mime,
            video,
            width,
            height,
            bitrate: number("averageBitrate")
                .or_else(|| number("bitrate"))
                .unwrap_or(0),
            size: number("contentLength"),
            fps: number("fps")
                .or_else(|| known.map(|k| u64::from(k.fps)))
                .unwrap_or(0),
            secondary: drc || boosted,
            voice,
            init: range("initRange"),
            index: range("indexRange"),
        })
    }

    /// Its name, which the row and the parts carry.
    fn ext(&self) -> &str {
        &self.container
    }
}

/// The picture and the sound (or the sound alone) for the quality asked,
/// chosen as `format_for` chooses: the biggest picture at or
/// under the quality, measured by its shorter side (a portrait video's
/// width: 720p is 720×1280 for a Short), in the container asked for where
/// there is one that size, with the best sound in the same container.
///
/// `frame_rate`, where the list gave the height two lines, keeps the picture to its side of 30
/// frames a second: above for the "60 fps" line, at or below for the other.
fn choose(
    formats: &[Format],
    quality: Quality,
    container: Option<&str>,
    frame_rate: Option<u32>,
) -> Option<Vec<Format>> {
    let usable: Vec<&Format> = formats.iter().filter(|f| !f.secondary).collect();
    let audio_in = |container: &str| {
        usable
            .iter()
            .filter(|f| !f.video && f.container == container)
            .max_by_key(|f| f.bitrate)
            .copied()
            .cloned()
    };
    let best_audio = || {
        audio_in("m4a")
            .into_iter()
            .chain(audio_in("webm"))
            .max_by_key(|f| f.bitrate)
    };
    if quality == Quality::Audio {
        // The sound alone in the container asked for, as the list offers
        // it (M4A first); the best of the rest where that has none.
        let preferred = match container {
            Some("webm") => "webm",
            _ => "m4a",
        };
        return audio_in(preferred).or_else(best_audio).map(|a| vec![a]);
    }
    let limit = match quality {
        Quality::UpTo(height) => height as u64,
        _ => u64::MAX,
    };
    let wanted = match container {
        Some("webm") => "webm",
        _ => "mp4",
    };
    // The picture's shorter side, which is what a quality names.
    let side = |f: &Format| {
        if f.width > 0 && f.width < f.height {
            f.width
        } else {
            f.height
        }
    };
    let all: Vec<&Format> = usable
        .iter()
        .filter(|f| f.video && f.height > 0 && side(f) <= limit)
        .copied()
        .collect();
    let biggest = all.iter().map(|f| side(f)).max()?;
    // The line's frame rate, where pictures that size have it; else what there is.
    let fast = |f: &Format| f.fps > 30;
    let pictures: Vec<&Format> = match frame_rate {
        Some(rate) => {
            let kept: Vec<&Format> = all
                .iter()
                .filter(|f| side(f) == biggest && fast(f) == (rate > 30))
                .copied()
                .collect();
            if kept.is_empty() {
                all
            } else {
                kept
            }
        }
        None => all,
    };
    // The container asked for at the biggest size, else whatever is biggest.
    let picture = pictures
        .iter()
        .filter(|f| side(f) == biggest && f.container == wanted)
        .max_by_key(|f| (f.height, f.fps, f.bitrate))
        .or_else(|| {
            pictures
                .iter()
                .filter(|f| side(f) == biggest)
                .max_by_key(|f| {
                    (
                        f.container == "mp4",
                        f.height,
                        f.fps,
                        itags::codec_rank(&f.codec),
                        f.bitrate,
                    )
                })
        })?;
    // A picture with its own sound (the formats with both in one file, which
    // name two codecs) needs none beside it: asking for one failed every
    // video offered only that way with "no stream at that quality"
    // (coverage tests, 2026-11-08).
    if picture.video && picture.codec.contains(',') {
        return Some(vec![(*picture).clone()]);
    }
    let sound = match picture.container.as_str() {
        "webm" => audio_in("webm").or_else(audio_in_m4a),
        _ => audio_in("m4a").or_else(|| audio_in("webm")),
    };
    fn audio_in_m4a() -> Option<Format> {
        None
    }
    let sound = sound.or_else(best_audio)?;
    Some(vec![(*picture).clone(), sound])
}

/// [`read`] for a download's options: through the proxy they name, at the
/// quality and in the container they ask, with the session they carry.
pub async fn read_for(
    url: &str,
    options: &crate::download::Options,
    staging: &Path,
) -> Result<String, Error> {
    let mut builder = crate::builder();
    if let Some(proxy) = options.proxy_for(url) {
        builder = builder.proxy(reqwest::Proxy::all(proxy)?);
    }
    let client = builder.build()?;
    // A part is cut from MP4 streams, whose pieces are indexed; a WebM
    // line picked in the panel gives way to the MP4 at that height.
    let part = options.span.is_some();
    let container = if part {
        Some("mp4")
    } else {
        options.container.as_deref()
    };
    // A part saved as GIF on Linux is made from the AV1 stream, which dm
    // decodes itself (`frames/av1.rs`); no taller than 720, as a GIF is 480
    // across and a decoder without the system's help is slow on more.
    let av1_gif = part && options.gif && cfg!(target_os = "linux");
    let (quality, codec) = if av1_gif {
        let at_most = match options.quality {
            Quality::UpTo(height) => Quality::UpTo(height.min(720)),
            _ => Quality::UpTo(720),
        };
        (at_most, Some("av01"))
    } else {
        (options.quality, None)
    };
    read_at(
        &client,
        url,
        quality,
        container,
        options.frame_rate,
        options.caption.clone(),
        part,
        codec,
        options.sign_in.as_ref(),
        staging,
    )
    .await
}

/// A YouTube watch page read, as `videopage::download_page` wants a reading:
/// the JSON a reader of the page prints for it, chosen at `quality`.
pub async fn read(
    client: &reqwest::Client,
    url: &str,
    quality: Quality,
    container: Option<&str>,
    mp4_only: bool,
    sign_in: Option<&crate::cookies::SignIn>,
    staging: &Path,
) -> Result<String, Error> {
    read_at(
        client, url, quality, container, None, None, mp4_only, None, sign_in, staging,
    )
    .await
}

/// [`read`], at the frame rate a list line named (`Options::frame_rate`), or the caption
/// track it named (`Options::caption`).
#[allow(clippy::too_many_arguments)]
pub async fn read_at(
    client: &reqwest::Client,
    url: &str,
    quality: Quality,
    container: Option<&str>,
    frame_rate: Option<u32>,
    caption: Option<(String, bool)>,
    mp4_only: bool,
    codec: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
    staging: &Path,
) -> Result<String, Error> {
    let id = video_id(url).ok_or_else(|| Error::Internal("not a YouTube video address".into()))?;
    tokio::time::timeout(
        WITHIN,
        read_within(
            client, &id, quality, container, frame_rate, caption, mp4_only, codec, sign_in, staging,
        ),
    )
    .await
    .map_err(|_| Error::Internal("the page took too long to read".into()))?
}

#[allow(clippy::too_many_arguments)]
async fn read_within(
    client: &reqwest::Client,
    id: &str,
    quality: Quality,
    container: Option<&str>,
    frame_rate: Option<u32>,
    caption: Option<(String, bool)>,
    mp4_only: bool,
    codec: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
    staging: &Path,
) -> Result<String, Error> {
    let origin = origin();
    let watch = format!("{origin}/watch?v={id}&bpctr=9999999999&has_verified=1");
    // The consent answer some countries' pages ask for first, and the
    // person's own session where the page handed one over.
    let mut cookie = String::from("PREF=hl=en&tz=UTC; SOCS=CAI");
    if let Some(theirs) = crate::cookies::header_with(sign_in, &watch) {
        cookie = format!("{theirs}; {cookie}");
    }
    let page = crate::redirects::send(
        client
            .get(&watch)
            .header(reqwest::header::USER_AGENT, PAGE_AGENT)
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(reqwest::header::COOKIE, &cookie),
    )
    .await?;
    let page = text_capped(Error::refused_or(page).await?, PAGE_MOST).await?;
    // A caption line: the track the page names, as a TTML file of its own (gaps row 72c).
    if let Some((language, asr)) = caption {
        return caption_reading(id, &page, &language, asr, staging);
    }
    let mut facts = page_facts(&page);
    // The player script, where its timestamp is wanted now (the page gave
    // none) and for the solver later.
    // Only YouTube's own player path is fetched and run: the solver runs
    // the player's code, so a page naming a script elsewhere is not
    // followed there.
    let js_path = facts
        .player_js
        .as_deref()
        .map(|named| {
            named
                .strip_prefix("https://www.youtube.com")
                .unwrap_or(named)
        })
        .filter(|path| player_path(path))
        .map(str::to_string);
    // The build every engine reads first, then the one the page named.
    let js_urls: Vec<String> = js_path
        .iter()
        .flat_map(|path| plain_build(path).into_iter().chain(Some(path.clone())))
        .map(|path| format!("{origin}{path}"))
        .collect();
    let fetch_script = || async {
        // Fetched once a session for every video it plays (gaps row Y13): a playlist's videos
        // share one player.
        if let Some(kept) = js_urls.iter().find_map(|url| kept_script(url)) {
            return Ok(kept);
        }
        let mut failed = None;
        for url in &js_urls {
            let answer = async {
                let script = crate::redirects::send(
                    client
                        .get(url)
                        .header(reqwest::header::USER_AGENT, PAGE_AGENT),
                )
                .await?;
                text_capped(Error::refused_or(script).await?, PAGE_MOST).await
            };
            match answer.await {
                Ok(text) => {
                    keep_script(url, &text);
                    return Ok(text);
                }
                Err(e) => failed = Some(e),
            }
        }
        Err(failed.unwrap_or_else(|| Error::Internal("the page names no player script".into())))
    };
    let mut script: Option<String> = None;
    if facts.sts.is_none() && !js_urls.is_empty() {
        let text = fetch_script().await?;
        facts.sts = sts_in_player(&text);
        script = Some(text);
    }

    let body = json!({
        "context": {"client": {
            "clientName": TV_CLIENT_NAME, "clientVersion": TV_CLIENT_VERSION,
            "deviceMake": "Samsung", "deviceModel": "UKS9800", "userAgent": TV_AGENT,
            "osName": "Tizen", "osVersion": "2.4.0",
            "hl": "en", "timeZone": "UTC", "utcOffsetMinutes": 0,
        }},
        "videoId": id,
        "playbackContext": {"contentPlaybackContext": {
            "html5Preference": "HTML5_PREF_WANTS",
            "signatureTimestamp": facts.sts,
        }},
        "contentCheckOk": true,
        "racyCheckOk": true,
    });
    let mut ask = client
        .post(format!("{origin}/youtubei/v1/player?prettyPrint=false"))
        .header(reqwest::header::USER_AGENT, TV_AGENT)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("X-Youtube-Client-Name", TV_CLIENT_ID)
        .header("X-Youtube-Client-Version", TV_CLIENT_VERSION)
        .header(reqwest::header::ORIGIN, &origin);
    if let Some(visitor) = &facts.visitor {
        ask = ask.header("X-Goog-Visitor-Id", visitor);
    }
    let answer = crate::redirects::send(ask.body(body.to_string())).await?;
    let answered = text_capped(Error::refused_or(answer).await?, PAGE_MOST).await?;
    let player: Value = serde_json::from_str(&answered)
        .map_err(|e| Error::Internal(format!("the player API answered no JSON ({e})")))?;
    let status = player
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        .unwrap_or("");
    if status != "OK" {
        let reason = player
            .pointer("/playabilityStatus/reason")
            .and_then(Value::as_str)
            .unwrap_or(status);
        return Err(Error::Internal(format!("YouTube says: {reason}")));
    }
    if player
        .pointer("/videoDetails/isLive")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        // Recorded from YouTube's own HLS playlist of it, where the player names one, by the
        // engine's live recording (gaps row Y5); refused, as before, where it names none.
        let live = player
            .pointer("/streamingData/hlsManifestUrl")
            .and_then(Value::as_str)
            .filter(|u| u.starts_with("https://"));
        let Some(live) = live else {
            return Err(Error::Internal("a live stream".into()));
        };
        let title = player
            .pointer("/videoDetails/title")
            .and_then(Value::as_str)
            .unwrap_or("live");
        return Ok(json!({
            "id": id,
            "title": title,
            "extractor": "youtube",
            "extractor_key": "Youtube",
            "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
            "is_live": true,
            "url": live,
            "protocol": "m3u8_native",
            "ext": "mp4",
            "filename": staging
                .join(format!("{} [{id}].mp4", safe_title(title)))
                .display()
                .to_string(),
            "read_by": "dm",
        })
        .to_string());
    }
    // The picture and the sound apart (`adaptiveFormats`), as a player
    // `bv+ba` takes them; the formats with both in one file only where
    // there is nothing apart, and then one alone (a picture with its own
    // sound and a sound beside it made a file with two sound tracks).
    let streams_at = |at: &str| -> Vec<Format> {
        player
            .pointer(at)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Format::of)
            .collect()
    };
    let mut formats = streams_at("/streamingData/adaptiveFormats");
    let muxed = formats.is_empty();
    if muxed {
        formats = streams_at("/streamingData/formats");
    }
    set_dubs_aside(&mut formats);
    if formats.is_empty() {
        return Err(Error::Internal(
            "the player names no stream dm can fetch".into(),
        ));
    }

    // The solver, for the `n` value every address carries and the cipher
    // some are locked behind.
    let needs = formats
        .iter()
        .any(|f| f.cipher.is_some() || f.url.as_deref().is_some_and(has_n));
    if needs {
        let script = match script {
            Some(text) => text,
            None => fetch_script().await?,
        };
        let solved = solve(&script, &formats).await?;
        for format in &mut formats {
            format.url = Some(unlocked(format, &solved)?);
            format.cipher = None;
        }
    }

    // A part is cut from MP4 streams alone (their pieces are indexed): a
    // height only WebM has gives way to the tallest MP4.
    let cuttable: Vec<Format> = formats
        .iter()
        .filter(|f| !mp4_only || f.container == "mp4" || f.container == "m4a")
        // A picture in the codec asked for (AV1, for a GIF made on Linux).
        .filter(|f| !f.video || codec.is_none_or(|c| f.codec.starts_with(c)))
        .cloned()
        .collect();
    if let Some(codec) = codec {
        if !cuttable.iter().any(|f| f.video) {
            return Err(Error::Internal(if codec == "av01" {
                "this video has no AV1 stream, which a GIF is made from on Linux".into()
            } else {
                format!("this video has no {codec} stream")
            }));
        }
    }
    let chosen = if muxed {
        choose(&cuttable, quality, container, frame_rate).map(|mut pair| {
            pair.truncate(1);
            pair
        })
    } else {
        choose(&cuttable, quality, container, frame_rate)
    }
    .ok_or_else(|| Error::Internal("no stream at that quality".into()))?;
    // The TV client's answer names no title; the page does.
    let title = player
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| page_title(&page))
        .unwrap_or_else(|| "video".into());
    let duration = player
        .pointer("/videoDetails/lengthSeconds")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
    let mut reading = reading_json(id, &title, duration, &formats, &chosen, staging);
    // The channel, written into the joined file with the title (gaps row Y11).
    if let Some(author) = player
        .pointer("/videoDetails/author")
        .and_then(Value::as_str)
    {
        reading["uploader"] = json!(author);
    }
    // The video's captions, for Options' "carry subtitles" to put inside it (gaps row Y6).
    let captions = captions_named(&player);
    if !captions.is_empty() {
        reading["requested_subtitles"] = Value::Object(captions);
    }
    // The player script read, for a record of which one answered (the weekly check).
    if let Some(path) = &js_path {
        reading["player"] = json!(path);
    }
    Ok(reading.to_string())
}

/// The most caption tracks a reading names: a video can list dozens of languages.
const CAPTIONS_MOST: usize = 8;

/// The caption tracks the player names, as the fetch step takes them (`requested_subtitles`:
/// each language's WebVTT address): those a person wrote, else the speech-recognised one, on
/// YouTube's own address alone (gaps row Y6).
fn captions_named(player: &Value) -> serde_json::Map<String, Value> {
    let tracks: Vec<&Value> = player
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let asr = |t: &&Value| t.get("kind").and_then(Value::as_str) == Some("asr");
    let written: Vec<&Value> = tracks.iter().copied().filter(|t| !asr(t)).collect();
    let chosen = if written.is_empty() {
        tracks.into_iter().filter(asr).take(1).collect()
    } else {
        written
    };
    let mut named = serde_json::Map::new();
    for track in chosen {
        if named.len() >= CAPTIONS_MOST {
            break;
        }
        let (Some(language), Some(base)) = (
            track.get("languageCode").and_then(Value::as_str),
            track.get("baseUrl").and_then(Value::as_str),
        ) else {
            continue;
        };
        let Ok(url) = caption_address(base, "vtt") else {
            continue;
        };
        named
            .entry(language.to_string())
            .or_insert_with(|| json!({ "url": url, "ext": "vtt" }));
    }
    named
}

/// A caption track's address in `format` (`vtt`, `ttml`): on YouTube's own `https` address
/// alone, its own `fmt` replaced; else why not.
fn caption_address(base: &str, format: &str) -> Result<String, &'static str> {
    let mut url = url::Url::parse(base)
        .or_else(|_| url::Url::parse(&format!("https://www.youtube.com{base}")))
        .map_err(|_| "the captions' address is not one")?;
    if url.host_str() != Some("www.youtube.com") || url.scheme() != "https" {
        return Err("the captions are not on YouTube");
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "fmt")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("fmt", format);
    Ok(url.to_string())
}

/// The reading for a caption line: the track of `language` (the speech-recognised one where
/// `asr`) the watch page's player data names, fetched as TTML into a file of its own named
/// after the video and the language. Only YouTube's own caption address is taken.
fn caption_reading(
    id: &str,
    page: &str,
    language: &str,
    asr: bool,
    staging: &Path,
) -> Result<String, Error> {
    const MARK: &str = "ytInitialPlayerResponse = ";
    let player: Value = page
        .find(MARK)
        .and_then(|at| json_object_at(page[at + MARK.len()..].trim_start()))
        .and_then(|object| serde_json::from_str(object).ok())
        .ok_or_else(|| Error::Internal("the page names no captions".into()))?;
    let track = player
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|t| {
            t.get("languageCode").and_then(Value::as_str) == Some(language)
                && (t.get("kind").and_then(Value::as_str) == Some("asr")) == asr
        })
        .ok_or_else(|| Error::Internal(format!("the video has no {language} captions now")))?;
    let base = track
        .get("baseUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Internal("the captions have no address".into()))?;
    let url = caption_address(base, "ttml").map_err(|why| Error::Internal(why.into()))?;
    let title = player
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .unwrap_or("video");
    let name = format!("{} [{id}].{language}.ttml", safe_title(title));
    Ok(json!({
        "id": id,
        "title": title,
        "extractor": "youtube",
        "extractor_key": "Youtube",
        "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
        "ext": "ttml",
        "filename": staging.join(name).display().to_string(),
        "url": url.to_string(),
        "protocol": "https",
        "format_id": format!("caption-{language}"),
        "read_by": "dm",
    })
    .to_string())
}

/// The video's title as the watch page gives it: its player data's, else
/// the page's own `<title>` without the site's name.
fn page_title(html: &str) -> Option<String> {
    const MARK: &str = "ytInitialPlayerResponse = ";
    let from_player = html.find(MARK).and_then(|at| {
        let object = json_object_at(html[at + MARK.len()..].trim_start())?;
        serde_json::from_str::<Value>(object)
            .ok()?
            .pointer("/videoDetails/title")?
            .as_str()
            .map(str::to_string)
    });
    from_player.or_else(|| {
        let start = html.find("<title>")? + "<title>".len();
        let end = html[start..].find("</title>")? + start;
        let title = html[start..end].trim();
        let title = title.strip_suffix(" - YouTube").unwrap_or(title);
        let title = unescaped(title);
        (!title.is_empty()).then_some(title)
    })
}

/// The few entities a page's title carries, read back.
fn unescaped(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Whether an address carries an `n` value to solve.
fn has_n(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| u.query_pairs().any(|(k, _)| k == "n"))
}

/// The address a stream is fetched from: its cipher decoded and its `n`
/// value replaced, from what the solver answered.
fn unlocked(format: &Format, solved: &Solved) -> Result<String, Error> {
    let (mut url, signature) = match (&format.url, &format.cipher) {
        (Some(url), _) => (parsed(url)?, None),
        (None, Some(cipher)) => {
            let pairs: HashMap<String, String> = url::form_urlencoded::parse(cipher.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            let url = pairs
                .get("url")
                .ok_or_else(|| Error::Internal("a cipher with no address".into()))?;
            let s = pairs
                .get("s")
                .ok_or_else(|| Error::Internal("a cipher with no signature".into()))?;
            let sig = solved
                .sig
                .get(s)
                .ok_or_else(|| Error::Internal("the signature was not solved".into()))?;
            let name = pairs
                .get("sp")
                .cloned()
                .unwrap_or_else(|| "signature".into());
            (parsed(url)?, Some((name, sig.clone())))
        }
        (None, None) => return Err(Error::Internal("a stream with no address".into())),
    };
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    for (key, value) in &mut pairs {
        if key == "n" {
            if let Some(answer) = solved.n.get(value) {
                *value = answer.clone();
            }
        }
    }
    if let Some((name, sig)) = signature {
        pairs.push((name, sig));
    }
    url.query_pairs_mut().clear().extend_pairs(pairs);
    Ok(url.to_string())
}

/// An address parsed, or why not in dm's words.
fn parsed(url: &str) -> Result<url::Url, Error> {
    url::Url::parse(url)
        .map_err(|e| Error::Internal(format!("a stream's address is not one ({e})")))
}

/// What the solver answered: each `n` value and each signature, by the
/// value asked about.
#[derive(Debug, Default)]
struct Solved {
    n: HashMap<String, String>,
    sig: HashMap<String, String>,
}

/// The build of the player script every engine reads, where the page named another: the one
/// in the JavaScript older engines know (`player_ias`) for the newer one (`player_es6`, whose
/// syntax Windows' own engine predates). The same version, so the same answers.
fn plain_build(path: &str) -> Option<String> {
    path.contains("/player_es6.vflset/")
        .then(|| path.replace("/player_es6.vflset/", "/player_ias.vflset/"))
}

/// Answers every `n` value and signature the formats carry, as reference A answers them: the
/// player script cut down to what answers them (`player::program`), run in the computer's own
/// JavaScript engine (`crate::javascript`).
async fn solve(player_js: &str, formats: &[Format]) -> Result<Solved, Error> {
    let mut ns = BTreeSet::new();
    let mut sigs = BTreeSet::new();
    let mut n_of = |address: &str| {
        if let Ok(url) = url::Url::parse(address) {
            ns.extend(
                url.query_pairs()
                    .filter(|(k, _)| k == "n")
                    .map(|(_, v)| v.into_owned()),
            );
        }
    };
    for format in formats {
        if let Some(url) = format.url.as_deref() {
            n_of(url);
        }
        // A locked address carries its `n` inside the cipher.
        if let Some(cipher) = &format.cipher {
            for (key, value) in url::form_urlencoded::parse(cipher.as_bytes()) {
                match &*key {
                    "s" => {
                        sigs.insert(value.into_owned());
                    }
                    "url" => n_of(&value),
                    _ => {}
                }
            }
        }
    }
    let ns: Vec<&str> = ns.iter().map(String::as_str).collect();
    let sigs: Vec<&str> = sigs.iter().map(String::as_str).collect();
    answer(player_js, &ns, &sigs).await
}

/// What the player script `player_js` answers for `ns` (`n` values) and `sigs` (signatures),
/// each by the value asked, cut and run as a download's reading runs it: for a person looking
/// into a player that changed (`examples/player_answers.rs`).
#[doc(hidden)]
pub async fn answers(
    player_js: &str,
    ns: &[&str],
    sigs: &[&str],
) -> Result<(HashMap<String, String>, HashMap<String, String>), Error> {
    let solved = answer(player_js, ns, sigs).await?;
    Ok((solved.n, solved.sig))
}

/// How many player scripts, and their cut programs, are kept: the one in use and the one
/// before it, a few megabytes each.
const PLAYERS_KEPT: usize = 2;

/// Player scripts fetched this session, by their address, newest last (gaps row Y13).
static SCRIPTS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// The cut programs of player scripts this session, by the script's own hash, newest last.
static PROGRAMS: std::sync::Mutex<Vec<(u64, String)>> = std::sync::Mutex::new(Vec::new());

fn kept_script(url: &str) -> Option<String> {
    crate::lock(&SCRIPTS)
        .iter()
        .find(|(kept, _)| kept == url)
        .map(|(_, text)| text.clone())
}

fn keep_script(url: &str, text: &str) {
    let mut kept = crate::lock(&SCRIPTS);
    kept.retain(|(old, _)| old != url);
    kept.push((url.to_string(), text.to_string()));
    let over = kept.len().saturating_sub(PLAYERS_KEPT);
    kept.drain(..over);
}

/// `player_js` cut down to what answers YouTube's checks (`player::program`), cut once a
/// session for each player rather than for every video.
fn cut_program(player_js: &str) -> Result<String, String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    player_js.hash(&mut hasher);
    let key = hasher.finish();
    if let Some((_, program)) = crate::lock(&PROGRAMS).iter().find(|(k, _)| *k == key) {
        return Ok(program.clone());
    }
    let program = player::program(player_js)?;
    let mut kept = crate::lock(&PROGRAMS);
    kept.push((key, program.clone()));
    let over = kept.len().saturating_sub(PLAYERS_KEPT);
    kept.drain(..over);
    Ok(program)
}

/// [`answers`], as the reading takes them.
async fn answer(player_js: &str, ns: &[&str], sigs: &[&str]) -> Result<Solved, Error> {
    let unanswered =
        |why: String| Error::Internal(format!("YouTube's checks were not answered: {why}"));
    let program = cut_program(player_js).map_err(unanswered)?;
    let call = player::call(ns, sigs);
    let output = crate::javascript::run(&program, &call, SOLVING)
        .await
        .map_err(unanswered)?;
    let answer: Value = serde_json::from_str(output.trim())
        .map_err(|_| unanswered(format!("the answer was {:?}", output.trim())))?;
    let mut solved = Solved::default();
    for (kind, asked, into) in [("n", ns, &mut solved.n), ("sig", sigs, &mut solved.sig)] {
        for challenge in asked.iter() {
            let got = answer
                .pointer(&format!(
                    "/{kind}/{}",
                    challenge.replace('~', "~0").replace('/', "~1")
                ))
                .and_then(Value::as_str)
                .filter(|got| !got.is_empty())
                .ok_or_else(|| unanswered(format!("no answer for the {kind} value {challenge}")))?;
            into.insert((*challenge).to_string(), got.to_string());
        }
    }
    Ok(solved)
}

/// The reading as JSON: what `videopage::download_page` reads of one
/// (`filename`, `requested_formats` or the one format, each with its
/// address, type, agent, chunk and size) and what the panel's list reads
/// (`formats` with heights, codecs and sizes).
fn reading_json(
    id: &str,
    title: &str,
    duration: Option<u64>,
    formats: &[Format],
    chosen: &[Format],
    staging: &Path,
) -> Value {
    let describe = |f: &Format| {
        json!({
            "format_id": f.itag.to_string(),
            "url": f.url,
            "protocol": "https",
            "ext": f.ext(),
            "vcodec": if f.video { f.codec.as_str() } else { "none" },
            "acodec": if f.video { "none" } else { f.codec.as_str() },
            "width": if f.video { Some(f.width) } else { None },
            "height": if f.video { Some(f.height) } else { None },
            "fps": if f.video && f.fps > 0 { Some(f.fps) } else { None },
            "tbr": f.bitrate / 1000,
            "filesize": f.size,
            "http_headers": {"User-Agent": TV_AGENT},
            "downloader_options": {"http_chunk_size": CHUNK},
            // dm's own, for cutting a part: where the stream's first boxes
            // and its index are.
            "dm_init_range": f.init.map(|(a, b)| format!("{a}-{b}")),
            "dm_index_range": f.index.map(|(a, b)| format!("{a}-{b}")),
        })
    };
    let ext = chosen
        .first()
        .map(|f| f.ext().to_string())
        .unwrap_or_else(|| "mp4".into());
    let name = format!("{} [{id}].{ext}", safe_title(title));
    let mut reading = json!({
        "id": id,
        "title": title,
        "extractor": "youtube",
        "extractor_key": "Youtube",
        "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
        "duration": duration,
        "ext": ext,
        "filename": staging.join(name).display().to_string(),
        "formats": formats.iter().map(describe).collect::<Vec<_>>(),
        "read_by": "dm",
    });
    match chosen {
        [one] => {
            if let (Value::Object(into), Value::Object(from)) = (&mut reading, describe(one)) {
                for (key, value) in from {
                    into.insert(key, value);
                }
            }
        }
        pair => {
            reading["requested_formats"] = pair.iter().map(describe).collect();
        }
    }
    reading
}

/// The title as it is written in a file name: the
/// characters a file name cannot hold replaced, cut to 200 bytes whole.
fn safe_title(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().chars() {
        let c = match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => ' ',
            c => c,
        };
        if out.len() + c.len_utf8() > 200 {
            break;
        }
        out.push(c);
    }
    let out = out.trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        "video".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The captions a person wrote are named, on YouTube's own address as WebVTT; a
    /// speech-recognised track only where there is none, and none from elsewhere (gaps row Y6).
    #[test]
    fn a_videos_written_captions_are_named_as_webvtt() {
        let player = json!({"captions": {"playerCaptionsTracklistRenderer": {"captionTracks": [
            {"languageCode": "en", "baseUrl": "https://www.youtube.com/api/timedtext?v=x&lang=en&fmt=srv3"},
            {"languageCode": "en", "kind": "asr", "baseUrl": "https://www.youtube.com/api/timedtext?v=x&lang=en&kind=asr"},
            {"languageCode": "de", "baseUrl": "/api/timedtext?v=x&lang=de"},
            {"languageCode": "fr", "baseUrl": "https://elsewhere.example/timedtext?lang=fr"}
        ]}}});
        let named = captions_named(&player);
        let mut languages: Vec<&String> = named.keys().collect();
        languages.sort();
        assert_eq!(languages, ["de", "en"]);
        let en = named["en"]["url"].as_str().unwrap();
        assert!(
            en.ends_with("&fmt=vtt") && !en.contains("srv3") && !en.contains("asr"),
            "{en}"
        );
        assert_eq!(named["de"]["ext"], "vtt");
        assert!(named["de"]["url"]
            .as_str()
            .unwrap()
            .starts_with("https://www.youtube.com/"));
        let spoken = json!({"captions": {"playerCaptionsTracklistRenderer": {"captionTracks": [
            {"languageCode": "en", "kind": "asr", "baseUrl": "https://www.youtube.com/api/timedtext?v=x&kind=asr"}
        ]}}});
        assert_eq!(captions_named(&spoken).len(), 1);
        assert!(captions_named(&json!({})).is_empty());
    }

    #[test]
    fn a_link_says_where_it_starts_in_each_of_youtubes_ways() {
        let at = |u: &str| start_of(u);
        assert_eq!(at("https://www.youtube.com/watch?v=abc&t=34"), Some(34));
        assert_eq!(at("https://youtu.be/abc?t=1m30s"), Some(90));
        assert_eq!(
            at("https://www.youtube.com/watch?v=abc#t=1h2m3s"),
            Some(3723)
        );
        assert_eq!(at("https://www.youtube.com/embed/abc?start=75"), Some(75));
        assert_eq!(at("https://www.youtube.com/watch?v=abc&t=0"), None);
        assert_eq!(at("https://www.youtube.com/watch?v=abc"), None);
        assert_eq!(at("https://www.youtube.com/watch?v=abc&t=soon"), None);
    }

    #[test]
    fn a_video_id_is_read_from_every_address_shape() {
        for (url, id) in [
            (
                "https://www.youtube.com/watch?v=jNQXAC9IVRw",
                Some("jNQXAC9IVRw"),
            ),
            (
                "https://youtube.com/watch?t=5&v=jNQXAC9IVRw&list=x",
                Some("jNQXAC9IVRw"),
            ),
            ("https://youtu.be/jNQXAC9IVRw?t=3", Some("jNQXAC9IVRw")),
            (
                "https://m.youtube.com/shorts/jNQXAC9IVRw",
                Some("jNQXAC9IVRw"),
            ),
            (
                "https://www.youtube.com/embed/jNQXAC9IVRw",
                Some("jNQXAC9IVRw"),
            ),
            (
                "https://www.youtube-nocookie.com/embed/jNQXAC9IVRw",
                Some("jNQXAC9IVRw"),
            ),
            (
                "https://www.youtube.com/live/jNQXAC9IVRw",
                Some("jNQXAC9IVRw"),
            ),
            ("https://www.youtube.com/playlist?list=PL123", None),
            ("https://www.youtube.com/watch?v=short", None),
            ("https://example.com/watch?v=jNQXAC9IVRw", None),
            ("not an address", None),
        ] {
            assert_eq!(video_id(url).as_deref(), id, "{url}");
        }
    }

    #[test]
    fn the_pages_facts_are_read_from_its_config() {
        let html = r#"<html><script>ytcfg.set({"CLIENT":"x"});ytcfg.set({"INNERTUBE_CONTEXT":{"client":{"visitorData":"Cgt2aXNpdG9y"}},"PLAYER_JS_URL":"/s/player/11a45ffb/player_ias.vflset/en_US/base.js","STS":20753,"TEXT":"a } brace in a string"});</script>"#;
        assert_eq!(
            page_facts(html),
            PageFacts {
                visitor: Some("Cgt2aXNpdG9y".into()),
                player_js: Some("/s/player/11a45ffb/player_ias.vflset/en_US/base.js".into()),
                sts: Some(20753),
            }
        );
        assert_eq!(page_facts("<html>nothing</html>"), PageFacts::default());
        assert_eq!(
            sts_in_player("var a={signatureTimestamp:20753,b:1}"),
            Some(20753)
        );
        assert_eq!(sts_in_player("var a={sts:123}"), None);
    }

    const PLAYER: &str = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"title":"Me at the zoo","lengthSeconds":"19"},
      "streamingData":{"adaptiveFormats":[
        {"itag":137,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1920,"height":1080,"fps":30,"bitrate":4000000,"contentLength":"8000","url":"https://r1.example/v?itag=137&n=aaa"},
        {"itag":248,"mimeType":"video/webm; codecs=\"vp9\"","width":1920,"height":1080,"bitrate":3000000,"contentLength":"3000","url":"https://r1.example/v?itag=248&n=aaa"},
        {"itag":136,"mimeType":"video/mp4; codecs=\"avc1.4d401f\"","width":1280,"height":720,"bitrate":2000000,"contentLength":"2000","signatureCipher":"s=ABCsig&sp=sig&url=https%3A%2F%2Fr1.example%2Fv%3Fitag%3D136%26n%3Dbbb"},
        {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500","url":"https://r1.example/a?itag=140&n=aaa"},
        {"itag":251,"mimeType":"audio/webm; codecs=\"opus\"","averageBitrate":140000,"contentLength":"400","url":"https://r1.example/a?itag=251&n=ccc"},
        {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500","isDrc":true,"url":"https://r1.example/a?itag=140&n=aaa&drc=1"},
        {"itag":22,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"bitrate":1,"drmFamilies":["w"],"url":"https://r1.example/drm"},
        {"itag":99,"mimeType":"video/mp4","width":1,"height":1,"bitrate":1,"type":"FORMAT_STREAM_TYPE_OTF","url":"https://r1.example/otf"}
      ]}}"#;

    fn formats() -> Vec<Format> {
        let player: Value = serde_json::from_str(PLAYER).unwrap();
        player
            .pointer("/streamingData/adaptiveFormats")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(Format::of)
            .collect()
    }

    #[test]
    fn only_youtubes_own_player_script_is_fetched() {
        assert!(player_path(
            "/s/player/abcd1234/player_ias.vflset/en_US/base.js"
        ));
        assert!(!player_path("/s/player/../../evil.js"));
        assert!(!player_path("//evil.example/s/player/x/base.js"));
        assert!(!player_path("/elsewhere/base.js"));
        assert!(!player_path("/s/player/x/base.js?y=1"));
    }

    #[test]
    fn a_playlist_address_is_told_from_a_watch_address_with_a_list() {
        assert_eq!(
            playlist_id("https://www.youtube.com/playlist?list=PLabc-_123"),
            Some("PLabc-_123".to_string())
        );
        assert_eq!(
            playlist_id("https://m.youtube.com/playlist?list=PL1&hl=en"),
            Some("PL1".into())
        );
        assert_eq!(
            playlist_id("https://www.youtube.com/watch?v=abc&list=PL1"),
            None
        );
        assert_eq!(playlist_id("https://www.youtube.com/playlist?list="), None);
        assert_eq!(
            playlist_id("https://www.youtube.com/playlist?list=a/b"),
            None
        );
        assert_eq!(playlist_id("https://example.com/playlist?list=PL1"), None);
    }

    #[test]
    fn a_playlist_pages_videos_and_its_next_page_are_gathered() {
        let html = r#"<html><script>ytcfg.set({"INNERTUBE_CLIENT_VERSION":"2.20261001.01.00"});</script>
        <script>var ytInitialData = {"metadata":{"playlistMetadataRenderer":{"title":"Northern Series"}},
        "contents":{"list":[{"playlistVideoRenderer":{"videoId":"vid1_______","title":{"runs":[{"text":"Northern Light"}]},"lengthSeconds":"634","index":{"simpleText":"1"}}},
        {"playlistVideoRenderer":{"videoId":"vid2_______","title":{"simpleText":"Still Water"}}},
        {"playlistVideoRenderer":{"videoId":"vid4_______","title":{"simpleText":"[Private video]"}}},
        {"playlistVideoRenderer":{"videoId":"vid5_______","isPlayable":false,"title":{"simpleText":"Gone"}}},
        {"playlistVideoRenderer":{"videoId":"abc&list=PL","title":{"simpleText":"Odd id"}}},
        {"playlistVideoRenderer":{"videoId":"vid1_______","title":{"runs":[{"text":"again"}]}}},
        {"lockupViewModel":{"contentId":"vid3_______","contentType":"LOCKUP_CONTENT_TYPE_VIDEO","contentImage":{"thumbnailViewModel":{"overlays":[{"thumbnailBottomOverlayViewModel":{"badges":[{"thumbnailBadgeViewModel":{"text":"12:15"}}]}}]}},"metadata":{"lockupMetadataViewModel":{"title":{"content":"Tears of Steel"}}}}},
        {"lockupViewModel":{"contentId":"PLx________","contentType":"LOCKUP_CONTENT_TYPE_PLAYLIST","metadata":{"lockupMetadataViewModel":{"title":{"content":"Another list"}}}}},
        {"continuationItemViewModel":{"continuationCommand":{"innertubeCommand":{"continuationCommand":{"token":"NEXT-1"}}}}}]}};</script></html>"#;
        let data = initial_data(html).unwrap();
        assert_eq!(playlist_title(&data).as_deref(), Some("Northern Series"));
        assert_eq!(client_version(html).as_deref(), Some("2.20261001.01.00"));
        let mut entries = Vec::new();
        let token = gather(&data, &mut entries);
        assert_eq!(token.as_deref(), Some("NEXT-1"));
        assert_eq!(
            entries,
            [
                PlaylistEntry {
                    id: "vid1_______".into(),
                    title: "Northern Light".into(),
                    seconds: Some(634),
                    page: None
                },
                PlaylistEntry {
                    id: "vid2_______".into(),
                    title: "Still Water".into(),
                    seconds: None,
                    page: None
                },
                PlaylistEntry {
                    id: "vid3_______".into(),
                    title: "Tears of Steel".into(),
                    seconds: Some(735),
                    page: None
                },
            ]
        );
        assert_eq!(
            entries[0].url(),
            "https://www.youtube.com/watch?v=vid1_______"
        );
        assert_eq!(initial_data("<html>nothing</html>"), None);
    }

    #[test]
    fn the_title_comes_from_the_page_where_the_api_gives_none() {
        let html = r#"<html><title>Me &amp; the zoo - YouTube</title><script>var ytInitialPlayerResponse = {"videoDetails":{"title":"Me at the zoo"}};</script>"#;
        assert_eq!(page_title(html).as_deref(), Some("Me at the zoo"));
        let bare = "<html><head><title>Me &amp; the zoo - YouTube</title></head></html>";
        assert_eq!(page_title(bare).as_deref(), Some("Me & the zoo"));
        assert_eq!(page_title("<html></html>"), None);
    }

    #[test]
    fn a_picture_listed_by_its_format_number_alone_is_read_from_the_table() {
        let f = |json: &str| Format::of(&serde_json::from_str::<Value>(json).unwrap()).unwrap();
        let bare = f(
            r#"{"itag":299,"mimeType":"video/mp4; codecs=\"avc1.64002a\"","url":"https://r1.example/v"}"#,
        );
        assert_eq!((bare.height, bare.fps), (1080, 60));
        let told = f(
            r#"{"itag":299,"mimeType":"video/mp4; codecs=\"avc1\"","width":1920,"height":1000,"fps":24,"url":"https://r1.example/v"}"#,
        );
        assert_eq!(
            (told.height, told.fps),
            (1000, 24),
            "what the answer says wins"
        );
        let sound = f(
            r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.example/a"}"#,
        );
        assert_eq!((sound.height, sound.fps), (0, 0));
    }

    /// The sound tracks as the player API gave them for a Spanish video with
    /// an English automatic dub (2026-11-09): the dub marked as the one to
    /// play, the original not, and the original in three copies (plain,
    /// loudness-evened, voice-boosted) at one bitrate.
    fn dubbed_video(with_xtags: bool) -> Vec<Format> {
        let sound = |track: &str, name: &str, default: bool, tags: &str, extra: &str| {
            let address = if with_xtags {
                format!(
                    "https://r1.example/a?itag=140&xtags={}",
                    tags.replace(':', "%3A").replace('=', "%3D")
                )
            } else {
                format!(
                    "https://r1.example/a?itag=140&t={}",
                    tags.replace([':', '='], "-")
                )
            };
            format!(
                r#"{{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":129483,"audioTrack":{{"id":"{track}","displayName":"{name}","audioIsDefault":{default}}}{extra},"url":"{address}"}}"#
            )
        };
        let mut formats: Vec<Format> = [
            r#"{"itag":137,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1920,"height":1080,"bitrate":4000000,"url":"https://r1.example/v?itag=137"}"#.to_string(),
            sound("en-US.10", "English (US)", true, "acont=dubbed-auto:lang=en-US", ""),
            sound("es-US.4", "Spanish (US) original", false, "acont=original:drc=1:lang=es-US", r#","isDrc":true"#),
            sound("es-US.4", "Spanish (US) original", false, "acont=original:lang=es-US", ""),
            sound("es-US.4", "Spanish (US) original", false, "acont=original:lang=es-US:vb=1", ""),
        ]
        .iter()
        .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
        .collect();
        set_dubs_aside(&mut formats);
        formats
    }

    fn sound_chosen(formats: &[Format], quality: Quality) -> String {
        choose(formats, quality, None, None)
            .unwrap()
            .into_iter()
            .find(|f| !f.video)
            .and_then(|f| f.url)
            .unwrap()
    }

    #[test]
    fn a_dubbed_videos_own_language_is_chosen_not_the_dub_marked_to_play() {
        let formats = dubbed_video(true);
        for quality in [Quality::Best, Quality::UpTo(1080), Quality::Audio] {
            let chosen = sound_chosen(&formats, quality);
            assert!(
                chosen.contains("acont%3Doriginal%3Alang%3Des-US") && !chosen.contains("vb%3D1"),
                "{quality:?}: {chosen}"
            );
        }
    }

    #[test]
    fn without_xtags_the_track_id_names_the_original() {
        let formats = dubbed_video(false);
        assert_eq!(
            formats.iter().filter(|f| !f.video && !f.secondary).count(),
            2,
            "the dub is set aside, the original's two copies kept (only the address named the boosted one)"
        );
        assert!(sound_chosen(&formats, Quality::Audio).contains("lang-es-US"));
        assert!(!sound_chosen(&formats, Quality::Audio).contains("dubbed"));
    }

    #[test]
    fn where_no_original_is_named_the_track_marked_to_play_is_the_videos_own() {
        let mut formats: Vec<Format> = [
            r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"audioTrack":{"id":"de.3","audioIsDefault":false},"url":"https://r1.example/a?d=de"}"#,
            r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":120000,"audioTrack":{"id":"en.3","audioIsDefault":true},"url":"https://r1.example/a?d=en"}"#,
        ]
        .iter()
        .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
        .collect();
        set_dubs_aside(&mut formats);
        assert_eq!(
            sound_chosen(&formats, Quality::Audio),
            "https://r1.example/a?d=en"
        );
    }

    #[test]
    fn tracks_none_of_which_is_the_videos_own_are_all_kept() {
        let mut formats: Vec<Format> = [
            r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"audioTrack":{"id":"de.3","audioIsDefault":false},"url":"https://r1.example/a?d=de"}"#,
            r#"{"itag":251,"mimeType":"audio/webm; codecs=\"opus\"","averageBitrate":120000,"audioTrack":{"id":"fr.3","audioIsDefault":false},"url":"https://r1.example/a?d=fr"}"#,
        ]
        .iter()
        .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
        .collect();
        set_dubs_aside(&mut formats);
        assert!(
            formats.iter().all(|f| !f.secondary),
            "a video is not left without sound"
        );
    }

    #[test]
    fn xtags_are_read_from_the_address_or_from_inside_its_cipher() {
        let pairs = |v: &[(&str, &str)]| {
            v.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            xtags(
                Some("https://r1.example/a?xtags=acont%3Doriginal%3Alang%3Des%3Avb%3D1&n=x"),
                None
            ),
            pairs(&[("acont", "original"), ("lang", "es"), ("vb", "1")])
        );
        let cipher = "s=abc&sp=sig&url=https%3A%2F%2Fr1.example%2Fa%3Fxtags%3Dacont%253Ddubbed%253Alang%253Den";
        assert_eq!(
            xtags(None, Some(cipher)),
            pairs(&[("acont", "dubbed"), ("lang", "en")])
        );
        assert!(xtags(Some("https://r1.example/a?itag=140"), None).is_empty());
        assert!(xtags(None, None).is_empty());
    }

    #[test]
    fn locked_and_streamed_only_formats_are_left_out_and_the_rest_read() {
        let formats = formats();
        let itags: Vec<u64> = formats.iter().map(|f| f.itag).collect();
        assert_eq!(itags, [137, 248, 136, 140, 251, 140]);
        assert!(formats[5].secondary, "the loudness variant is not chosen");
        assert_eq!(formats[3].ext(), "m4a");
        assert_eq!(formats[1].ext(), "webm");
    }

    #[test]
    fn the_quality_asked_chooses_the_stream_it_names() {
        let formats = formats();
        let itags = |chosen: Option<Vec<Format>>| {
            chosen.map(|c| c.iter().map(|f| f.itag).collect::<Vec<_>>())
        };
        assert_eq!(
            itags(choose(&formats, Quality::Best, None, None)),
            Some(vec![137, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::Best, Some("webm"), None)),
            Some(vec![248, 251])
        );
        assert_eq!(
            itags(choose(&formats, Quality::UpTo(720), None, None)),
            Some(vec![136, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::Audio, None, None)),
            Some(vec![140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::Audio, Some("webm"), None)),
            Some(vec![251])
        );
        assert_eq!(
            itags(choose(&formats, Quality::UpTo(100), None, None)),
            None
        );
    }

    #[test]
    fn a_portrait_video_is_chosen_by_its_shorter_side() {
        // A Short: 720p is 720 wide and 1280 tall, as YouTube names it, not
        // the 405×720 picture that is 720 tall.
        let player: Value = serde_json::from_str(
            r#"[
            {"itag":301,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1080,"height":1920,"bitrate":5000000,"url":"https://r1.example/v?itag=301&n=aaa"},
            {"itag":302,"mimeType":"video/mp4; codecs=\"avc1.4d401f\"","width":720,"height":1280,"bitrate":2500000,"url":"https://r1.example/v?itag=302&n=aaa"},
            {"itag":303,"mimeType":"video/mp4; codecs=\"avc1.4d401e\"","width":405,"height":720,"bitrate":1000000,"url":"https://r1.example/v?itag=303&n=aaa"},
            {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"url":"https://r1.example/a?itag=140&n=aaa"}
            ]"#,
        )
        .unwrap();
        let formats: Vec<Format> = player
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Format::of)
            .collect();
        let itags = |chosen: Option<Vec<Format>>| {
            chosen.map(|c| c.iter().map(|f| f.itag).collect::<Vec<_>>())
        };
        assert_eq!(
            itags(choose(&formats, Quality::UpTo(720), None, None)),
            Some(vec![302, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::UpTo(1080), None, None)),
            Some(vec![301, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::UpTo(480), None, None)),
            Some(vec![303, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::Best, None, None)),
            Some(vec![301, 140])
        );
    }

    #[test]
    fn an_address_is_unlocked_by_what_the_solver_answered() {
        let formats = formats();
        let solved = Solved {
            n: [
                ("aaa".to_string(), "AAA".to_string()),
                ("bbb".into(), "BBB".into()),
            ]
            .into_iter()
            .collect(),
            sig: [("ABCsig".to_string(), "SOLVEDSIG".to_string())]
                .into_iter()
                .collect(),
        };
        assert_eq!(
            unlocked(&formats[0], &solved).unwrap(),
            "https://r1.example/v?itag=137&n=AAA"
        );
        assert_eq!(
            unlocked(&formats[2], &solved).unwrap(),
            "https://r1.example/v?itag=136&n=BBB&sig=SOLVEDSIG"
        );
        // An `n` the solver did not answer stays as it was.
        assert_eq!(
            unlocked(&formats[4], &solved).unwrap(),
            "https://r1.example/a?itag=251&n=ccc"
        );
        assert!(has_n("https://r1.example/a?itag=251&n=ccc"));
        assert!(!has_n("https://r1.example/a?itag=251"));
    }

    /// A caption line is the track the page names, fetched as TTML into a file named after the
    /// video and its language; a track elsewhere than YouTube is refused (gaps row 72c).
    #[test]
    fn a_caption_line_is_its_track_as_a_ttml_file() {
        let page = r#"<script>var ytInitialPlayerResponse = {"videoDetails":{"title":"A talk"},
          "captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[
            {"baseUrl":"https://www.youtube.com/api/timedtext?v=abc&lang=hi&kind=asr&fmt=srv3","languageCode":"hi","kind":"asr"},
            {"baseUrl":"https://evil.example/t?lang=en","languageCode":"en"}]}}};</script>"#;
        let dir = std::path::Path::new("/staging");
        let reading: Value =
            serde_json::from_str(&caption_reading("abc", page, "hi", true, dir).unwrap()).unwrap();
        let url = reading["url"].as_str().unwrap();
        assert!(
            url.starts_with("https://www.youtube.com/api/timedtext?"),
            "{url}"
        );
        assert!(
            url.contains("lang=hi") && url.ends_with("fmt=ttml") && !url.contains("srv3"),
            "{url}"
        );
        assert_eq!(reading["ext"], "ttml");
        assert!(reading["filename"]
            .as_str()
            .unwrap()
            .ends_with("A talk [abc].hi.ttml"));
        assert!(
            caption_reading("abc", page, "en", false, dir).is_err(),
            "not YouTube's"
        );
        assert!(
            caption_reading("abc", page, "hi", false, dir).is_err(),
            "not the ASR one"
        );
    }

    /// The line's frame rate is the picture's: "720p 60 fps" gets the faster
    /// picture, "720p" the other, and with no rate named the faster (gaps row 72d).
    #[test]
    fn the_line_picked_names_the_frame_rate_fetched() {
        let formats: Vec<Format> = [
            r#"{"itag":136,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"fps":30,"bitrate":1000,"url":"https://r1.example/v?itag=136"}"#,
            r#"{"itag":298,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"fps":60,"bitrate":2000,"url":"https://r1.example/v?itag=298"}"#,
            r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":128000,"url":"https://r1.example/v?itag=140"}"#,
        ]
        .iter()
        .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
        .collect();
        let picture = |rate| {
            choose(&formats, Quality::UpTo(720), Some("mp4"), rate)
                .unwrap()
                .first()
                .map(|f| f.itag)
        };
        assert_eq!(picture(Some(60)), Some(298));
        assert_eq!(picture(Some(30)), Some(136));
        assert_eq!(picture(None), Some(298));
    }

    #[test]
    fn the_reading_is_in_the_shape_the_downloader_reads() {
        let mut formats = formats();
        for f in &mut formats {
            f.url = Some("https://r1.example/x".into());
            f.cipher = None;
        }
        let chosen = choose(&formats, Quality::Best, None, None).unwrap();
        let reading = reading_json(
            "jNQXAC9IVRw",
            "Me at the zoo: a \"first\" / video",
            Some(19),
            &formats,
            &chosen,
            Path::new("/tmp/staging"),
        );
        // Joined as the system joins paths (a backslash on Windows).
        assert_eq!(
            reading["filename"],
            Path::new("/tmp/staging")
                .join("Me at the zoo_ a _first_ _ video [jNQXAC9IVRw].mp4")
                .display()
                .to_string()
        );
        let pair = reading["requested_formats"].as_array().unwrap();
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[0]["ext"], "mp4");
        assert_eq!(pair[0]["protocol"], "https");
        assert_eq!(pair[0]["downloader_options"]["http_chunk_size"], 10_485_760);
        assert_eq!(pair[1]["ext"], "m4a");
        assert_eq!(pair[1]["vcodec"], "none");
        assert_eq!(reading["formats"].as_array().unwrap().len(), 6);
        let sound = choose(&formats, Quality::Audio, Some("webm"), None).unwrap();
        let alone = reading_json("jNQXAC9IVRw", "x", None, &formats, &sound, Path::new("/s"));
        assert_eq!(alone["ext"], "webm");
        assert_eq!(alone["url"], "https://r1.example/x");
        assert!(alone.get("requested_formats").is_none());
        assert_eq!(safe_title(&"é".repeat(150)).len(), 200);
        assert_eq!(safe_title("  ... "), "video");
    }
}
