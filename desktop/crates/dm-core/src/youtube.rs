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

mod chapters;
mod itags;
mod kept;
mod player;
mod playlist;

use crate::stream::Quality;
use crate::Error;
use chapters::{chapters_in, largest_thumbnail, publish_date};
pub use kept::set_kept_players;
use kept::{cut_program, keep_script, kept_script};
#[cfg(test)]
use playlist::{channel_id_in, gather, playlist_title};
pub use playlist::{
    channel_of, is_list, playlist_id, read_playlist, Channel, Playlist, PlaylistEntry,
};

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

/// A client YouTube's player is asked as: dm's own, or one the served rules name (gaps row Y1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerClient {
    /// Its `clientName`.
    pub name: String,
    /// Its `clientVersion`.
    pub version: String,
    /// Its number (`X-Youtube-Client-Name`).
    pub id: String,
    /// The User-Agent it is asked with.
    pub agent: String,
    /// Further fields of its `context.client`.
    pub context: Vec<(String, String)>,
}

/// The clients the served rules name, asked before dm's own, in their order.
static SERVED_CLIENTS: std::sync::Mutex<Vec<PlayerClient>> = std::sync::Mutex::new(Vec::new());

/// The clients the served rules name (`rules.json`'s `youtube.clients`), in place of any before:
/// asked in order before dm's own, so a client YouTube stops answering is changed with no new
/// build (gaps row Y1).
pub fn set_player_clients(clients: Vec<PlayerClient>) {
    *crate::lock(&SERVED_CLIENTS) = clients;
}

/// The clients the served rules name, as set last.
pub fn served_player_clients() -> Vec<PlayerClient> {
    crate::lock(&SERVED_CLIENTS).clone()
}

/// The player scripts whose checks were not answered this run, by id, and how many of them the
/// window has been handed (gaps row Y3).
static UNSOLVED: std::sync::Mutex<(Vec<String>, usize)> = std::sync::Mutex::new((Vec::new(), 0));

/// The id of the player script at `path` (`/s/player/<id>/…`), noted as one whose checks were
/// not answered, once.
fn note_unsolved(path: Option<&str>) {
    let Some(player) = path
        .and_then(|p| p.strip_prefix("/s/player/"))
        .and_then(|p| p.split('/').next())
        .filter(|id| !id.is_empty())
    else {
        return;
    };
    let mut held = crate::lock(&UNSOLVED);
    if !held.0.iter().any(|seen| seen == player) {
        held.0.push(player.to_string());
    }
}

/// Every player script whose checks were not answered this run, by id, in the order met.
pub fn unsolved_players() -> Vec<String> {
    crate::lock(&UNSOLVED).0.clone()
}

/// The next player not yet handed on whose checks were not answered: for the window to ask
/// for fresh site rules once for each, out of the weekly turn (gaps row Y3).
pub fn take_unsolved_player() -> Option<String> {
    let mut held = crate::lock(&UNSOLVED);
    let (all, handed) = &mut *held;
    let next = all.get(*handed).cloned()?;
    *handed += 1;
    Some(next)
}

