// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Media a page loaded, seen in the browser's responses.

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
/// ask it again with no CORS and no private-network check (security review,
/// 2026-11-07).
pub fn may_reread(target: &str, from: &str) -> bool {
    let host = dm_protocol::host_of(target);
    !inside(&host) || dm_protocol::host_of(from) == host
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
        Ok(std::net::IpAddr::V4(v4)) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                // Carrier-grade NAT: a provider's network, not the web.
                || (a == 100 && (64..128).contains(&b))
        }
        Ok(std::net::IpAddr::V6(v6)) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return inside(&v4.to_string());
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
        Err(_) => false,
    }
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
    /// The stream's address.
    pub url: String,
    /// What kind of media the stream is.
    pub kind: StreamKind,
    /// Its size, where the response said.
    pub size: Option<u64>,
    /// The label the panel lists it under.
    pub label: String,
    /// For a picture and a sound a player fetched apart, joined into one
    /// line of the list: the address of the sound, which dm fetches with the
    /// picture at `url` and joins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    /// For a playlist or manifest read: the addresses of the renditions and
    /// sound tracks it lists, which a player fetches one by one and which are
    /// not lines of their own beside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    /// How long it plays, in seconds, where its playlist or manifest said: said on its line
    /// (gaps row 155), so two videos' streams of one page are told apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// A whole MP4 file a player fetched in byte ranges asked for in its `Range` header, not
    /// in its address (Bilibili's picture and sound, gaps row 155): picture or sound by its
    /// type until its first bytes are read ([`mp4_track`]), as such a server may call both
    /// `video/mp4`; and not a line beside a playlist or manifest, whose own files it may be.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_ranges: bool,
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
    // A host is letters, digits, dots and hyphens and nothing else: a message a
    // page posts itself is not read through a URL parser, and a `#`, a backslash, an `@` or a `:`
    // in the part before the first slash would name another host than the one
    // the suffix test sees.
    let plain = host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !plain
        || !(host == "googlevideo.com" || host.ends_with(".googlevideo.com"))
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

/// The label of a playlist that carries only sound.
const HLS_SOUND: &str = "HLS sound";

/// The label of a manifest that carries only sound.
const DASH_SOUND: &str = "DASH sound";

/// The labels of a playlist or manifest nothing is known of yet; the ones
/// [`Streams::wants_description`] asks to have read.
const HLS_PLAIN: &str = "HLS stream";
const DASH_PLAIN: &str = "DASH stream";

/// The most of a playlist or manifest that is read for its qualities.
pub const PLAYLIST_READ_CAP: usize = 256 * 1024;

/// A file smaller than this, beside something larger, is a sign-in page's
/// clip, an advert or a preview, not the video the person came for.
pub const CLIP_FLOOR: u64 = 512 * 1024;

/// One rendition a master playlist lists: a picture with its sound, or a
/// sound track alone.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendition {
    /// Its playlist's address, made absolute against the master's.
    pub url: String,
    /// The picture's height (a portrait clip's smaller side), if it says.
    pub height: Option<u32>,
    /// Frames a second, rounded, if it says.
    pub frame_rate: Option<u32>,
    /// Bits a second, if it says.
    pub bandwidth: Option<u64>,
    /// A track with no picture.
    pub sound_only: bool,
}

/// The text of a tag after its name, if the line is that tag (the name in
/// any case).
fn after_tag<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    let n = tag.len();
    (line.len() >= n && line.is_char_boundary(n) && line[..n].eq_ignore_ascii_case(tag))
        .then(|| &line[n..])
}

/// The attributes of a playlist tag (`BANDWIDTH=1,CODECS="a,b"`): names
/// upper-cased, values without their quotes; a comma inside quotes stays.
fn tag_attributes(text: &str) -> Vec<(String, String)> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ',' if !quoted => pieces.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    pieces.push(current);
    pieces
        .into_iter()
        .filter_map(|piece| {
            let (name, value) = piece.split_once('=')?;
            Some((
                name.trim().to_ascii_uppercase(),
                value.trim().trim_matches('"').to_string(),
            ))
        })
        .collect()
}

