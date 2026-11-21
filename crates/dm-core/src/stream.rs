// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Adaptive streams: a manifest listing segments rather than one file.
//!
//! An HLS playlist is not a download in the sense the rest of this engine
//! means. There is no length to split into ranges and no resume position — it
//! is an ordered list of small files that concatenate into a playable one. So
//! it gets its own path, and reuses only the parts that still apply: the
//! retry policy, the speed ceiling, and being able to stop.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use url::Url;

use crate::Error;

mod master;

use master::audio_uri;
pub use master::{audio_playlist, pick, subtitle_renditions, ungrouped_audio_playlist, variants};

/// What a manifest URL turns out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An HLS playlist (`.m3u8`).
    Hls,
    /// A DASH manifest (`.mpd`).
    Dash,
}

/// Whether this names a manifest rather than a file.
pub fn kind_of(url: &str, content_type: Option<&str>) -> Option<Kind> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = content_type.unwrap_or("").to_ascii_lowercase();

    if path.ends_with(".mpd") || mime.contains("dash+xml") {
        return Some(Kind::Dash);
    }
    if path.ends_with(".m3u8")
        || path.ends_with(".m3u")
        || mime.contains("mpegurl")
        || mime.contains("x-mpegurl")
    {
        return Some(Kind::Hls);
    }
    None
}

/// The AES-128 key every segment of a playlist needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    /// Where the key is fetched from.
    pub uri: String,
    /// The playlist's own IV, where it declared one. Hex, `0x` optional.
    pub iv: Option<String>,
    /// SAMPLE-AES: the samples inside the segment are scrambled, not the segment, and are put
    /// in the clear once it is joined (`dm_media::sealed`, gaps row Y16).
    pub sample: bool,
}

/// One piece of the stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The piece's address.
    pub url: String,
    /// An initialisation segment, which must lead the file or nothing decodes.
    pub init: bool,
    /// The media sequence number, which is the IV when the playlist declares
    /// none of its own.
    pub sequence: u64,
    /// The bytes of `url` this piece is, first and last, both included, where
    /// the stream is cut from one file by `#EXT-X-BYTERANGE`.
    pub range: Option<(u64, u64)>,
    /// How long the piece plays, in seconds, as the playlist's `#EXTINF`
    /// says; 0 for an initialisation segment or where it said nothing.
    pub duration: f64,
    /// The key it is decrypted with: the playlist's last `#EXT-X-KEY` before it, which may
    /// change part-way (gaps row Y16); none for one in the clear.
    pub key: Option<Key>,
}

/// Everything needed to fetch a stream as one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    /// The pieces, in playing order.
    pub parts: Vec<Part>,
    /// The key they are locked with, if any.
    pub key: Option<Key>,
    /// `ts`, `mp4`, `m4a` or `aac` — what the concatenation will actually be.
    pub container: String,
    /// The name to save the stream under.
    pub filename: String,
}

/// One rendition a master playlist offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// The playlist of this rendition.
    pub url: String,
    /// The picture's shorter side, which is what a quality names: its
    /// height, or a portrait video's width (720 for 720x1280). 0 where the
    /// playlist did not say, which is how audio-only is spotted.
    pub height: u32,
    /// Bits per second the rendition needs.
    pub bandwidth: u64,
    /// The audio group it plays with (`AUDIO=`), where its sound is listed
    /// apart from it.
    pub audio: Option<String>,
    /// Its codecs (`CODECS=`), as written: where it names no audio group, they say whether its
    /// sound is one of the master's renditions (gaps row 205).
    pub codecs: Option<String>,
    /// The subtitle group it plays with (`SUBTITLES=`), whose renditions are
    /// listed by [`subtitle_renditions`].
    pub subtitles: Option<String>,
}

/// A subtitle rendition a master playlist lists (`#EXT-X-MEDIA:TYPE=SUBTITLES`):
/// a media playlist of WebVTT segments (gaps row Y20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleRendition {
    /// Its language (`LANGUAGE`), as a file's name could carry it; `und` where
    /// it names none, or one that could not be.
    pub language: String,
    /// Its media playlist's address, joined onto the master's.
    pub url: String,
}

/// Which rendition to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    /// The tallest on offer.
    #[default]
    Best,
    /// The tallest at or below this height.
    UpTo(u32),
    /// The smallest stream carrying no picture.
    Audio,
}

