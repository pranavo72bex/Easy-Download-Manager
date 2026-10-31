//! A YouTube watch page read by dm itself, with no video helper.
//!
//! What yt-dlp does for such a page today, watched request by request
//! (`docs/decisions.md`, "How YouTube is got today"): the watch page as a
//! browser, for the visitor id, the player script's address and its
//! signature timestamp; the player API as a TV client, which answers with
//! plain `https` addresses for every picture and sound; the player script;
//! and one run of the `yt-dlp-ejs` solver scripts (public domain, fetched
//! from their releases and pinned by digest) in the QuickJS dm fetches,
//! which turns each address's `n` value into the one the server accepts.
//! The result is written in the shape of yt-dlp's own reading, so
//! `ytdlp::download_page` fetches and joins the streams as it does for one
//! of yt-dlp's. Anything this cannot read (a video wanting a sign-in, a
//! live stream, a changed client) goes to yt-dlp as before.
//!
//! Test knobs: `DM_YOUTUBE_HOST` names a stand-in for `www.youtube.com`
//! (a test's server), and `DM_EJS_RELEASES` a stand-in for the solver's
//! releases, whose files are then not held to the pinned digests.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde_json::{json, Value};

use crate::stream::Quality;
use crate::Error;

/// The solver's pinned release, and the digest of each of its two scripts.
const SOLVER_VERSION: &str = "0.8.0";
const SOLVER_LIB: (&str, &str) = (
    "yt.solver.lib.min.js",
    "c55987fe697e5b9ee18830163f7af85327e9bb5c3e674b969d38c8d205eaa577",
);
const SOLVER_CORE: (&str, &str) = (
    "yt.solver.core.min.js",
    "18da6ce0758b416e7ae645084f4f8801f9f9d59d6c477c05eaa0ff94ebd8cc00",
);

/// The TV client yt-dlp reads with today: its answers carry plain
/// addresses and want no proof-of-origin token.
const TV_CLIENT_NAME: &str = "TVHTML5";
const TV_CLIENT_VERSION: &str = "5.20260707";
const TV_CLIENT_ID: &str = "7";
const TV_AGENT: &str = "Mozilla/5.0 (SMART-TV; Linux; Tizen 2.4.0) AppleWebKit/538.1 (KHTML, like Gecko) Version/2.4.0 TV Safari/538.1";
/// The browser the watch page is asked as.
const PAGE_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";
/// The most one request for a stream asks for, as yt-dlp sets it for
/// these addresses (`http_chunk_size`): the server slows a response past it.
const CHUNK: u64 = 10 << 20;
/// How long the whole reading may take.
const WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

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
    pub id: String,
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
    pub id: String,
    pub title: String,
    pub entries: Vec<PlaylistEntry>,
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

/// The video's id in a watch, short, shorts, embed or live address.
pub fn video_id(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("m.").unwrap_or(host);
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
    /// A sound track that is not the video's own (a dub), or a loudness
    /// variant: never chosen, kept in the list.
    secondary: bool,
    /// Where the stream's first boxes and its index of pieces are
    /// (`initRange`, `indexRange`), for cutting a part of it (`clip`).
    init: Option<(u64, u64)>,
    index: Option<(u64, u64)>,
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
        let dubbed = f
            .pointer("/audioTrack/audioIsDefault")
            .and_then(Value::as_bool)
            .is_some_and(|default| !default);
        let drc = f.get("isDrc").and_then(Value::as_bool).unwrap_or(false);
        let range = |key: &str| {
            let r = f.get(key)?;
            let edge = |k: &str| {
                r.get(k)
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            };
            let (start, end) = (edge("start")?, edge("end")?);
            (end >= start).then_some((start, end))
        };
        Some(Format {
            itag: number("itag")?,
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
            width: number("width").unwrap_or(0),
            height: number("height").unwrap_or(0),
            bitrate: number("averageBitrate")
                .or_else(|| number("bitrate"))
                .unwrap_or(0),
            size: number("contentLength"),
            fps: number("fps").unwrap_or(0),
            secondary: dubbed || drc,
            init: range("initRange"),
            index: range("indexRange"),
        })
    }

    /// Its name in yt-dlp's words, which the row and the parts carry.
    fn ext(&self) -> &str {
        &self.container
    }
}