/// An address a playlist names, made absolute against the playlist's own.
fn resolve_against(base: &str, reference: &str) -> String {
    let reference = reference.trim();
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return reference.to_string();
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return reference.to_string();
    };
    let rest = rest.split('#').next().unwrap_or(rest);
    let (authority, path_query) = match rest.split_once('/') {
        Some((a, p)) => (a, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    if let Some(r) = reference.strip_prefix("//") {
        return format!("{scheme}://{r}");
    }
    if reference.starts_with('/') {
        return format!("{scheme}://{authority}{reference}");
    }
    let path = path_query.split('?').next().unwrap_or("/");
    let dir = &path[..=path.rfind('/').unwrap_or(0)];
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let (ref_path, ref_query) = match reference.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (reference, None),
    };
    for part in ref_path.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    let mut out = format!("{scheme}://{authority}/{}", segments.join("/"));
    if ref_path.ends_with('/') && !out.ends_with('/') {
        out.push('/');
    }
    if let Some(q) = ref_query {
        out.push('?');
        out.push_str(q);
    }
    out
}

/// Whether a codec list names no picture.
fn only_sound(codecs: &str) -> bool {
    let codecs = codecs.to_ascii_lowercase();
    !codecs.trim().is_empty()
        && codecs.split(',').all(|c| {
            let c = c.trim();
            ![
                "avc", "hvc", "hev", "vp0", "vp8", "vp9", "av01", "dvh", "dva",
            ]
            .iter()
            .any(|video| c.starts_with(video))
        })
}

/// The renditions a master playlist lists, in its order: each
/// `EXT-X-STREAM-INF` with the address on the line after it, and each
/// `EXT-X-MEDIA` sound track with an address. A media playlist (segments, no
/// renditions) lists none.
pub fn hls_renditions(master_url: &str, body: &str) -> Vec<Rendition> {
    let mut out = Vec::new();
    let mut lines = body.lines().map(str::trim);
    while let Some(line) = lines.next() {
        if let Some(rest) = after_tag(line, "#EXT-X-STREAM-INF:") {
            let attrs = tag_attributes(rest);
            let get = |name: &str| {
                attrs
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.as_str())
            };
            let uri = lines
                .by_ref()
                .find(|l| !l.is_empty() && !l.starts_with('#'));
            let Some(uri) = uri else { break };
            let height = get("RESOLUTION")
                .and_then(|r| r.split_once(['x', 'X']))
                .and_then(|(w, h)| {
                    Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
                })
                .filter(|(w, h)| *w > 0 && *h > 0 && *w <= 16000 && *h <= 16000)
                .map(|(w, h)| w.min(h));
            out.push(Rendition {
                url: resolve_against(master_url, uri),
                height,
                frame_rate: get("FRAME-RATE")
                    .and_then(|f| f.parse::<f64>().ok())
                    .filter(|f| *f > 0.0)
                    .map(|f| f.round() as u32),
                bandwidth: get("AVERAGE-BANDWIDTH")
                    .or_else(|| get("BANDWIDTH"))
                    .and_then(|b| b.parse().ok()),
                sound_only: height.is_none() && only_sound(get("CODECS").unwrap_or("")),
            });
        } else if let Some(rest) = after_tag(line, "#EXT-X-MEDIA:") {
            let attrs = tag_attributes(rest);
            let get = |name: &str| {
                attrs
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.as_str())
            };
            if get("TYPE").is_some_and(|t| t.eq_ignore_ascii_case("AUDIO")) {
                if let Some(uri) = get("URI").filter(|u| !u.is_empty()) {
                    out.push(Rendition {
                        url: resolve_against(master_url, uri),
                        height: None,
                        frame_rate: None,
                        bandwidth: None,
                        sound_only: true,
                    });
                }
            }
        }
    }
    out
}

/// Bits a second as megabits, one decimal (`2.5 Mbps`).
fn megabits(bandwidth: u64) -> String {
    let tenths = (bandwidth + 50_000) / 100_000;
    format!("{}.{} Mbps", tenths / 10, tenths % 10)
}

/// The line of a master playlist: named by the best picture it lists (its
/// height, with the frame rate when it is 50 or more), or by its bandwidth
/// where no height is said, or as sound where it lists no picture at all.
/// `None` for a playlist that lists nothing (a media playlist).
pub fn hls_master_label(renditions: &[Rendition]) -> Option<String> {
    if renditions.is_empty() {
        return None;
    }
    let best = renditions.iter().filter(|r| !r.sound_only).max_by_key(|r| {
        (
            r.height.unwrap_or(0),
            r.frame_rate.unwrap_or(0),
            r.bandwidth.unwrap_or(0),
        )
    });
    let Some(best) = best else {
        return Some(HLS_SOUND.into());
    };
    Some(match (best.height, best.bandwidth) {
        (Some(h), _) => {
            let fps = best
                .frame_rate
                .filter(|f| *f >= 50)
                .map(|f| f.to_string())
                .unwrap_or_default();
            format!("{h}p{fps} HLS stream")
        }
        (None, Some(b)) => format!("{} HLS stream", megabits(b)),
        _ => HLS_PLAIN.into(),
    })
}