impl Quality {
    /// From its word in settings and hand-offs: "audio", a height such as
    /// "720", or anything else for the best on offer. A container after a
    /// slash ("720/mp4") is the panel's and is read by `container_of`.
    pub fn named(word: &str) -> Quality {
        let word = word.split('/').next().unwrap_or_default();
        match word.trim().to_ascii_lowercase().as_str() {
            "audio" => Quality::Audio,
            height => match height.parse::<u32>() {
                Ok(height) if height > 0 => Quality::UpTo(height),
                _ => Quality::Best,
            },
        }
    }

    /// The container in a panel's quality word ("720/mp4" is "mp4"), if any.
    pub fn container_of(word: &str) -> Option<String> {
        word.split('/')
            .nth(1)
            .map(|c| c.trim().to_ascii_lowercase())
            .filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric()))
    }

    /// The caption track a panel's quality word names ("caption/en" or
    /// "caption/en/asr", the second YouTube's own made by speech recognition):
    /// the language and whether it is the recognised one.
    pub fn caption_of(word: &str) -> Option<(String, bool)> {
        let rest = word.trim().strip_prefix("caption/")?;
        let mut parts = rest.split('/');
        let language = parts.next()?.trim();
        let ok = !language.is_empty()
            && language.len() <= 20
            && language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        ok.then(|| (language.to_string(), parts.next() == Some("asr")))
    }

    /// The frame rate in a panel's quality word ("720/mp4/60" is 60), where
    /// its height is listed twice, at two frame rates.
    pub fn frame_rate_of(word: &str) -> Option<u32> {
        word.split('/')
            .nth(2)
            .and_then(|r| r.trim().parse::<u32>().ok())
            .filter(|r| *r > 0)
    }

    /// The format number a panel's picture line names, after its container and frame rate
    /// ("1080/mp4/f137" and "720/mp4/60/f298" name 137 and 298): the picture the list was
    /// built from, which the download takes where its own reading has it (gaps row Y9).
    pub fn format_of(word: &str) -> Option<u64> {
        let mut parts = word.trim().split('/');
        parts.next()?;
        parts.next()?;
        parts.find_map(format_number)
    }

    /// A panel's quality word without the format number [`Quality::format_of`] reads
    /// ("1080/mp4/f137" is "1080/mp4"), as it was written before the list carried one.
    pub fn without_format(word: &str) -> &str {
        match word.rsplit_once('/') {
            Some((head, last)) if head.contains('/') && format_number(last).is_some() => head,
            _ => word,
        }
    }

    /// The word `named` reads back.
    pub fn word(self) -> String {
        match self {
            Quality::Best => "best".into(),
            Quality::UpTo(height) => height.to_string(),
            Quality::Audio => "audio".into(),
        }
    }
}

/// A quality word's part that is a format number, "f137": 137.
fn format_number(part: &str) -> Option<u64> {
    let digits = part.trim().strip_prefix('f')?;
    let plain = digits.len() <= 9 && digits.bytes().all(|b| b.is_ascii_digit());
    digits.parse::<u64>().ok().filter(|_| plain)
}

fn join(base: &str, relative: &str) -> Result<String, Error> {
    let joined = Url::parse(base)
        .and_then(|base| base.join(relative.trim()))
        .map(|joined| joined.to_string())
        .map_err(|e| Error::Internal(format!("a stream listed an address dm cannot read: {e}")))?;
    // A public playlist may not send dm inside, as a redirect may not (`redirects::named_inside`).
    if crate::redirects::named_inside(base, &joined) {
        return Err(Error::Internal(crate::http_client::INWARD_REDIRECT.into()));
    }
    Ok(joined)
}

/// Reads `ATTR=VALUE` pairs off an `#EXT-X-` tag, honouring quoted values.
///
/// Hand-scanned rather than matched with a regular expression: a value may
/// contain commas inside quotes — `CODECS="avc1.4d401f,mp4a.40.2"` is the
/// ordinary case — and splitting on commas first gets that wrong.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let body = match tag.find(':') {
        Some(at) => &tag[at + 1..],
        None => return Vec::new(),
    };

    let mut out = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let Some(equals) = rest.find('=') else { break };
        let name = rest[..equals]
            .trim()
            .trim_start_matches(',')
            .to_ascii_uppercase();
        let after = &rest[equals + 1..];

        let (value, next) = if let Some(stripped) = after.strip_prefix('"') {
            match stripped.find('"') {
                Some(close) => (&stripped[..close], &stripped[close + 1..]),
                None => (stripped, ""),
            }
        } else {
            match after.find(',') {
                Some(comma) => (&after[..comma], &after[comma..]),
                None => (after, ""),
            }
        };

        out.push((name, value.trim().to_string()));
        rest = next.trim_start_matches([',', ' ']);
    }
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    attributes(tag)
        .into_iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// Non-empty, trimmed lines.