/// The picture and the sound (or the sound alone) for the quality asked,
/// chosen as `format_for` has yt-dlp choose: the biggest picture at or
/// under the quality, measured by its shorter side (a portrait video's
/// width: 720p is 720×1280 for a Short), in the container asked for where
/// there is one that size, with the best sound in the same container.
fn choose(formats: &[Format], quality: Quality, container: Option<&str>) -> Option<Vec<Format>> {
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
    let pictures: Vec<&Format> = usable
        .iter()
        .filter(|f| f.video && f.height > 0 && side(f) <= limit)
        .copied()
        .collect();
    let biggest = pictures.iter().map(|f| side(f)).max()?;
    // The container asked for at the biggest size, else whatever is biggest.
    let picture = pictures
        .iter()
        .filter(|f| side(f) == biggest && f.container == wanted)
        .max_by_key(|f| (f.height, f.fps, f.bitrate))
        .or_else(|| {
            pictures
                .iter()
                .filter(|f| side(f) == biggest)
                .max_by_key(|f| (f.container == "mp4", f.height, f.fps, f.bitrate))
        })?;
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
    read(
        &client,
        url,
        options.quality,
        container,
        part,
        options.sign_in.as_ref(),
        staging,
    )
    .await
}

/// A YouTube watch page read, as `ytdlp::download_page` wants a reading:
/// the JSON yt-dlp would have printed for it, chosen at `quality`.
pub async fn read(
    client: &reqwest::Client,
    url: &str,
    quality: Quality,
    container: Option<&str>,
    mp4_only: bool,
    sign_in: Option<&crate::cookies::SignIn>,
    staging: &Path,
) -> Result<String, Error> {
    let id = video_id(url).ok_or_else(|| Error::Internal("not a YouTube video address".into()))?;
    tokio::time::timeout(
        WITHIN,
        read_within(client, &id, quality, container, mp4_only, sign_in, staging),
    )
    .await
    .map_err(|_| Error::Internal("the page took too long to read".into()))?
}