/// The TV client this reads with today: its answers carry plain addresses and want no
/// proof-of-origin token.
fn own_client() -> PlayerClient {
    PlayerClient {
        name: TV_CLIENT_NAME.into(),
        version: TV_CLIENT_VERSION.into(),
        id: TV_CLIENT_ID.into(),
        agent: TV_AGENT.into(),
        context: [
            ("deviceMake", "Samsung"),
            ("deviceModel", "UKS9800"),
            ("osName", "Tizen"),
            ("osVersion", "2.4.0"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .to_vec(),
    }
}

/// Every client to ask as, in order: the served ones, then dm's own (once).
fn player_clients() -> Vec<PlayerClient> {
    let mut all = crate::lock(&SERVED_CLIENTS).clone();
    let own = own_client();
    if !all
        .iter()
        .any(|c| c.name == own.name && c.version == own.version)
    {
        all.push(own);
    }
    all
}

/// The player's answer for video `id`, asked as each of `clients` in turn until one answers
/// with a video that plays, the last one's reason where none does: the turn `read_within`
/// takes, without the streams (which a client is also passed over for), for its tests.
#[cfg(test)]
async fn ask_player(
    client: &reqwest::Client,
    origin: &str,
    id: &str,
    facts: &PageFacts,
    clients: &[PlayerClient],
) -> Result<Value, Error> {
    let mut failed = None;
    for asked_as in clients {
        match ask_player_as(client, origin, id, facts, asked_as).await {
            Ok(player) => return Ok(player),
            Err(e) => failed = Some(e),
        }
    }
    Err(failed.unwrap_or_else(|| Error::Internal("no client to ask YouTube's player as".into())))
}

/// The player's answer for video `id`, asked as `asked_as`; an error unless it plays.
async fn ask_player_as(
    client: &reqwest::Client,
    origin: &str,
    id: &str,
    facts: &PageFacts,
    asked_as: &PlayerClient,
) -> Result<Value, Error> {
    let mut who = serde_json::Map::new();
    for (key, value) in &asked_as.context {
        who.insert(key.clone(), Value::String(value.clone()));
    }
    for (key, value) in [
        ("clientName", &asked_as.name),
        ("clientVersion", &asked_as.version),
        ("userAgent", &asked_as.agent),
    ] {
        who.insert(key.into(), Value::String(value.clone()));
    }
    who.insert("hl".into(), json!("en"));
    who.insert("timeZone".into(), json!("UTC"));
    who.insert("utcOffsetMinutes".into(), json!(0));
    let body = json!({
        "context": {"client": who},
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
        .header(reqwest::header::USER_AGENT, &asked_as.agent)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("X-Youtube-Client-Name", &asked_as.id)
        .header("X-Youtube-Client-Version", &asked_as.version)
        .header(reqwest::header::ORIGIN, origin);
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
    Ok(player)
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
    /// Served only as numbered pieces (`FORMAT_STREAM_TYPE_OTF`: `&sq=0` its first boxes, then
    /// `&sq=1` on), not as one file of byte ranges: fetched by the DASH piece fetcher (gaps row
    /// Y14), and never cut into a part, having no index.
    numbered: bool,
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
        // A locked (DRM) stream, a live one. One served only as numbered pieces is read, and
        // fetched piece by piece (gaps row Y14): it was left out, and a video served that way
        // lost its higher qualities.
        if f.get("drmFamilies").is_some() || f.get("targetDurationSec").is_some() {
            return None;
        }
        let numbered = f.get("type").and_then(Value::as_str) == Some("FORMAT_STREAM_TYPE_OTF");
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
            numbered,
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

tokio::task_local! {
    /// Set while a video is read with its formats served only as numbered pieces left out
    /// ([`read_for_without_numbered`]).
    static WITHOUT_NUMBERED: ();
}

/// [`read_for`] with the formats served only as numbered pieces left out, as they were before
/// dm fetched them (gaps row Y14): what a download falls back to where their first piece does
/// not say how many follow, or none of them could be fetched.
pub async fn read_for_without_numbered(
    url: &str,
    options: &crate::download::Options,
    staging: &Path,
) -> Result<String, Error> {
    WITHOUT_NUMBERED
        .scope((), read_for(url, options, staging))
        .await
}

/// [`read`] for a download's options: through the proxy they name, at the
/// quality and in the container they ask, with the session they carry.
pub async fn read_for(
    url: &str,
    options: &crate::download::Options,
    staging: &Path,
) -> Result<String, Error> {
    // Through the proxy named, a client of its own; else the program's shared one (gaps row
    // S14), not a new one for every video read.
    let client = match options.proxy_for(url) {
        Some(proxy) => crate::builder()
            .proxy(reqwest::Proxy::all(proxy)?)
            .build()?,
        None => crate::client(),
    };
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

    // Each client in turn until one answers with streams dm can fetch: a client YouTube
    // stops serving, or whose addresses this player's checks could not be answered for
    // (gaps row Y3), is passed over for the next.
    let mut failed = None;
    let mut found = None;
    for asked_as in &player_clients() {
        let player = match ask_player_as(client, &origin, id, &facts, asked_as).await {
            Ok(player) => player,
            Err(e) => {
                failed = Some(e);
                continue;
            }
        };
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
        let without_numbered = WITHOUT_NUMBERED.try_with(|()| ()).is_ok();
        let streams_at = |at: &str| -> Vec<Format> {
            player
                .pointer(at)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Format::of)
                .filter(|f| !(without_numbered && f.numbered))
                .collect()
        };
        let mut formats = streams_at("/streamingData/adaptiveFormats");
        let muxed = formats.is_empty();
        if muxed {
            formats = streams_at("/streamingData/formats");
        }
        set_dubs_aside(&mut formats);
        if formats.is_empty() {
            failed = Some(Error::Internal(
                "the player names no stream dm can fetch".into(),
            ));
            continue;
        }

        // The solver, for the `n` value every address carries and the cipher
        // some are locked behind.
        let needs = formats
            .iter()
            .any(|f| f.cipher.is_some() || f.url.as_deref().is_some_and(has_n));
        if needs {
            if script.is_none() {
                script = Some(fetch_script().await?);
            }
            let solved = match solve(script.as_deref().unwrap_or_default(), &formats).await {
                Ok(solved) => solved,
                // Not answered for this client's addresses: the player's id noted (the window
                // asks for fresh rules by it) and the next client asked, whose answer may want
                // no answer, or one this player gives.
                Err(e) => {
                    note_unsolved(js_path.as_deref());
                    failed = Some(e);
                    continue;
                }
            };
            for format in &mut formats {
                format.url = Some(unlocked(format, &solved)?);
                format.cipher = None;
            }
        }
        found = Some((player, formats, muxed));
        break;
    }
    let (player, formats, muxed) = found.ok_or_else(|| {
        failed.unwrap_or_else(|| Error::Internal("no client to ask YouTube's player as".into()))
    })?;

    // A part is cut from MP4 streams alone (their pieces are indexed): a
    // height only WebM has gives way to the tallest MP4.
    let cuttable: Vec<Format> = formats
        .iter()
        .filter(|f| !mp4_only || f.container == "mp4" || f.container == "m4a")
        // Nor from numbered pieces, which have no index to cut by.
        .filter(|f| !mp4_only || !f.numbered)
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
    // Its picture, the largest the player names, and the chapters its description lists,
    // written into the file (gaps row Y11).
    if let Some(picture) = largest_thumbnail(&player) {
        reading["thumbnail"] = json!(picture);
    }
    if let Some(day) = publish_date(&page) {
        reading["upload_date"] = json!(day);
    }
    let chapters = player
        .pointer("/videoDetails/shortDescription")
        .and_then(Value::as_str)
        .map(chapters_in)
        .unwrap_or_default();
    if !chapters.is_empty() {
        reading["chapters"] = Value::Array(
            chapters
                .into_iter()
                .map(|(start, title)| json!({ "start_time": start, "title": title }))
                .collect(),
        );
    }
    // The video's captions, for Options' "carry subtitles" to put inside it (gaps row Y6).
    let captions = captions_named(&player);
    if !captions.is_empty() {
        reading["requested_subtitles"] = Value::Object(captions);
    }
    // The languages YouTube translates them into, listed and not fetched.
    let translated = translations_named(&player);
    if !translated.is_empty() {
        reading["translated_subtitles"] = Value::Object(translated);
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
    let mut named = serde_json::Map::new();
    for track in captions_chosen(player) {
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

/// The caption tracks a reading takes, by the choice `captions_named` says, in the player's
/// order.
fn captions_chosen(player: &Value) -> Vec<&Value> {
    let tracks: Vec<&Value> = player
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let asr = |t: &&Value| t.get("kind").and_then(Value::as_str) == Some("asr");
    let written: Vec<&Value> = tracks.iter().copied().filter(|t| !asr(t)).collect();
    if written.is_empty() {
        tracks.into_iter().filter(asr).take(1).collect()
    } else {
        written
    }
}

/// The translated caption tracks the player offers (`translated_subtitles`: each language's
/// WebVTT address and name), listed only, never fetched: the first chosen track YouTube can
/// translate (`isTranslatable`), in each language the player's `translationLanguages` names
/// that no chosen track is already in, as YouTube's own `tlang`; at most 8, on YouTube's own
/// address alone (gaps row Y6).
fn translations_named(player: &Value) -> serde_json::Map<String, Value> {
    let chosen = captions_chosen(player);
    let mut named = serde_json::Map::new();
    let Some(base) = chosen
        .iter()
        .find(|t| t.get("isTranslatable").and_then(Value::as_bool) == Some(true))
        .and_then(|t| t.get("baseUrl").and_then(Value::as_str))
    else {
        return named;
    };
    let own: Vec<&str> = chosen
        .iter()
        .filter_map(|t| t.get("languageCode").and_then(Value::as_str))
        .collect();
    let languages = player
        .pointer("/captions/playerCaptionsTracklistRenderer/translationLanguages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for language in languages {
        if named.len() >= CAPTIONS_MOST {
            break;
        }
        let Some(code) = language.get("languageCode").and_then(Value::as_str) else {
            continue;
        };
        let plain = !code.is_empty()
            && code
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        if !plain || own.contains(&code) || named.contains_key(code) {
            continue;
        }
        let Ok(url) = caption_address(base, "vtt") else {
            return named;
        };
        let Ok(mut url) = url::Url::parse(&url) else {
            return named;
        };
        let kept: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| k != "tlang")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        url.query_pairs_mut()
            .clear()
            .extend_pairs(kept)
            .append_pair("tlang", code);
        let name = language
            .pointer("/languageName/simpleText")
            .and_then(Value::as_str)
            .unwrap_or(code);
        named.insert(
            code.to_string(),
            json!({ "url": url.to_string(), "ext": "vtt", "name": name }),
        );
    }
    named
}

/// A caption track's address in `format` (`vtt`, `ttml`): on YouTube's own `https` address
/// alone (or a test's stand-in, `DM_YOUTUBE_HOST`, named in full), its own `fmt` replaced;
/// else why not.
fn caption_address(base: &str, format: &str) -> Result<String, &'static str> {
    let mut url = url::Url::parse(base)
        .or_else(|_| url::Url::parse(&format!("https://www.youtube.com{base}")))
        .map_err(|_| "the captions' address is not one")?;
    let youtube = url.host_str() == Some("www.youtube.com") && url.scheme() == "https";
    let stand_in = std::env::var("DM_YOUTUBE_HOST")
        .ok()
        .filter(|v| !v.is_empty())
        .and_then(|v| url::Url::parse(&v).ok())
        .is_some_and(|o| o.origin() == url.origin());
    if !youtube && !stand_in {
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
        let mut described = json!({
            "format_id": f.itag.to_string(),
            "url": f.url,
            // Numbered pieces (`&sq=N`), fetched by the DASH piece fetcher; else one file.
            "protocol": if f.numbered { "http_dash_segments" } else { "https" },
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
        });
        // Numbered pieces state no size: about, from the bitrate and the video's length, for
        // the quality list's sizes.
        if let (true, None, Some(seconds)) = (f.numbered, f.size, duration) {
            described["filesize_approx"] = json!(f.bitrate / 8 * seconds);
        }
        described
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
mod tests;