fn lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

/// Why dm should not assemble this stream itself, where it should not: a
/// shape whose segments, joined in order, are not the file. Each of these
/// was saved as the wrong file and reported Completed — the whole source
/// repeated, a silent film, ciphertext — so they are refused, not saved
/// as a file that is not one. `master` is the master playlist where
/// there was one; `media` the media playlist that would be fetched, and
/// `_media_url` where it was served from, which its entries are relative to.
pub fn beyond_dm(
    master: Option<&str>,
    audio_group: Option<&str>,
    media: &str,
    _media_url: &str,
) -> Option<&'static str> {
    // A key that changes part-way is each segment's own (`Part::key`), fetched once each, as
    // a player does (gaps row Y16): refused before. So is an initialisation segment that
    // changes part-way (an advert break): each starts a run of its own, decoded with its own
    // (`media_playlist`, `mp4mux`'s runs), in a pair too; a live pair refuses it (`changes_map`).
    /* An initialisation segment listed under an AES-128 key is encrypted with it;
     * `METHOD=NONE` encrypts nothing, and SAMPLE-AES scrambles the samples, never the
     * initialisation segment. */
    let mut sealing = false;
    for line in lines(media) {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("#EXT-X-KEY") {
            sealing = attribute(line, "METHOD").is_some_and(|m| m.eq_ignore_ascii_case("AES-128"));
        } else if upper.starts_with("#EXT-X-MAP") && sealing {
            return Some("this stream's initialisation segment is encrypted");
        }
    }
    // Sound apart from the picture: the chosen rendition's audio group,
    // whose default track (or first) is a playlist of its own. A group of
    // tracks without addresses is sound carried in the picture's segments;
    // a commentary track elsewhere in the master is not this rendition's.
    if let (Some(master), Some(group)) = (master, audio_group) {
        let want = crate::sound_language::wanted();
        if audio_uri(master, group, want.as_deref()).is_some() {
            return Some("this stream carries its sound separately from its picture");
        }
    }
    None
}

/// Whether a media playlist's initialisation segment changes part-way: a second, different one
/// belongs to the segments after it. Restated unchanged it is still one, however it is written:
/// compared as the addresses they resolve to, so `URI="i.mp4"` and `URI="./i.mp4"`, or the same
/// line with its attributes in another order, are one segment. A playlist is joined run by run,
/// alone or in a pair (gaps row Y16); a live pair's recording is not, and is refused.
pub fn changes_map(media: &str, media_url: &str) -> bool {
    let maps: std::collections::BTreeSet<String> = lines(media)
        .iter()
        .filter(|l| l.to_ascii_uppercase().starts_with("#EXT-X-MAP"))
        .map(|l| {
            attribute(l, "URI")
                .and_then(|uri| join(media_url, &uri).ok())
                .unwrap_or_else(|| l.trim().to_string())
        })
        .collect();
    maps.len() > 1
}

/// Whether a media playlist's samples are scrambled by SAMPLE-AES: put in the clear once its
/// segments are joined, alone or in a pair (gaps row Y16), which a live recording does not do,
/// and refuses.
pub fn sample_scrambled(media: &str) -> bool {
    lines(media).iter().any(|l| {
        l.to_ascii_uppercase().starts_with("#EXT-X-KEY")
            && attribute(l, "METHOD").is_some_and(|m| m.eq_ignore_ascii_case("SAMPLE-AES"))
    })
}

/// A byte range as the playlist writes it: a length, and `@` where it starts.
fn byte_range(text: &str) -> Result<(u64, Option<u64>), Error> {
    let bad = || {
        Error::Internal(format!(
            "the stream names a byte range dm cannot read: {text}"
        ))
    };
    let (len, at) = match text.trim().trim_matches('"').split_once('@') {
        Some((len, at)) => (len, Some(at.parse::<u64>().map_err(|_| bad())?)),
        None => (text.trim().trim_matches('"'), None),
    };
    let len = len.parse::<u64>().map_err(|_| bad())?;
    if len == 0 {
        return Err(bad());
    }
    Ok((len, at))
}

