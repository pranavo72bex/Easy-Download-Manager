// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Media a page loaded, seen in the browser's responses.

mod manifests;
mod ranges;

pub use manifests::{
    dash_label, dash_length, hls_length, hls_master_label, hls_renditions, length_words, Rendition,
    PLAYLIST_READ_CAP,
};
pub use ranges::{mp4_track, pieces_of, ranged_of, whole_by_ranges, TRACK_READ_CAP};
use ranges::{ranged_line, youtube_edge};

use super::*;

/// One header of a `webRequest` response.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Header {
    /// The header's name.
    pub name: String,
    /// The header's value, where it has one.
    pub value: Option<String>,
}

/// A response the browser received, as `webRequest.onResponseStarted`
/// reports it. Read for media, to find a player's stream on sites no page
/// reader knows.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Seen {
    /// The response's address.
    pub url: String,
    /// The browser tab it was loaded for.
    pub tab_id: i64,
    /// What kind of request it answered, as the browser names it.
    #[serde(rename = "type")]
    pub kind: String,
    /// The HTTP status code.
    pub status_code: u16,
    /// The response's headers.
    pub response_headers: Vec<Header>,
    /// The origin of the page that made the request (Chrome and Edge).
    pub initiator: String,
    /// The address of the page that made the request (Firefox).
    pub origin_url: String,
}

impl Seen {
    /// The page that made the request, as the browser says it: Chrome's
    /// `initiator`, or Firefox's `originUrl`.
    pub fn requester(&self) -> &str {
        if self.initiator.is_empty() {
            &self.origin_url
        } else {
            &self.initiator
        }
    }
}

/// Whether the worker may fetch `target` again itself, with the browser's
/// session, for what it says of its qualities: always on the public web, and
/// on this computer or a private network (`localhost`, `127.0.0.1`,
/// `192.168.1.1`, a link-local or unique-local address) only for a request
/// that came from that very host, `from` (the page that loaded a playlist,
/// or the playlist that named a rendition). Otherwise any page could load a
/// "playlist" from a router's or a printer's address and have the extension
/// ask it again with no CORS and no private-network check (security review).
pub fn may_reread(target: &str, from: &str) -> bool {
    let host = edm_protocol::host_of(target);
    !inside(&host) || edm_protocol::host_of(from) == host
}

/// An IPv4 address on this computer, a private or link-local network, or a provider's shared
/// space (carrier-grade NAT): not the web. The same hosts as edm-core's `redirects::inward`.
fn inside_v4(v4: std::net::Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || (a == 100 && (64..128).contains(&b))
}

/// A host on this computer or a private network.
fn inside(host: &str) -> bool {
    if host.is_empty() {
        return true;
    }
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".lan")
        || host.ends_with(".home.arpa")
    {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => inside_v4(v4),
        Ok(std::net::IpAddr::V6(v6)) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return inside_v4(v4);
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
        Err(_) => false,
    }
}

pub use edm_protocol::media::{
    label_height, panel_list, Stream, StreamKind, CLIP_FLOOR, DASH_PLAIN, DASH_SOUND, HLS_PLAIN,
    HLS_SOUND, RANGED_PICTURE, RANGED_SOUND,
};

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
    if let Some(track) = ranged_of(&seen.url, &mime, header("content-range")) {
        return Some(track);
    }
    // `application/vnd.apple.mpegurl`, `audio/x-mpegurl` and the `application/octet-stream-m3u8`
    // some servers send.
    if mime.contains("mpegurl") || mime.contains("m3u8") || ext == "m3u8" {
        return Some(Stream {
            url: seen.url.clone(),
            kind: StreamKind::Hls,
            size: None,
            label: hls_label(&path),
            sound: None,
            members: Vec::new(),
            length: None,
            by_ranges: false,
        });
    }
    // `application/dash+xml` and `video/vnd.mpeg.dash.mpd`.
    if mime.contains("dash+xml") || mime.contains("dash.mpd") || ext == "mpd" {
        return Some(Stream {
            url: seen.url.clone(),
            kind: StreamKind::Dash,
            size: None,
            label: "DASH stream".into(),
            sound: None,
            members: Vec::new(),
            length: None,
            by_ranges: false,
        });
    }

    let media = mime.starts_with("video/") || mime.starts_with("audio/");
    // HDS (Adobe's old Flash streaming: a `.f4m` manifest and `Seg1-Frag1` fragments) is not
    // read: neither is a file, and offering them would save a few bytes of XML (docs/decisions.md).
    let segment = matches!(ext.as_str(), "ts" | "m4s" | "m4f" | "cmfv" | "cmfa" | "f4m" | "f4f")
        || mime == "video/f4m"
        || mime == "video/f4f"
        || mime == "video/mp2t"
        || mime == "audio/mp2t"
        || mime.contains("iso.segment")
        // Players that ask for a file in pieces name the piece in the
        // address; the piece is not the file.
        || seen.url.contains("range=")
        || seen.url.contains("&rn=");
    // What is dropped here may still be a whole file fetched by ranges its request's header
    // asked for (an `.m4s` that is a picture or a sound whole, gaps row 155).
    let by_ranges = || whole_by_ranges(&seen.url, &mime, seen.status_code, header("content-range"));
    if !media || segment {
        return by_ranges();
    }
    // The whole length: a range answer says it after the slash, and is the
    // file only if it starts at its first byte.
    let size = match header("content-range") {
        Some(range) => {
            let (span, total) = range.trim_start_matches("bytes").trim().split_once('/')?;
            if !span.trim().starts_with("0-") {
                return by_ranges();
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
        members: Vec::new(),
        length: None,
        by_ranges: false,
    })
}