async fn read_within(
    client: &reqwest::Client,
    id: &str,
    quality: Quality,
    container: Option<&str>,
    mp4_only: bool,
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
    let mut facts = page_facts(&page);
    // The player script, where its timestamp is wanted now (the page gave
    // none) and for the solver later.
    // Only YouTube's own player path is fetched and run: the solver runs
    // the player's code, so a page naming a script elsewhere is not
    // followed there.
    let js_url = facts
        .player_js
        .as_deref()
        .map(|named| {
            named
                .strip_prefix("https://www.youtube.com")
                .unwrap_or(named)
        })
        .filter(|path| player_path(path))
        .map(|path| format!("{origin}{path}"));
    let fetch_script = |url: String| async move {
        let script = crate::redirects::send(
            client
                .get(&url)
                .header(reqwest::header::USER_AGENT, PAGE_AGENT),
        )
        .await?;
        let text = text_capped(Error::refused_or(script).await?, PAGE_MOST).await?;
        Ok::<String, Error>(text)
    };
    let mut script: Option<String> = None;
    if facts.sts.is_none() {
        if let Some(url) = js_url.clone() {
            let text = fetch_script(url).await?;
            facts.sts = sts_in_player(&text);
            script = Some(text);
        }
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
        return Err(Error::Internal("a live stream".into()));
    }
    // The picture and the sound apart (`adaptiveFormats`), as yt-dlp's
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
            None => match js_url {
                Some(url) => fetch_script(url).await?,
                None => return Err(Error::Internal("the page names no player script".into())),
            },
        };
        let solved = solve(client, &script, &formats, staging).await?;
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
        .cloned()
        .collect();
    let chosen = if muxed {
        choose(&cuttable, quality, container).map(|mut pair| {
            pair.truncate(1);
            pair
        })
    } else {
        choose(&cuttable, quality, container)
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
    Ok(reading_json(id, &title, duration, &formats, &chosen, staging).to_string())
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

/// Runs the solver scripts over the player script for every `n` value and
/// signature the formats carry, in QuickJS (dm's own, or one installed), or
/// Deno where that is what is installed.
async fn solve(
    client: &reqwest::Client,
    player_js: &str,
    formats: &[Format],
    staging: &Path,
) -> Result<Solved, Error> {
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
    let mut requests = Vec::new();
    if !ns.is_empty() {
        requests.push(json!({"type": "n", "challenges": ns.iter().collect::<Vec<_>>()}));
    }
    if !sigs.is_empty() {
        requests.push(json!({"type": "sig", "challenges": sigs.iter().collect::<Vec<_>>()}));
    }
    let (lib, core) = solver_scripts(client).await?;
    let input = json!({"type": "player", "player": player_js, "requests": requests});
    let program = format!(
        "{lib}\nObject.assign(globalThis, lib);\n{core}\nconsole.log(JSON.stringify(jsc({input})));\n"
    );
    let output = run_js(&program, staging).await?;
    let answer: Value = serde_json::from_str(
        output
            .lines()
            .rev()
            .find(|line| line.trim_start().starts_with('{'))
            .unwrap_or(""),
    )
    .map_err(|_| Error::Internal(format!("the solver said: {}", output.trim())))?;
    if answer.get("type").and_then(Value::as_str) == Some("error") {
        let why = answer.get("error").and_then(Value::as_str).unwrap_or("");
        return Err(Error::Internal(format!("the solver failed: {why}")));
    }
    let mut solved = Solved::default();
    let responses = answer
        .get("responses")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Internal("the solver answered nothing".into()))?;
    for (request, response) in requests.iter().zip(responses) {
        let kind = request.get("type").and_then(Value::as_str).unwrap_or("");
        if response.get("type").and_then(Value::as_str) != Some("result") {
            let why = response.get("error").and_then(Value::as_str).unwrap_or("");
            return Err(Error::Internal(format!(
                "the solver could not solve {kind}: {why}"
            )));
        }
        let data = response.get("data").and_then(Value::as_object);
        let into = if kind == "n" {
            &mut solved.n
        } else {
            &mut solved.sig
        };
        for (challenge, result) in data.into_iter().flatten() {
            if let Some(result) = result.as_str() {
                into.insert(challenge.clone(), result.to_string());
            }
        }
    }
    Ok(solved)
}

/// The solver's two scripts, fetched once into dm's tools folder and held
/// to their digests, as QuickJS is.
async fn solver_scripts(client: &reqwest::Client) -> Result<(String, String), Error> {
    let folder = crate::tools::folder()
        .ok_or_else(|| Error::Internal("dm has no tools folder here".into()))?
        .join(format!("ejs-{SOLVER_VERSION}"));
    let stand_in = std::env::var("DM_EJS_RELEASES")
        .ok()
        .filter(|v| !v.is_empty());
    let base = stand_in.clone().unwrap_or_else(|| {
        format!("https://github.com/yt-dlp/ejs/releases/download/{SOLVER_VERSION}")
    });
    let mut texts = Vec::new();
    for (name, digest) in [SOLVER_LIB, SOLVER_CORE] {
        let path = folder.join(name);
        if let Ok(text) = tokio::fs::read_to_string(&path).await {
            if stand_in.is_some() || crate::tools::sha256_hex(text.as_bytes()) == digest {
                texts.push(text);
                continue;
            }
        }
        let bytes = crate::tools::fetch_bytes(client, &format!("{base}/{name}")).await?;
        if stand_in.is_none() && crate::tools::sha256_hex(&bytes) != digest {
            return Err(Error::Internal(format!(
                "the solver script {name} fetched is not the release dm knows, so it was not kept"
            )));
        }
        tokio::fs::create_dir_all(&folder).await?;
        let partial = path.with_extension("partial");
        tokio::fs::write(&partial, &bytes).await?;
        tokio::fs::rename(&partial, &path).await?;
        texts.push(String::from_utf8_lossy(&bytes).into_owned());
    }
    let core = texts.pop().unwrap_or_default();
    let lib = texts.pop().unwrap_or_default();
    Ok((lib, core))
}