/// Reads a media playlist into the segments that make up the file.
///
/// `name_from` is the URL the person actually reached for: a rendition is
/// called something like `v1080/index.m3u8`, and naming the file after that
/// tells nobody what it is.
pub fn media_playlist(
    text: &str,
    url: &str,
    name_from: &str,
    quality: Quality,
) -> Result<Stream, Error> {
    let mut parts: Vec<Part> = Vec::new();
    // The initialisation segment in force, and whether it has been put before a segment yet:
    // each one leads the segments after it, until the next (gaps row Y16).
    let mut init: Option<Part> = None;
    let mut init_placed = false;
    // Media segments so far: what numbers them, initialisation segments not counted.
    let mut media = 0u64;
    let mut key = None;
    let mut container = "ts";
    let mut sequence = 0u64;
    // `#EXT-X-BYTERANGE` names the bytes of the next segment: a length, and
    // where it starts unless it goes on from the last piece of the same file.
    let mut cut: Option<(u64, Option<u64>)> = None;
    let mut ended_at: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    // `#EXTINF`'s seconds, for the segment that follows it.
    let mut pending = 0.0f64;

    for row in lines(text) {
        /* Segment URLs are the overwhelming majority of a media playlist and
         * none of the tags below can match one, so the cheap test comes
         * first — uppercasing every line allocated a copy of the whole
         * playlist for the sake of the handful of lines that start with #. */
        if !row.starts_with('#') {
            let url = join(url, row)?;
            let range = match cut.take() {
                Some((len, at)) => {
                    let first = at.or_else(|| ended_at.get(&url).copied()).ok_or_else(|| {
                        Error::Internal("a byte range with no start follows nothing".into())
                    })?;
                    let last = first
                        .checked_add(len)
                        .and_then(|end| end.checked_sub(1))
                        .ok_or_else(|| Error::Internal("a byte range of no length".into()))?;
                    ended_at.insert(url.clone(), last + 1);
                    Some((first, last))
                }
                None => None,
            };
            if let Some(init) = init.as_ref().filter(|_| !init_placed) {
                parts.push(init.clone());
                init_placed = true;
            }
            parts.push(Part {
                url,
                range,
                duration: std::mem::take(&mut pending),
                init: false,
                /* Numbered by media segments only.
                 *
                 * Counting the initialisation segment as one shifts every
                 * number by one, and the number *is* the initialisation
                 * vector for a playlist that declares none — so a stream that
                 * is both fMP4 and AES-128 would decrypt to noise. */
                sequence: sequence + media,
                key: key.clone(),
            });
            media += 1;
            continue;
        }
        let upper = row.to_ascii_uppercase();

        if upper.starts_with("#EXTINF") {
            pending = row
                .split_once(':')
                .and_then(|(_, rest)| rest.split(',').next())
                .and_then(|seconds| seconds.trim().parse::<f64>().ok())
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .unwrap_or(0.0);
            continue;
        }

        if upper.starts_with("#EXT-X-MEDIA-SEQUENCE") {
            sequence = row
                .split(':')
                .nth(1)
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            continue;
        }

        if upper.starts_with("#EXT-X-BYTERANGE") {
            cut = Some(byte_range(row.split_once(':').map_or("", |(_, v)| v))?);
            continue;
        }

        if upper.starts_with("#EXT-X-KEY") {
            key = tags::key_of(row, url)?;
            continue;
        }

        if upper.starts_with("#EXT-X-MAP") {
            if let Some(uri) = attribute(row, "URI") {
                let range = tags::map_range(row)?;
                let url = join(url, &uri)?;
                // Restated, it is the same one, and is not put in again.
                let same = init
                    .as_ref()
                    .is_some_and(|i| i.url == url && i.range == range);
                if !same {
                    init = Some(Part {
                        url,
                        init: true,
                        sequence: 0,
                        range,
                        duration: 0.0,
                        // Never encrypted where an AES-128 key is in force (`beyond_dm`).
                        key: None,
                    });
                    init_placed = false;
                }
                container = "mp4";
            }
            continue;
        }
    }

    if parts.is_empty() {
        return Err(Error::Internal("that playlist listed no segments".into()));
    }

    container = tags::container_of(&parts, container, quality);

    // The stream's key, where one is: its first encrypted segment's; each segment keeps its
    // own, as the key may change part-way (gaps row Y16).
    let _ = key;
    let key = parts.iter().find_map(|p| p.key.clone());
    Ok(Stream {
        filename: name_for(name_from, container),
        parts,
        key,
        container: container.to_string(),
    })
}

