// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! HLS and DASH manifests read: an HLS master's renditions and its label, a playlist's length, and
//! a DASH manifest's label and length.

use super::*;

/// The most of a playlist or manifest that is read for its qualities.
pub const PLAYLIST_READ_CAP: usize = 256 * 1024;

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
