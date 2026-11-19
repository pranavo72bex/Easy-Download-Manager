// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A video site's stream in pieces: its signed address read, its byte ranges, and the MP4 track a
//! range holds.

use super::*;

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

/// YouTube's edge servers, whose pieces `track_of` reads by their itags.
pub(super) fn youtube_edge(host: &str) -> bool {
    host == "googlevideo.com" || host.ends_with(".googlevideo.com")
}

/// The kind and line of a picture or a sound fetched as a file in ranges: a picture called by
/// its height where that is known.
pub(super) fn ranged_line(sound: bool, height: Option<u32>) -> (StreamKind, String) {
    match (sound, height) {
        (true, _) => (StreamKind::Sound, RANGED_SOUND.to_string()),
        (false, Some(h)) => (StreamKind::Picture, format!("{h}p {RANGED_PICTURE}")),
        (false, None) => (StreamKind::Picture, RANGED_PICTURE.to_string()),
    }
}

/// `range=` in base64, as a path segment that names a piece's bytes begins.
const SIGNED_RANGE: &str = "cmFuZ2U9";

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
    if youtube_edge(host) {
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
    // The range also written into the path, signed with it (Vimeo's `…/range/prot/<base64 of
    // "range=2607-76215">/avf/<id>.mp4?pathsig=…&range=2607-76215`): without the parameter it
    // names the same piece, and its signature fits no other, so there is no whole file here.
    if path
        .split('/')
        .any(|segment| segment.starts_with(SIGNED_RANGE))
    {
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
    let (kind, label) = ranged_line(sound, None);
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
    if youtube_edge(&dm_protocol::host_of(url)) {
        return None;
    }
    let name = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    let extension = dm_protocol::extension_of(name).unwrap_or_default();
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
    let (kind, label) = ranged_line(sound, None);
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
/// `moov` comes first in such a file, and its first track's header and handler are in its
/// first kilobytes (about one in Bilibili's files).
pub const TRACK_READ_CAP: usize = 16 * 1024;

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