/// The part of `stream` that plays between `from` and `to` seconds: its
/// initialisation segments, then every segment whose playing time overlaps
/// the span, in order. With the second where the first segment taken
/// begins and the second where the last ends (a segment is taken whole, so
/// a part is cut at the nearest pieces unless the container can be trimmed
/// afterwards). `None` where no segment overlaps, or the playlist gave no
/// lengths (`#EXTINF`) to place them by.
pub fn cover(stream: &Stream, from: f64, to: f64) -> Option<(Stream, f64, f64)> {
    let mut parts: Vec<Part> = Vec::new();
    let (mut at, mut starts, mut ends, mut any) = (0.0f64, 0.0f64, 0.0f64, false);
    // The initialisation segment of the segments that follow it, put in before the first of
    // them taken: where it changes part-way, each run keeps its own (gaps row Y16).
    let mut init: Option<&Part> = None;
    for part in &stream.parts {
        if part.init {
            init = Some(part);
            continue;
        }
        let end = at + part.duration;
        if part.duration > 0.0 && at < to && end > from {
            if !any {
                starts = at;
                any = true;
            }
            ends = end;
            if let Some(init) = init.take() {
                parts.push(init.clone());
            }
            parts.push(part.clone());
        }
        at = end;
    }
    any.then(|| {
        (
            Stream {
                parts,
                key: stream.key.clone(),
                container: stream.container.clone(),
                filename: stream.filename.clone(),
            },
            starts,
            ends,
        )
    })
}

/// What a stream's segments are, from the first bytes of one: a transport
/// stream, fragmented MP4, or packed ADTS audio (bare, or behind the ID3
/// tag that carries its timestamp). `None` where the bytes say none of
/// these, and the playlist's own words decide.
pub fn container_of_head(head: &[u8]) -> Option<&'static str> {
    let adts = |at: usize| {
        matches!(
            (head.get(at), head.get(at + 1)),
            (Some(0xFF), Some(second)) if second & 0xF6 == 0xF0
        )
    };
    if head.first() == Some(&0x47) && head.get(188).is_none_or(|&b| b == 0x47) {
        return Some("ts");
    }
    if matches!(head.get(4..8), Some(b"ftyp" | b"styp" | b"moof" | b"sidx")) {
        return Some("mp4");
    }
    if adts(0) {
        return Some("aac");
    }
    if head.get(..3) == Some(b"ID3") {
        // A syncsafe length: seven bits a byte, after a ten-byte header,
        // with a ten-byte footer where the flags say there is one.
        let size = head
            .get(6..10)?
            .iter()
            .fold(0usize, |n, &b| (n << 7) | usize::from(b & 0x7F));
        let footer = if head.get(5).is_some_and(|f| f & 0x10 != 0) {
            10
        } else {
            0
        };
        if adts(10 + size + footer) {
            return Some("aac");
        }
    }
    None
}

impl Stream {
    /// Names the stream for what its segments turned out to be, read from
    /// their first bytes (`container_of_head`): a playlist's addresses are
    /// often tokens with no extension to go by, and packed audio from such
    /// addresses was called a transport stream and handed to the remux.
    pub fn found_to_be(&mut self, found: &str, name_from: &str, quality: Quality) {
        let container = match found {
            "mp4" if quality == Quality::Audio => "m4a",
            "mp4" => "mp4",
            "aac" => "aac",
            _ => "ts",
        };
        if container != self.container {
            self.container = container.to_string();
            self.filename = name_for(name_from, container);
        }
    }
}

/// A name for the finished file, from the URL the person reached for.
///
/// The manifest is nearly always called `index.m3u8` or `master.m3u8`, so the
/// directory above it carries the only meaningful name.
pub(crate) fn name_for(url: &str, container: &str) -> String {
    const ANONYMOUS: [&str; 6] = ["index", "master", "playlist", "manifest", "stream", "video"];

    let stem = (|| {
        let parsed = Url::parse(url).ok()?;
        let mut segments: Vec<String> = parsed
            .path_segments()?
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let last = segments.pop()?;
        let bare = last
            .trim_end_matches(".m3u8")
            .trim_end_matches(".m3u")
            .trim_end_matches(".mpd")
            .to_string();
        if ANONYMOUS.iter().any(|a| a.eq_ignore_ascii_case(&bare)) {
            segments.pop().or(Some(bare))
        } else if bare.is_empty() {
            None
        } else {
            Some(bare)
        }
    })()
    .unwrap_or_else(|| "stream".into());

    let cleaned = crate::probe::sanitize(&crate::probe::decode(&stem));
    let stem = if cleaned.is_empty() {
        "stream".into()
    } else {
        cleaned
    };
    format!("{stem}.{container}")
}