/// Picture and sound itags of a video site's player, for the label and for a
/// response that names no type: the picture's height, or 0 for a sound.
fn itag_height(itag: u32) -> Option<u32> {
    Some(match itag {
        // The same formats as the app's own table (`edm-core`'s `youtube/itags.rs`): H.264, VP9,
        // AV1, their 60-frame, HDR and 8K ones (gaps row Y14).
        160 | 278 | 394 | 330 | 694 => 144,
        133 | 242 | 395 | 331 | 695 => 240,
        134 | 243 | 396 | 332 | 696 => 360,
        135 | 244 | 397 | 333 | 697 => 480,
        136 | 247 | 298 | 302 | 398 | 334 | 698 => 720,
        137 | 248 | 299 | 303 | 399 | 335 | 699 => 1080,
        264 | 271 | 304 | 308 | 400 | 336 | 700 => 1440,
        266 | 305 | 313 | 315 | 401 | 337 | 701 => 2160,
        272 | 571 | 702 => 4320,
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
    // A host is letters, digits, dots and hyphens and nothing else: a message a
    // page posts itself is not read through a URL parser, and a `#`, a backslash, an `@` or a `:`
    // in the part before the first slash would name another host than the one
    // the suffix test sees.
    let plain = host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !plain || !youtube_edge(host) || !path.ends_with("videoplayback") {
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
        members: Vec::new(),
        length: None,
        by_ranges: false,
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
    /// The playlists and manifests already asked to be read for their
    /// qualities, each once: a live playlist is fetched again every few
    /// seconds by its player.
    described: std::collections::HashSet<(i64, String)>,
}

/// Adds a stream's length to its line, once (gaps row 155). Whether it did.
fn say_length(stream: &mut Stream, seconds: f64) -> bool {
    let Some(words) = length_words(seconds) else {
        return false;
    };
    if stream.length.is_some() {
        return false;
    }
    stream.length = Some(seconds.round() as u64);
    stream.label = format!("{}, {words}", stream.label);
    true
}

/// A master playlist's renditions are not lines beside it: what a list holds
/// of them is taken out.
fn drop_members(list: &mut Vec<Stream>) {
    let members: std::collections::HashSet<String> = list
        .iter()
        .flat_map(|s| s.members.iter().cloned())
        .collect();
    if !members.is_empty() {
        list.retain(|s| !members.contains(&s.url));
    }
}

/// How many streams a tab keeps: a page that loads more is cycling through
/// renditions or adverts, and the first and latest few are what matter.
pub const STREAMS_PER_TAB: usize = 8;

impl Streams {
    /// Takes in one response: a new page in a tab forgets the old page's
    /// streams, and media is remembered once per address.
    pub fn absorb(&mut self, seen: &Seen) {
        if new_page(seen) {
            self.forget(seen.tab_id);
            return;
        }
        let Some(stream) = stream_seen(seen) else {
            return;
        };
        let list = self.by_tab.entry(seen.tab_id).or_default();
        // Once a master playlist is read, the renditions it lists are its,
        // not lines of their own.
        if list
            .iter()
            .any(|known| known.url == stream.url || known.members.contains(&stream.url))
        {
            return;
        }
        list.push(stream);
        if list.len() > STREAMS_PER_TAB {
            // The first (a master playlist, usually) stays; the oldest of
            // the rest goes.
            list.remove(1);
        }
    }

    /// Forgets everything learned about this tab.
    pub fn forget(&mut self, tab: i64) {
        self.by_tab.remove(&tab);
        self.described.retain(|(t, _)| *t != tab);
    }

    /// Whether the playlist or manifest at `url` is to be read now for its
    /// qualities: it is one of this tab's lines, named by nothing but its
    /// kind, and has not been asked about before. Asking marks it, so a
    /// player that fetches it again does not have it read again.
    pub fn wants_description(&mut self, tab: i64, url: &str) -> bool {
        let plain = self
            .by_tab
            .get(&tab)
            .and_then(|list| list.iter().find(|s| s.url == url))
            .is_some_and(|s| {
                (s.kind == StreamKind::Hls && s.label == HLS_PLAIN)
                    || (s.kind == StreamKind::Dash && s.label == DASH_PLAIN)
            });
        plain && self.described.insert((tab, url.to_string()))
    }

    /// What the playlist or manifest at `url` says of itself, read: a master
    /// playlist is named by its best rendition and takes its renditions (and
    /// sound tracks) out of the list, which are not lines of their own; a
    /// DASH manifest is named by its tallest picture. A media playlist, or
    /// text that says nothing, changes nothing. Whether the list changed.
    pub fn describe(&mut self, tab: i64, url: &str, body: &str) -> bool {
        let Some(list) = self.by_tab.get_mut(&tab) else {
            return false;
        };
        let Some(stream) = list.iter_mut().find(|s| s.url == url) else {
            return false;
        };
        let (label, members, length) = match stream.kind {
            StreamKind::Hls => {
                let renditions = hls_renditions(url, body);
                match hls_master_label(&renditions) {
                    Some(label) => (
                        label,
                        renditions.into_iter().map(|r| r.url).collect::<Vec<_>>(),
                        None,
                    ),
                    // A media playlist keeps its name and says how long it plays, where it ends.
                    None => match hls_length(body) {
                        Some(seconds) => (stream.label.clone(), Vec::new(), Some(seconds)),
                        None => return false,
                    },
                }
            }
            StreamKind::Dash => {
                let Some(label) = dash_label(body) else {
                    return false;
                };
                (label, Vec::new(), dash_length(body))
            }
            _ => return false,
        };
        stream.label = label;
        stream.members = members;
        if let Some(seconds) = length {
            say_length(stream, seconds);
        }
        drop_members(list);
        true
    }

    /// Whether the file at `url`, found by its byte ranges ([`whole_by_ranges`]), is to have
    /// its first bytes read now for what it is; asking marks it, so it is read once.
    pub fn wants_track_read(&mut self, tab: i64, url: &str) -> bool {
        let unread = self
            .by_tab
            .get(&tab)
            .and_then(|list| list.iter().find(|s| s.url == url))
            .is_some_and(|s| s.by_ranges);
        unread && self.described.insert((tab, url.to_string()))
    }

    /// What the first bytes of the file at `url` said it holds ([`mp4_track`]): a sound, or a
    /// picture of that height, which its line is then called. Whether the list changed.
    pub fn read_track(&mut self, tab: i64, url: &str, sound: bool, height: Option<u32>) -> bool {
        let Some(stream) = self
            .by_tab
            .get_mut(&tab)
            .and_then(|list| list.iter_mut().find(|s| s.url == url && s.by_ranges))
        else {
            return false;
        };
        let (kind, label) = ranged_line(sound, height);
        let changed = stream.kind != kind || stream.label != label;
        stream.kind = kind;
        stream.label = label;
        changed
    }

    /// The rendition to read for a master playlist's length: its first, while its line says
    /// none (a master names its renditions, not how long they play).
    pub fn rendition_for_length(&self, tab: i64, url: &str) -> Option<String> {
        let stream = self.by_tab.get(&tab)?.iter().find(|s| s.url == url)?;
        if stream.kind != StreamKind::Hls || stream.length.is_some() {
            return None;
        }
        stream.members.first().cloned()
    }

    /// Says on a line how long its stream plays, once: for a master playlist, read from one of
    /// its renditions (each plays as long as the others). Whether the line changed.
    pub fn lengthen(&mut self, tab: i64, url: &str, seconds: f64) -> bool {
        self.by_tab
            .get_mut(&tab)
            .and_then(|list| list.iter_mut().find(|s| s.url == url))
            .is_some_and(|stream| say_length(stream, seconds))
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
        drop_members(&mut list);
        self.by_tab.insert(tab, list);
    }

    /// A tab's streams as remembered, newest last, for keeping.
    pub fn raw(&self, tab: i64) -> Vec<Stream> {
        self.by_tab.get(&tab).cloned().unwrap_or_default()
    }

    /// What a tab has loaded, for the panel's list ([`panel_list`]).
    pub fn of(&self, tab: i64) -> Vec<Stream> {
        panel_list(self.by_tab.get(&tab).cloned().unwrap_or_default())
    }
}