/// How long a video plays, in the words of the panel's lines (reference A's ", 41 sec"):
/// "41 sec", "4 min 12 sec", "1 h 2 min". Nothing for under half a second.
pub fn length_words(seconds: f64) -> Option<String> {
    if !seconds.is_finite() || seconds < 0.5 {
        return None;
    }
    let total = seconds.round() as u64;
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    Some(match (hours, minutes, secs) {
        (0, 0, s) => format!("{s} sec"),
        (0, m, 0) => format!("{m} min"),
        (0, m, s) => format!("{m} min {s} sec"),
        (h, 0, _) => format!("{h} h"),
        (h, m, _) => format!("{h} h {m} min"),
    })
}

/// How long an HLS media playlist plays: its segments' times summed, for one that ends
/// (`#EXT-X-ENDLIST`). A live playlist, or a master, has no length.
pub fn hls_length(body: &str) -> Option<f64> {
    if !body.contains("#EXT-X-ENDLIST") {
        return None;
    }
    let total: f64 = body
        .lines()
        .filter_map(|line| line.trim().strip_prefix("#EXTINF:"))
        .filter_map(|rest| rest.split(',').next()?.trim().parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .sum();
    (total > 0.0).then_some(total)
}

/// How long a DASH manifest plays: its `mediaPresentationDuration` (`PT4M12.5S`). A live
/// manifest names none.
pub fn dash_length(body: &str) -> Option<f64> {
    let (_, attrs) = xml_tags(body).into_iter().find(|(name, _)| name == "MPD")?;
    let (_, value) = attrs
        .iter()
        .find(|(key, _)| key == "mediaPresentationDuration")?;
    iso_duration(value)
}

/// An ISO 8601 duration as manifests write it (`P1DT2H3M4.5S`), in seconds. Years and months,
/// which have no fixed length, are not read.
fn iso_duration(text: &str) -> Option<f64> {
    let rest = text.trim().strip_prefix('P')?;
    let (days, time) = rest.split_once('T').unwrap_or((rest, ""));
    let mut total = 0.0;
    for (part, units) in [
        (days, &[('D', 86_400.0)][..]),
        (time, &[('H', 3600.0), ('M', 60.0), ('S', 1.0)][..]),
    ] {
        let mut number = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() || c == '.' {
                number.push(c);
                continue;
            }
            let (_, scale) = units.iter().find(|(unit, _)| *unit == c)?;
            total += number.parse::<f64>().ok()? * scale;
            number.clear();
        }
        if !number.is_empty() {
            return None;
        }
    }
    (total > 0.0).then_some(total)
}

/// The attributes of the XML tags of a manifest, with the tag's name first
/// (`Representation`, `height`, `1080`, …): enough to read a DASH manifest's
/// renditions without an XML library.
fn xml_tags(body: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        if tag.starts_with('/') || tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        let tag = tag.trim_end_matches('/');
        let name_end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
        let name = tag[..name_end].rsplit(':').next().unwrap_or("").to_string();
        let mut attrs = Vec::new();
        let mut text = &tag[name_end..];
        while let Some(eq) = text.find('=') {
            let key = text[..eq].trim().to_string();
            let after = text[eq + 1..].trim_start();
            let Some(quote) = after.chars().next().filter(|q| *q == '"' || *q == '\'') else {
                break;
            };
            let Some(close) = after[1..].find(quote) else {
                break;
            };
            attrs.push((key, after[1..1 + close].to_string()));
            text = &after[close + 2..];
        }
        out.push((name, attrs));
    }
    out
}

