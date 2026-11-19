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

mod captions;
mod chapters;
mod choosing;
mod clients;
mod itags;
mod kept;
mod player;
mod playlist;
mod reading;
mod solving;

use crate::stream::Quality;
use crate::Error;
use captions::{caption_reading, captions_named, translations_named};
use chapters::{chapters_in, largest_thumbnail, publish_date};
#[cfg(test)]
use choosing::{choose, xtags};
use choosing::{choose_named, ended_stream_playlist, played_from_playlist, set_dubs_aside};
pub(crate) use clients::on_stand_in;
#[cfg(test)]
use clients::{ask_player, sapisid_hash};
use clients::{
    note_unsolved, origin, player_answer, player_clients, plays, signed_in_clients, wants_sign_in,
};
pub use clients::{
    served_player_clients, set_player_clients, take_unsolved_player, unsolved_players, PlayerClient,
};
pub use kept::kept_players;
#[doc(hidden)]
pub use kept::set_kept_players;
use kept::{cut_program, keep_script, kept_script};
#[cfg(test)]
use playlist::{channel_id_in, gather, playlist_title};
pub use playlist::{
    channel_of, is_list, playlist_id, read_playlist, Channel, Playlist, PlaylistEntry,
};
use reading::{
    chosen_of, cuttable_of, fetch_script, formats_of, player_script, reading_of, watch_page,
};
pub use solving::answers;
use solving::{has_n, plain_build, reading_json, solve, unescaped, unlocked};

/// The TV client this reads with today: its answers carry plain
/// addresses and want no proof-of-origin token.
const TV_CLIENT_NAME: &str = "TVHTML5";
const TV_CLIENT_VERSION: &str = "5.20260707";
const TV_CLIENT_ID: &str = "7";
const TV_AGENT: &str = "Mozilla/5.0 (SMART-TV; Linux; Tizen 2.4.0) AppleWebKit/538.1 (KHTML, like Gecko) Version/2.4.0 TV Safari/538.1";
/// The browser the watch page is asked as.
const PAGE_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";
/// The web client's version where the page names none (a stand-in's).
const WEB_CLIENT_VERSION: &str = "2.20261020.00.00";
/// Where YouTube's web clients say they ask from: a signed-in request's `Origin` and the
/// origin its `SAPISIDHASH` is made with (gaps row Y4).
const WEB_ORIGIN: &str = "https://www.youtube.com";
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

/// The browser's session for a signed-in ask of the player: the cookies the page handed over
/// (or the cookies file holds) for YouTube, with the `SAPISID` its hash is made from. Held in
/// memory for the one read it is for; never written to a file or a log (gaps row Y4).
struct Session {
    cookie: String,
    sapisid: String,
}

impl Session {
    /// The session for the player at `origin`, where its cookies name a `SAPISID` (else
    /// `__Secure-3PAPISID`, the name newer sessions give the same value).
    fn for_player(sign_in: Option<&crate::cookies::SignIn>, origin: &str) -> Option<Session> {
        let theirs = crate::cookies::header_with(sign_in, &format!("{origin}/youtubei/v1/player"))?;
        let named = |wanted: &str| {
            theirs
                .split(';')
                .filter_map(|pair| pair.trim().split_once('='))
                .find(|(name, _)| name.trim() == wanted)
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let sapisid = named("SAPISID").or_else(|| named("__Secure-3PAPISID"))?;
        Some(Session {
            cookie: format!("{theirs}; PREF=hl=en&tz=UTC; SOCS=CAI"),
            sapisid,
        })
    }
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
    /// The language a sound track speaks (`xtags`' `lang`, else its track id's `es-US` of
    /// `es-US.4`), where it names one; for a sound-language choice (gaps row Y10).
    lang: Option<String>,
    /// A described track (`acont=descriptive`), taken in its language only where no plain one is.
    described: bool,
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

tokio::task_local! {
    /// Set while a video is read with its formats served only as numbered pieces left out
    /// ([`read_for_without_numbered`]).
    static WITHOUT_NUMBERED: ();
    /// The format number of the picture a list line was built from, while a download reads its
    /// video ([`read_for`], `Options::format`; gaps row Y9).
    static FORMAT_ASKED: u64;
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
    let reading = read_at(
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
    );
    match options.format {
        Some(format) => FORMAT_ASKED.scope(format, reading).await,
        None => reading.await,
    }
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
    let page = watch_page(client, &origin, id, sign_in).await?;
    // A caption line: the track the page names, as a TTML file of its own (gaps row 72c).
    if let Some((language, asr)) = caption {
        return caption_reading(id, &page, &language, asr, staging);
    }
    let mut facts = page_facts(&page);
    // The player script, where its timestamp is wanted now (the page gave
    // none) and for the solver later.
    let (js_path, js_urls) = player_script(&facts, &origin);
    let mut script: Option<String> = None;
    if facts.sts.is_none() && !js_urls.is_empty() {
        let text = fetch_script(client, &js_urls).await?;
        facts.sts = sts_in_player(&text);
        script = Some(text);
    }

    // Each client in turn until one answers with streams dm can fetch: a client YouTube
    // stops serving, or whose addresses this player's checks could not be answered for
    // (gaps row Y3), is passed over for the next.
    // A video that wants a sign-in is asked again, with the browser's session where the page
    // handed one over (gaps row Y4); without one, the refusal stands as YouTube words it.
    let session = Session::for_player(sign_in, &origin);
    let mut asks: Vec<(PlayerClient, bool)> =
        player_clients().into_iter().map(|c| (c, false)).collect();
    let mut at = 0;
    let mut failed = None;
    let mut found = None;
    while let Some((asked_as, signed)) = asks.get(at).cloned() {
        at += 1;
        let with = session.as_ref().filter(|_| signed);
        let answer = player_answer(client, &origin, id, &facts, &asked_as, with).await;
        if let Ok(refused) = &answer {
            if session.is_some() && !asks.iter().any(|(_, s)| *s) && wants_sign_in(refused) {
                asks.extend(signed_in_clients(&page).into_iter().map(|c| (c, true)));
            }
        }
        let player = match answer.and_then(plays) {
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
            return Ok(played_from_playlist(id, &player, live, staging));
        }
        let (mut formats, muxed) = formats_of(&player);
        if formats.is_empty() {
            // A stream that has just ended, while YouTube makes the video of it: its streams are
            // still listed as a live one's, so none is read, and its HLS playlist plays it
            // (gaps row Y5). Fetched whole where the playlist says it has ended, recorded as
            // it goes where it does not, as a live one is.
            if let Some(playlist) = ended_stream_playlist(&player) {
                return Ok(played_from_playlist(id, &player, playlist, staging));
            }
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
                script = Some(fetch_script(client, &js_urls).await?);
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

    let cuttable = cuttable_of(&formats, mp4_only, codec)?;
    let chosen = chosen_of(id, &cuttable, muxed, quality, container, frame_rate)?;
    Ok(reading_of(
        id,
        &page,
        &player,
        &formats,
        &chosen,
        staging,
        js_path.as_deref(),
    ))
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

/// What the solver answered: each `n` value and each signature, by the
/// value asked about.
#[derive(Debug, Default)]
struct Solved {
    n: HashMap<String, String>,
    sig: HashMap<String, String>,
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
