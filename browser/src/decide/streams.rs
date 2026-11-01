//! Media a page loaded, seen in the browser's responses.

use super::*;

/// One header of a `webRequest` response.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Header {
    pub name: String,
    pub value: Option<String>,
}

/// A response the browser received, as `webRequest.onResponseStarted`
/// reports it. Read for media, to find a player's stream on sites no page
/// reader knows.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Seen {
    pub url: String,
    pub tab_id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub status_code: u16,
    pub response_headers: Vec<Header>,
}

/// What kind of media a stream is, which decides how the app fetches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
pub enum StreamKind {
    /// An HLS playlist: the app fetches its segments and joins them.
    Hls,
    /// A DASH manifest: the app reads it, fetches picture and sound and
    /// joins them itself.
    Dash,
    /// A whole audio or video file.
    File,
    /// The picture of a video a player fetches apart from its sound: not
    /// offered alone, but with a sound as one line (`Stream::sound`).
    Picture,
    /// The sound that goes with a [`StreamKind::Picture`].
    Sound,
}

/// Media a page loaded, offered in the panel beside what the app reads.
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Stream {
    pub url: String,
    pub kind: StreamKind,
    pub size: Option<u64>,
    pub label: String,
    /// For a picture and a sound a player fetched apart, joined into one
    /// line of the list: the address of the sound, which dm fetches with the
    /// picture at `url` and joins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
}

/// A whole file smaller than this is a thumbnail, a sound effect or an ad
/// beacon, not something a person came to download.
pub const STREAM_FLOOR: u64 = 1024 * 1024;

/// The floor for a file the browser itself reports a `<video>` or `<audio>`
/// element fetching (`media`): a short clip a site plays as its post (X's
/// GIF-style `tweet_video`) is the thing a person came for, where the same
/// size loaded by a script is a beacon.
pub const MEDIA_FLOOR: u64 = 64 * 1024;

/// The media in a response, if it is a stream or a whole media file worth
/// offering. Pieces are not: an HLS or DASH segment, and a range of a
/// longer file that does not start at its beginning (a player fetching a
/// file in parts, as YouTube does), are parts of something else.
pub fn stream_seen(seen: &Seen) -> Option<Stream> {
    if seen.tab_id < 0 || !is_web(&seen.url) || !(200..300).contains(&seen.status_code) {
        return None;
    }
    let header = |name: &str| {
        seen.response_headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| h.value.as_deref())
            .map(str::trim)
    };
    let mime = header("content-type")
        .and_then(|v| v.split(';').next())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let path = seen
        .url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = path.rsplit('/').next().unwrap_or("").to_string();
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_string())
        .unwrap_or_default();

    // On the built-in list (an ad network, a beacon, a page's own sound, an
    // ad server by name, a Windows media stub): not offered, whatever it is.
    let declared = header("content-length").and_then(|v| v.parse::<u64>().ok());
    if super::excluded::response(&seen.url, &mime, header("server"), declared).is_some() {
        return None;
    }
    if let Some(track) = track_of(&seen.url) {
        return Some(track);
    }
    if let Some(track) = pieces_of(&seen.url, &mime) {
        return Some(track);
    }
    if mime.contains("mpegurl") || ext == "m3u8" {
        return Some(Stream {
            url: seen.url.clone(),
            kind: StreamKind::Hls,
            size: None,
            label: hls_label(&path),
            sound: None,
        });
    }
    if mime.contains("dash+xml") || ext == "mpd" {
        return Some(Stream {
            url: seen.url.clone(),
            kind: StreamKind::Dash,
            size: None,
            label: "DASH stream".into(),
            sound: None,
        });
    }

    let media = mime.starts_with("video/") || mime.starts_with("audio/");
    let segment = matches!(ext.as_str(), "ts" | "m4s" | "m4f" | "cmfv" | "cmfa")
        || mime == "video/mp2t"
        || mime.contains("iso.segment")
        // Players that ask for a file in pieces name the piece in the
        // address; the piece is not the file.
        || seen.url.contains("range=")
        || seen.url.contains("&rn=");
    if !media || segment {
        return None;
    }
    // The whole length: a range answer says it after the slash, and is the
    // file only if it starts at its first byte.
    let size = match header("content-range") {
        Some(range) => {
            let (span, total) = range.trim_start_matches("bytes").trim().split_once('/')?;
            if !span.trim().starts_with("0-") {
                return None;
            }
            total.trim().parse::<u64>().ok()
        }
        None => header("content-length").and_then(|v| v.parse::<u64>().ok()),
    };
    let floor = if seen.kind == "media" {
        MEDIA_FLOOR
    } else {
        STREAM_FLOOR
    };
    if size.is_some_and(|bytes| bytes < floor) {
        return None;
    }
    let format = if ext.is_empty() || ext.len() > 5 {
        mime.split('/')
            .nth(1)
            .unwrap_or("media")
            .to_ascii_uppercase()
    } else {
        ext.to_ascii_uppercase()
    };
    let kind = if mime.starts_with("audio/") {
        "audio"
    } else {
        "video"
    };
    Some(Stream {
        url: seen.url.clone(),
        kind: StreamKind::File,
        size,
        label: format!("{format} {kind} file"),
        sound: None,
    })
}