/// The line of a DASH manifest: named by the tallest picture it lists, or
/// as sound where it lists no picture, `None` where it says neither.
pub fn dash_label(body: &str) -> Option<String> {
    let mut set_is_sound = false;
    let (mut any, mut picture) = (false, false);
    let (mut height, mut bandwidth) = (0u32, 0u64);
    for (name, attrs) in xml_tags(body) {
        let get = |key: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.to_ascii_lowercase())
        };
        let sound_word = |v: &str| v.starts_with("audio");
        match name.as_str() {
            "AdaptationSet" => {
                set_is_sound = get("contentType").is_some_and(|v| sound_word(&v))
                    || get("mimeType").is_some_and(|v| sound_word(&v));
            }
            "Representation" => {
                any = true;
                let own = get("mimeType");
                let sound = match own.as_deref() {
                    Some(v) if v.starts_with("video") => false,
                    Some(v) if sound_word(v) => true,
                    _ => set_is_sound || get("codecs").is_some_and(|c| only_sound(&c)),
                };
                if sound {
                    continue;
                }
                picture = true;
                height = height.max(get("height").and_then(|h| h.parse().ok()).unwrap_or(0));
                bandwidth =
                    bandwidth.max(get("bandwidth").and_then(|b| b.parse().ok()).unwrap_or(0));
            }
            _ => {}
        }
    }
    if !any {
        None
    } else if !picture {
        Some(DASH_SOUND.into())
    } else if height > 0 {
        Some(format!("{height}p DASH stream"))
    } else if bandwidth > 0 {
        Some(format!("{} DASH stream", megabits(bandwidth)))
    } else {
        None
    }
}

/// The picture height a line's label names (`720p HLS stream`, `1080p60 …`),
/// for sorting the best first.
fn label_height(label: &str) -> u32 {
    // A manifest named by nothing but its kind is not known to be less than
    // the renditions named beside it: it is usually their master.
    if label == HLS_PLAIN || label == DASH_PLAIN {
        return u32::MAX;
    }
    let digits: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
    if label[digits.len()..].starts_with('p') {
        digits.parse().unwrap_or(0)
    } else {
        0
    }
}

/// `text` with its percent escapes read (`%3D` as `=`): a CDN writes the padding of a
/// base64 value that way in an address, and the value is not base64 until it is read.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let Some(&here) = bytes.get(at) else {
            break;
        };
        let escaped = (here == b'%')
            .then(|| bytes.get(at + 1..at + 3))
            .flatten()
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(here);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

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
        .and_then(|efg| unbase64(&percent_decoded(&efg)))
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
        members: Vec::new(),
        length: None,
        by_ranges: false,
    })
}

/// What a picture and a sound found by [`ranged_of`] are called. A file with one of these names
/// and no partner is a whole file all the same, and is offered as one.
const RANGED_PICTURE: &str = "video file";
const RANGED_SOUND: &str = "audio file";

/// A picture or a sound of any site's player that fetches each as a file in byte ranges named in
/// the address (`bytestart` and `byteend`, or `range`): the file is the address without them, and
/// what it is — picture or sound — is the response's type, which must say `video/` or `audio/`
/// (a guess from the address alone would offer a sprite or a script). `content_range` is the
/// response's `Content-Range`, whose total, where it has one, is the file's size: the largest
/// picture and the largest sound are then the best the page loaded, joined as one line by
/// [`Streams::of`].
///
/// Nothing for the pieces of an HLS or DASH stream (segments by extension), which are not whole
/// files, or for an address with no range parameter at all.
pub fn ranged_of(url: &str, mime: &str, content_range: Option<&str>) -> Option<Stream> {
    let sound = if mime.starts_with("audio/") {
        true
    } else if mime.starts_with("video/") {
        false
    } else {
        return None;
    };
    let (authority, rest) = url.split_once("://")?.1.split_once('/')?;
    let host = authority.split(':').next().unwrap_or(authority);
    // YouTube's pieces are read by `track_of`, which knows its itags.
    if host == "googlevideo.com" || host.ends_with(".googlevideo.com") {
        return None;
    }
    let (path, query) = rest.split_once('?')?;
    let query = query.split('#').next().unwrap_or("");
    let names = |pair: &str| pair.split('=').next().unwrap_or("").to_string();
    const RANGE_PARAMS: [&str; 3] = ["bytestart", "byteend", "range"];
    if !query
        .split('&')
        .any(|p| RANGE_PARAMS.contains(&names(p).as_str()))
    {
        return None;
    }
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(
        extension.as_str(),
        "ts" | "m4s" | "m4f" | "cmfv" | "cmfa" | "aac" | "vtt"
    ) {
        return None;
    }
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !RANGE_PARAMS.contains(&names(pair).as_str()))
        .collect();
    let whole = if kept.is_empty() {
        format!("https://{authority}/{path}")
    } else {
        format!("https://{authority}/{path}?{}", kept.join("&"))
    };
    if excluded::address(&whole).is_some() {
        return None;
    }
    // `bytes 0-1023/5242880`: the total after the slash, unless it is `*`.
    let size = content_range
        .and_then(|value| value.rsplit_once('/'))
        .and_then(|(_, total)| total.trim().parse::<u64>().ok());
    let (kind, label) = if sound {
        (StreamKind::Sound, RANGED_SOUND.to_string())
    } else {
        (StreamKind::Picture, RANGED_PICTURE.to_string())
    };
    Some(Stream {
        url: whole,
        kind,
        size,
        label,
        sound: None,
        members: Vec::new(),
        length: None,
        by_ranges: false,
    })
}