/// Runs `program` in QuickJS (the one dm fetched, or one installed) or in
/// an installed Deno, and gives what it printed.
async fn run_js(program: &str, staging: &Path) -> Result<String, Error> {
    let quickjs = match crate::ytdlp::quickjs() {
        Some(found) => Some(found),
        None if crate::ytdlp::deno().is_none() => {
            crate::tools::ensure(&crate::client(), crate::tools::Tool::QuickJs)
                .await
                .ok()
        }
        None => None,
    };
    // In a file of this account's own: QuickJS does not read a program
    // from its standard input.
    tokio::fs::create_dir_all(staging).await?;
    let file = staging.join(format!(".dm-solver-{}.js", std::process::id()));
    crate::ytdlp::write_private(&file, program).await?;
    let _gone = crate::ytdlp::RemoveOnDrop(file.clone());
    // Deno first where it is installed: run with no permissions it can
    // reach neither files nor programs, which QuickJS's command line does
    // not prevent.
    let mut command = match (crate::ytdlp::deno(), quickjs) {
        (None, Some(qjs)) => {
            let mut c = tokio::process::Command::new(qjs);
            c.arg(&file);
            c
        }
        (Some(deno), _) => {
            let mut c = tokio::process::Command::new(deno);
            c.args([
                "run",
                "--ext=js",
                "--no-code-cache",
                "--no-prompt",
                "--no-remote",
                "--no-lock",
                "--no-config",
                "--no-npm",
            ])
            .arg(&file);
            c
        }
        (None, None) => {
            return Err(Error::Internal(
                "no JavaScript engine to run the player's challenges in".into(),
            ))
        }
    };
    crate::no_window_async(&mut command);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(45), command.output())
        .await
        .map_err(|_| Error::Internal("the solver took too long".into()))?
        .map_err(|e| Error::Internal(format!("the solver would not start: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!(
            "the solver stopped: {}",
            stderr.lines().last().unwrap_or("").trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The reading in yt-dlp's shape: what `ytdlp::download_page` reads of one
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

/// The title as yt-dlp's `%(title).200B` writes it in a file name: the
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
    fn locked_and_streamed_only_formats_are_left_out_and_the_rest_read() {
        let formats = formats();
        let itags: Vec<u64> = formats.iter().map(|f| f.itag).collect();
        assert_eq!(itags, [137, 248, 136, 140, 251, 140]);
        assert!(formats[5].secondary, "the loudness variant is not chosen");
        assert_eq!(formats[3].ext(), "m4a");
        assert_eq!(formats[1].ext(), "webm");
    }

    #[test]
    fn the_quality_asked_chooses_as_yt_dlp_would() {
        let formats = formats();
        let itags = |chosen: Option<Vec<Format>>| {
            chosen.map(|c| c.iter().map(|f| f.itag).collect::<Vec<_>>())
        };
        assert_eq!(
            itags(choose(&formats, Quality::Best, None)),
            Some(vec![137, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::Best, Some("webm"))),
            Some(vec![248, 251])
        );
        assert_eq!(
            itags(choose(&formats, Quality::UpTo(720), None)),
            Some(vec![136, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::Audio, None)),
            Some(vec![140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::Audio, Some("webm"))),
            Some(vec![251])
        );
        assert_eq!(itags(choose(&formats, Quality::UpTo(100), None)), None);
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
            itags(choose(&formats, Quality::UpTo(720), None)),
            Some(vec![302, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::UpTo(1080), None)),
            Some(vec![301, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::UpTo(480), None)),
            Some(vec![303, 140])
        );
        assert_eq!(
            itags(choose(&formats, Quality::Best, None)),
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

    #[test]
    fn the_reading_is_in_yt_dlps_shape() {
        let mut formats = formats();
        for f in &mut formats {
            f.url = Some("https://r1.example/x".into());
            f.cipher = None;
        }
        let chosen = choose(&formats, Quality::Best, None).unwrap();
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
        let sound = choose(&formats, Quality::Audio, Some("webm")).unwrap();
        let alone = reading_json("jNQXAC9IVRw", "x", None, &formats, &sound, Path::new("/s"));
        assert_eq!(alone["ext"], "webm");
        assert_eq!(alone["url"], "https://r1.example/x");
        assert!(alone.get("requested_formats").is_none());
        assert_eq!(safe_title(&"é".repeat(150)).len(), 200);
        assert_eq!(safe_title("  ... "), "video");
    }
}