/// Picture and sound itags of a video site's player, for the label and for a
/// response that names no type: the picture's height, or 0 for a sound.
fn itag_height(itag: u32) -> Option<u32> {
    Some(match itag {
        160 | 278 | 394 => 144,
        133 | 242 | 395 => 240,
        134 | 243 | 396 => 360,
        135 | 244 | 397 => 480,
        136 | 247 | 298 | 398 => 720,
        137 | 248 | 299 | 399 => 1080,
        264 | 271 | 400 => 1440,
        266 | 313 | 401 => 2160,
        139 | 140 | 141 | 249 | 250 | 251 => 0,
        _ => return None,
    })
}

/// One query parameter of an address, percent-decoded where it is a type.
fn param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next()?;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.replace("%2F", "/").replace("%2f", "/"))
    })
}

/// A picture or a sound of a player that fetches each as a whole file in
/// pieces, read from the address of any of its requests: the file's own
/// address (the player's per-piece `range`, `rn` and `rbuf` taken off, which
/// name the piece and not the file), what it is and how long it is.
pub fn track_of(url: &str) -> Option<Stream> {
    let (host, rest) = url.split_once("://")?.1.split_once('/')?;
    let (path, query) = rest.split_once('?')?;
    if !(host == "googlevideo.com" || host.ends_with(".googlevideo.com"))
        || !path.ends_with("videoplayback")
    {
        return None;
    }
    let itag = param(url, "itag").and_then(|v| v.parse::<u32>().ok());
    let kind_word = param(url, "mime").unwrap_or_default().to_ascii_lowercase();
    let height = itag.and_then(itag_height);
    let kind = if kind_word.starts_with("video/") || height.is_some_and(|h| h > 0) {
        StreamKind::Picture
    } else if kind_word.starts_with("audio/") || height == Some(0) {
        StreamKind::Sound
    } else {
        return None;
    };
    let kept: Vec<&str> = query
        .split('#')
        .next()
        .unwrap_or("")
        .split('&')
        .filter(|pair| {
            let name = pair.split('=').next().unwrap_or("");
            !matches!(name, "range" | "rn" | "rbuf")
        })
        .collect();
    let whole = format!("https://{host}/{path}?{}", kept.join("&"));
    let label = match (kind, height) {
        (StreamKind::Picture, Some(h)) if h > 0 => format!("{h}p video"),
        (StreamKind::Picture, _) => "video".to_string(),
        _ => "sound".to_string(),
    };
    Some(Stream {
        url: whole,
        kind,
        size: param(url, "clen").and_then(|v| v.parse().ok()),
        label,
        sound: None,
    })
}

/// What a playlist's address says of it, for its line in the list: the
/// height of a rendition named in the path (`/pl/avc1/1280x720/x.m3u8`, a
/// portrait clip's smaller side), or that it carries only sound (`/mp4a/`,
/// `/audio/`). A master playlist, or one that says nothing, is just a stream.
fn hls_label(path: &str) -> String {
    let mut height = None;
    for part in path.split('/') {
        if part == "mp4a" || part == "audio" || part.starts_with("audio_") {
            return HLS_SOUND.into();
        }
        if let Some((w, h)) = part.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                if (100..=8000).contains(&w) && (100..=8000).contains(&h) {
                    height = Some(w.min(h));
                }
            }
        }
    }
    match height {
        Some(h) => format!("{h}p HLS stream"),
        None => "HLS stream".into(),
    }
}

/// The label of a playlist that carries only sound.
const HLS_SOUND: &str = "HLS sound";

/// Standard base64, either alphabet, padding or none.
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut have) = (0u32, 0u32);
    for c in text.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(v);
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have) as u8);
            bits &= (1 << have) - 1;
        }
    }
    Some(out)
}