/// The smallest whole file found by [`whole_by_ranges`]: below it, a sound effect or a preview.
const BY_RANGES_FLOOR: u64 = 64 * 1024;

/// A picture or a sound some players fetch as one whole MP4 file in byte ranges that the
/// request's `Range` header asks for, the address naming no range (Bilibili's `…-1-100022.m4s`
/// and `…-1-30216.m4s`, both answered `206 video/mp4`; gaps row 155): a partial answer with
/// the file's whole length after the slash of its `Content-Range`, of an MP4 type, at an
/// address that ends as an MP4 file or a DASH segment does. What it is, picture or sound, is
/// its type for now, and is read from its first bytes later ([`Stream::by_ranges`]).
///
/// Nothing for YouTube's pieces (`track_of` reads those), a whole answer (200: a segment of
/// a stream, or a file `stream_seen` takes as it is), or an address on the built-in list.
pub fn whole_by_ranges(
    url: &str,
    mime: &str,
    status: u16,
    content_range: Option<&str>,
) -> Option<Stream> {
    if status != 206 {
        return None;
    }
    let sound = match mime {
        "audio/mp4" => true,
        "video/mp4" => false,
        _ => return None,
    };
    let (authority, rest) = url.split_once("://")?.1.split_once('/')?;
    let host = authority.split(':').next().unwrap_or(authority);
    if host == "googlevideo.com" || host.ends_with(".googlevideo.com") {
        return None;
    }
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(extension.as_str(), "m4s" | "mp4" | "m4v" | "m4a") {
        return None;
    }
    let size = content_range
        .and_then(|value| value.rsplit_once('/'))
        .and_then(|(_, total)| total.trim().parse::<u64>().ok())
        .filter(|total| *total >= BY_RANGES_FLOOR)?;
    if excluded::address(url).is_some() {
        return None;
    }
    let (kind, label) = if sound {
        (StreamKind::Sound, RANGED_SOUND.to_string())
    } else {
        (StreamKind::Picture, RANGED_PICTURE.to_string())
    };
    Some(Stream {
        url: url.to_string(),
        kind,
        size: Some(size),
        label,
        sound: None,
        members: Vec::new(),
        length: None,
        by_ranges: true,
    })
}

/// How many bytes of a file found by [`whole_by_ranges`] are read for what it is: an MP4's
/// `moov` comes first in such a file, and is a few kilobytes.
pub const TRACK_READ_CAP: usize = 64 * 1024;