/// A stream's file name with the page's title in place of the one its address gave (gaps row
/// 214): the address names a playlist (`playlist.m3u8`, `HLSPlaylist.m3u8`) or the folder above
/// it (`…/cXNyPTE/playlist.m3u8`), never the video, so a title the page gave wins, with the
/// container `named` ends in. A name a person typed (`save_name`) is placed by `finish` and wins
/// over both; without a title `named` is kept.
pub fn titled(named: &str, title: Option<&str>, save_name: Option<&str>) -> String {
    if save_name.is_some_and(|n| !n.trim().is_empty()) {
        return named.to_string();
    }
    let extension = std::path::Path::new(named)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .and_then(|t| crate::naming::title_name(t, extension))
        .map(|n| crate::probe::sanitize(&n))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| named.to_string())
}

/// The initialisation vector for one segment.
///
/// The playlist's own where it declares one, otherwise the media sequence
/// number as a 128-bit big-endian integer, which is what RFC 8216 says.
pub fn iv_for(key: &Key, sequence: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    if let Some(declared) = &key.iv {
        let hex = declared
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        for (i, slot) in iv.iter_mut().enumerate() {
            *slot = hex
                .get(i * 2..i * 2 + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .unwrap_or(0);
        }
        return iv;
    }
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    iv
}

/// Whether a media playlist is still being added to: a live stream, with no
/// end tag and not said to be a finished video.
pub fn is_live(media: &str) -> bool {
    let upper = media.to_ascii_uppercase();
    !upper.contains("#EXT-X-ENDLIST") && !upper.contains("#EXT-X-PLAYLIST-TYPE:VOD")
}

/// How long a live playlist's segments last, at most (`#EXT-X-TARGETDURATION`),
/// in seconds: how often it is worth asking for it again. Six where it does
/// not say, as the specification's examples have it.
pub fn target_duration(media: &str) -> u64 {
    lines(media)
        .iter()
        .find_map(|l| {
            l.trim()
                .strip_prefix("#EXT-X-TARGETDURATION:")
                .and_then(|v| v.trim().parse::<u64>().ok())
        })
        .filter(|s| (1..=60).contains(s))
        .unwrap_or(6)
}

/// What a copy-protected stream is told.
pub const PROTECTED: &str =
    "this video is copy-protected (DRM), so only the site's own player can play it; dm does not download it";

/// Whether an HLS key belongs to a rights system rather than to HLS's own
/// scrambling: any key format but `identity`, or a key address that is not
/// one dm can fetch (FairPlay's `skd://`, a `data:` licence blob).
fn protected_key(format: &str, uri: &str) -> bool {
    let format = format.trim();
    let uri = uri.trim().to_ascii_lowercase();
    (!format.is_empty() && !format.eq_ignore_ascii_case("identity"))
        || !(uri.starts_with("http://") || uri.starts_with("https://") || !uri.contains(':'))
}

/// Decrypts one segment, AES-128-CBC with PKCS#7 padding.
pub fn decrypt(key: &[u8], iv: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, Error> {
    use cbc::cipher::{block_padding::Pkcs7, BlockModeDecrypt, InnerIvInit, KeyInit};

    if key.len() != 16 {
        return Err(Error::Internal(format!(
            "a stream key must be 16 bytes; this one is {}",
            key.len()
        )));
    }
    let cipher = aes::Aes128::new_from_slice(key)
        .map_err(|_| Error::Internal("that stream key is not usable".into()))?;

    cbc::Decryptor::<aes::Aes128>::inner_iv_slice_init(cipher, iv)
        .map_err(|_| Error::Internal("that stream key is not usable".into()))?
        .decrypt_padded_vec::<Pkcs7>(data)
        .map_err(|_| Error::Internal("a stream segment failed to decrypt".into()))
}

mod tags;

#[cfg(test)]
mod tests;

/// Reddit's master playlists, read as a player reads them (gaps row 205).
#[cfg(test)]
mod reddit_tests;