/// A picture or a sound of a player that asks a social site's CDN
/// (`fbcdn.net`, `cdninstagram.com`: Facebook, Instagram, Threads) for each
/// as a file in pieces, each piece named in the address by `bytestart` and
/// `byteend`. The file is the address without those two, which the CDN
/// answers whole. What a piece is — picture or sound, and at what height —
/// is in the `efg` parameter, a base64 JSON with the encoder's tag
/// (`…dash_ln_heaac_vbr3_audio`, `…dash_h264-basic-gen2_720p_…`); the
/// response's type is the fallback.
///
/// The size is left unknown (the pieces seen so far are not the file), so
/// of several renditions the latest seen is the line's picture.
pub fn pieces_of(url: &str, mime: &str) -> Option<Stream> {
    let (authority, rest) = url.split_once("://")?.1.split_once('/')?;
    let host = authority.split(':').next().unwrap_or(authority);
    if !(host.ends_with(".fbcdn.net")
        || host == "fbcdn.net"
        || host.ends_with(".cdninstagram.com")
        || host == "cdninstagram.com")
    {
        return None;
    }
    let (path, query) = rest.split_once('?')?;
    let query = query.split('#').next().unwrap_or("");
    let names = |pair: &str| pair.split('=').next().unwrap_or("").to_string();
    if !query
        .split('&')
        .any(|p| matches!(names(p).as_str(), "bytestart" | "byteend"))
    {
        return None;
    }
    let tag = param(url, "efg")
        .and_then(|efg| unbase64(&efg))
        .and_then(|json| serde_json::from_slice::<serde_json::Value>(&json).ok())
        .and_then(|v| {
            v.get("vencode_tag")
                .and_then(|t| t.as_str().map(str::to_owned))
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    let sound = if tag.is_empty() {
        mime.starts_with("audio/")
    } else {
        tag.contains("audio") || tag.contains("heaac") || tag.ends_with("_aac")
    };
    let height = tag
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter_map(|word| word.strip_suffix('p'))
        .filter_map(|n| n.parse::<u32>().ok())
        .find(|h| (144..=4320).contains(h));
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !matches!(names(pair).as_str(), "bytestart" | "byteend"))
        .collect();
    let whole = if kept.is_empty() {
        format!("https://{authority}/{path}")
    } else {
        format!("https://{authority}/{path}?{}", kept.join("&"))
    };
    let (kind, label) = match (sound, height) {
        (true, _) => (StreamKind::Sound, "sound".to_string()),
        (false, Some(h)) => (StreamKind::Picture, format!("{h}p video")),
        (false, None) => (StreamKind::Picture, "video".to_string()),
    };
    Some(Stream {
        url: whole,
        kind,
        size: None,
        label,
        sound: None,
    })
}

/// Whether a response is a new page in its tab, whose streams start afresh:
/// a top-level document. A download opened in the tab (`main_frame` too,
/// but a file and not a page) leaves the page, and its streams, in place.
pub fn new_page(seen: &Seen) -> bool {
    if seen.kind != "main_frame" || seen.tab_id < 0 {
        return false;
    }
    let header = |name: &str| {
        seen.response_headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| h.value.as_deref())
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    if header("content-disposition").starts_with("attachment") {
        return false;
    }
    let mime = header("content-type");
    let essence = mime.split(';').next().unwrap_or("").trim();
    essence.is_empty() || essence.contains("html") || essence.contains("xml")
}

/// What a response means for the copy of a tab's streams kept in the
/// session's storage, which outlives the worker that heard them.
#[derive(Debug, Clone, PartialEq)]
pub enum Keep {
    /// A new page: the old page's copy is removed, whatever this worker
    /// remembers — a worker woken by the navigation remembers nothing, and
    /// the copy would otherwise be offered on the new page.
    Forget,
    /// A stream this tab has not been heard to load: the kept copy is read
    /// into memory before it is added, and the merge written back.
    Add,
    /// Nothing to keep.
    Nothing,
}

/// What to do with a response, for the kept copy.
pub fn keep(seen: &Seen) -> Keep {
    if new_page(seen) {
        Keep::Forget
    } else if stream_seen(seen).is_some() {
        Keep::Add
    } else {
        Keep::Nothing
    }
}

/// Which page each tab is on, counted, so a stream heard on one page whose
/// storage read is still in flight when the tab navigates is dropped rather
/// than written back over the new page's empty list. Responses are handled
/// concurrently: an `Add` awaits the kept copy, and a `Forget` for the next
/// page can run in that wait.
#[derive(Debug, Default)]
pub struct Pages {
    by_tab: std::collections::HashMap<i64, u64>,
}

impl Pages {
    /// The page a tab is on now.
    pub fn now(&self, tab: i64) -> u64 {
        self.by_tab.get(&tab).copied().unwrap_or(0)
    }

    /// A new page, or the tab closed: whatever was begun on the old one is
    /// stale from here.
    pub fn next(&mut self, tab: i64) {
        let n = self.now(tab).wrapping_add(1);
        self.by_tab.insert(tab, n);
    }

    /// Whether work begun on page `then` of a tab is still for its page.
    pub fn still(&self, tab: i64, then: u64) -> bool {
        self.now(tab) == then
    }
}

/// The streams each tab has loaded, newest last, a few per tab.
#[derive(Default, Debug)]
pub struct Streams {
    by_tab: std::collections::HashMap<i64, Vec<Stream>>,
}

/// How many streams a tab keeps: a page that loads more is cycling through
/// renditions or adverts, and the first and latest few are what matter.
pub const STREAMS_PER_TAB: usize = 8;

impl Streams {
    /// Takes in one response: a new page in a tab forgets the old page's
    /// streams, and media is remembered once per address.
    pub fn absorb(&mut self, seen: &Seen) {
        if new_page(seen) {
            self.by_tab.remove(&seen.tab_id);
            return;
        }
        let Some(stream) = stream_seen(seen) else {
            return;
        };
        let list = self.by_tab.entry(seen.tab_id).or_default();
        if list.iter().any(|known| known.url == stream.url) {
            return;
        }
        list.push(stream);
        if list.len() > STREAMS_PER_TAB {
            // The first (a master playlist, usually) stays; the oldest of
            // the rest goes.
            list.remove(1);
        }
    }

    pub fn forget(&mut self, tab: i64) {
        self.by_tab.remove(&tab);
    }

    /// Whether this worker has heard anything of a tab since it started.
    pub fn knows(&self, tab: i64) -> bool {
        self.by_tab.contains_key(&tab)
    }

    /// A tab's streams as kept before the worker last stopped, merged with
    /// what this worker has heard of the tab since: the kept ones first (a
    /// master playlist among them), then the new, each address once, as
    /// many as a tab keeps. Merging rather than replacing means a worker
    /// woken mid-page, which first hears only a variant playlist, neither
    /// loses the master to it nor has it lost again when the two are read
    /// in the other order.
    pub fn restore(&mut self, tab: i64, kept: Vec<Stream>) {
        let heard = self.by_tab.remove(&tab).unwrap_or_default();
        let mut list: Vec<Stream> = Vec::new();
        for stream in kept.into_iter().chain(heard) {
            if list.iter().any(|known| known.url == stream.url) {
                continue;
            }
            list.push(stream);
            if list.len() > STREAMS_PER_TAB {
                list.remove(1);
            }
        }
        self.by_tab.insert(tab, list);
    }

    /// A tab's streams as remembered, newest last, for keeping.
    pub fn raw(&self, tab: i64) -> Vec<Stream> {
        self.by_tab.get(&tab).cloned().unwrap_or_default()
    }

    /// What a tab has loaded, for the panel's list: manifests first, which
    /// are whole videos, then files, largest first. Two of a kind are told
    /// apart by their file name.
    pub fn of(&self, tab: i64) -> Vec<Stream> {
        let mut list = self.by_tab.get(&tab).cloned().unwrap_or_default();
        // A picture and a sound a player fetched apart are one line, with
        // the largest of each: the best the page has loaded.
        let largest = |list: &[Stream], kind: StreamKind| {
            list.iter()
                .filter(|s| s.kind == kind)
                .max_by_key(|s| s.size.unwrap_or(0))
                .cloned()
        };
        let (picture, sound) = (
            largest(&list, StreamKind::Picture),
            largest(&list, StreamKind::Sound),
        );
        list.retain(|s| !matches!(s.kind, StreamKind::Picture | StreamKind::Sound));
        // A playlist of sound alone is not offered beside a stream with a
        // picture: it would save the sound and nothing else.
        if list
            .iter()
            .any(|s| s.kind == StreamKind::Hls && s.label != HLS_SOUND)
        {
            list.retain(|s| !(s.kind == StreamKind::Hls && s.label == HLS_SOUND));
        }
        if let (Some(picture), Some(sound)) = (picture, sound) {
            list.push(Stream {
                url: picture.url,
                kind: StreamKind::File,
                size: picture.size.zip(sound.size).map(|(a, b)| a + b),
                label: format!("{} + sound", picture.label),
                sound: Some(sound.url),
            });
        }
        list.sort_by_key(|s| {
            (
                s.kind == StreamKind::File,
                std::cmp::Reverse(s.size.unwrap_or(0)),
            )
        });
        let labels: Vec<String> = list.iter().map(|s| s.label.clone()).collect();
        for stream in &mut list {
            if labels.iter().filter(|l| **l == stream.label).count() > 1 {
                let name = stream
                    .url
                    .split(['?', '#'])
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(40)
                    .collect::<String>();
                if !name.is_empty() {
                    stream.label = format!("{} · {name}", stream.label);
                }
            }
        }
        list
    }
}