/// What an MP4 file's first bytes say it holds: its first track's handler, picture
/// (`vide`) or sound (`soun`), and a picture's height from its track header. `None` for bytes
/// that are not an MP4's start, or that end before its first track says what it is.
pub fn mp4_track(bytes: &[u8]) -> Option<(bool, Option<u32>)> {
    // The boxes in `data`: (type, body), as far as they are whole.
    fn boxes(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
        let word = |at: usize| -> Option<u32> {
            Some(u32::from_be_bytes(
                data.get(at..at.checked_add(4)?)?.try_into().ok()?,
            ))
        };
        let mut out = Vec::new();
        let mut at = 0usize;
        while let (Some(size), Some(kind)) = (word(at), data.get(at + 4..at + 8)) {
            let Ok(kind) = <[u8; 4]>::try_from(kind) else {
                break;
            };
            let (header, size) = match size {
                1 => match (word(at + 8), word(at + 12)) {
                    (Some(high), Some(low)) => (16usize, (u64::from(high) << 32) | u64::from(low)),
                    _ => break,
                },
                // To the end of the file: as far as the bytes go.
                0 => (8, (data.len() - at) as u64),
                n => (8, u64::from(n)),
            };
            if size < header as u64 {
                break;
            }
            let end = at.saturating_add(usize::try_from(size).unwrap_or(usize::MAX));
            let Some(body) = data.get(at + header..end.min(data.len())) else {
                break;
            };
            out.push((kind, body));
            if end >= data.len() {
                break;
            }
            at = end;
        }
        out
    }
    let find = |data, kind: &[u8; 4]| {
        boxes(data)
            .into_iter()
            .find(|(k, _)| k == kind)
            .map(|(_, b)| b)
    };
    let top = boxes(bytes);
    if !top
        .iter()
        .any(|(k, _)| k == b"ftyp" || k == b"moov" || k == b"styp")
    {
        return None;
    }
    let moov = top.iter().find(|(k, _)| k == b"moov")?.1;
    let trak = find(moov, b"trak")?;
    let mdia = find(trak, b"mdia")?;
    let hdlr = find(mdia, b"hdlr")?;
    // Version and flags, then `pre_defined`, then the handler's type.
    let handler: [u8; 4] = hdlr.get(8..12)?.try_into().ok()?;
    let sound = match &handler {
        b"soun" => true,
        b"vide" => false,
        _ => return None,
    };
    // The track header's height is its last four bytes, 16.16 fixed point.
    let height = find(trak, b"tkhd")
        .filter(|tkhd| tkhd.len() >= 84)
        .and_then(|tkhd| tkhd.get(tkhd.len().saturating_sub(4)..))
        .and_then(|b| b.try_into().ok())
        .map(|b: [u8; 4]| u32::from_be_bytes(b) >> 16)
        .filter(|h| !sound && (16..=8640).contains(h));
    Some((sound, height))
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
        let (kind, label) = match (sound, height) {
            (true, _) => (StreamKind::Sound, RANGED_SOUND.to_string()),
            (false, Some(h)) => (StreamKind::Picture, format!("{h}p {RANGED_PICTURE}")),
            (false, None) => (StreamKind::Picture, RANGED_PICTURE.to_string()),
        };
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

    /// What a tab has loaded, for the panel's list: manifests first, which
    /// are whole videos, then files, largest first. Two of a kind are told
    /// apart by their file name.
    pub fn of(&self, tab: i64) -> Vec<Stream> {
        let mut list = self.by_tab.get(&tab).cloned().unwrap_or_default();
        // Beside a playlist or manifest, the whole files a player fetched by ranges are that
        // stream's own (an HLS or DASH stream's single-file renditions), not lines of their own.
        if list
            .iter()
            .any(|s| matches!(s.kind, StreamKind::Hls | StreamKind::Dash))
        {
            list.retain(|s| !s.by_ranges);
        }
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
        // A file found by its byte ranges that has no partner is a whole file on its own.
        let alone = match (&picture, &sound) {
            (Some(p), None) if p.label.ends_with(RANGED_PICTURE) => Some(p.clone()),
            (None, Some(s)) if s.label == RANGED_SOUND => Some(s.clone()),
            _ => None,
        };
        if let Some(mut file) = alone {
            file.kind = StreamKind::File;
            list.push(file);
        }
        // A playlist of sound alone is not offered beside a stream with a
        // picture: it would save the sound and nothing else.
        if list
            .iter()
            .any(|s| s.kind == StreamKind::Hls && s.label != HLS_SOUND)
        {
            list.retain(|s| !(s.kind == StreamKind::Hls && s.label == HLS_SOUND));
        }
        if list
            .iter()
            .any(|s| s.kind == StreamKind::Dash && s.label != DASH_SOUND)
        {
            list.retain(|s| !(s.kind == StreamKind::Dash && s.label == DASH_SOUND));
        }
        if let (Some(picture), Some(sound)) = (picture, sound) {
            list.push(Stream {
                url: picture.url,
                kind: StreamKind::File,
                size: picture.size.zip(sound.size).map(|(a, b)| a + b),
                label: format!("{} + sound", picture.label),
                sound: Some(sound.url),
                members: Vec::new(),
                length: None,
                by_ranges: false,
            });
        }
        // A small clip beside something larger (a sign-in page's, an advert,
        // a preview) is not what the person came for. Alone, it is the post.
        let snapshot = list.clone();
        list.retain(|s| {
            let small = s.kind == StreamKind::File && s.size.is_some_and(|z| z < CLIP_FLOOR);
            !small
                || !snapshot.iter().any(|other| {
                    other.url != s.url
                        && (other.kind != StreamKind::File
                            || other.size.is_none_or(|z| z >= CLIP_FLOOR))
                })
        });
        // Manifests first (whole videos), the tallest picture first among
        // them, then files, the largest first.
        list.sort_by_key(|s| {
            (
                s.kind == StreamKind::File,
                std::cmp::Reverse(s.size.unwrap_or(0)),
                std::cmp::Reverse(label_height(&s.label)),
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
