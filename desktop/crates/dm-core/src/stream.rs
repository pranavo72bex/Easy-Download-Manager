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

/// What a manifest URL turns out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Hls,
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
    pub uri: String,
    /// The playlist's own IV, where it declared one. Hex, `0x` optional.
    pub iv: Option<String>,
}

/// One piece of the stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub url: String,
    /// An initialisation segment, which must lead the file or nothing decodes.
    pub init: bool,
    /// The media sequence number, which is the IV when the playlist declares
    /// none of its own.
    pub sequence: u64,
}

/// Everything needed to fetch a stream as one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub parts: Vec<Part>,
    pub key: Option<Key>,
    /// `ts`, `mp4`, `m4a` or `aac` — what the concatenation will actually be.
    pub container: String,
    pub filename: String,
}

/// One rendition a master playlist offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub url: String,
    /// 0 where the playlist did not say, which is how audio-only is spotted.
    pub height: u32,
    pub bandwidth: u64,
    /// The audio group it plays with (`AUDIO=`), where its sound is listed
    /// apart from it.
    pub audio: Option<String>,
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
        word.split_once('/')
            .map(|(_, c)| c.trim().to_ascii_lowercase())
            .filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric()))
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

fn join(base: &str, relative: &str) -> Result<String, Error> {
    Url::parse(base)
        .and_then(|base| base.join(relative.trim()))
        .map(|joined| joined.to_string())
        .map_err(|e| Error::Internal(format!("a stream listed an address dm cannot read: {e}")))
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
/// repeated, a silent film, ciphertext — so they go to yt-dlp, which reads
/// them properly, or are refused. `master` is the master playlist where
/// there was one; `media` the media playlist that would be fetched, and
/// `media_url` where it was served from, which its entries are relative to.
pub fn beyond_dm(
    master: Option<&str>,
    audio_group: Option<&str>,
    media: &str,
    media_url: &str,
) -> Option<&'static str> {
    let upper = media.to_ascii_uppercase();
    if upper.contains("#EXT-X-BYTERANGE") || upper.contains("BYTERANGE=") {
        return Some("this stream is cut from one file by byte ranges");
    }
    // Keys that differ, not key lines: some packagers restate the same key
    // before every segment, which dm decrypts as one key.
    let keys: std::collections::BTreeSet<String> = lines(media)
        .iter()
        .filter(|l| l.to_ascii_uppercase().starts_with("#EXT-X-KEY"))
        .map(|l| l.trim().to_string())
        .collect();
    if keys.len() > 1 {
        return Some("this stream changes its encryption part-way through");
    }
    /* One initialisation segment leads the file; a second, different one
     * belongs to the segments after it, and joined as one file they decode
     * against the wrong one. Restated unchanged, it is still one — however
     * it is written: compared as the addresses they resolve to, so
     * `URI="i.mp4"` and `URI="./i.mp4"`, or the same line with its
     * attributes in another order, are one segment. The one place this is
     * decided; `media_playlist` reads only what passed here. */
    let maps: std::collections::BTreeSet<String> = lines(media)
        .iter()
        .filter(|l| l.to_ascii_uppercase().starts_with("#EXT-X-MAP"))
        .map(|l| {
            attribute(l, "URI")
                .and_then(|uri| join(media_url, &uri).ok())
                .unwrap_or_else(|| l.trim().to_string())
        })
        .collect();
    if maps.len() > 1 {
        return Some("this stream changes its initialisation segment part-way through");
    }
    // An initialisation segment listed after a key that encrypts is
    // encrypted too; `METHOD=NONE` encrypts nothing.
    let encrypting = keys
        .iter()
        .any(|k| !k.to_ascii_uppercase().contains("METHOD=NONE"));
    if encrypting {
        let key_at = upper.find("#EXT-X-KEY");
        let map_at = upper.find("#EXT-X-MAP");
        if let (Some(k), Some(m)) = (key_at, map_at) {
            if m > k {
                return Some("this stream's initialisation segment is encrypted");
            }
        }
    }
    // Sound apart from the picture: the chosen rendition's audio group,
    // whose default track (or first) is a playlist of its own. A group of
    // tracks without addresses is sound carried in the picture's segments;
    // a commentary track elsewhere in the master is not this rendition's.
    if let (Some(master), Some(group)) = (master, audio_group) {
        let rows = lines(master);
        let tracks: Vec<&str> = rows
            .iter()
            .copied()
            .filter(|l| {
                let u = l.to_ascii_uppercase();
                u.starts_with("#EXT-X-MEDIA")
                    && u.contains("TYPE=AUDIO")
                    && attribute(l, "GROUP-ID").as_deref() == Some(group)
            })
            .collect();
        let default = tracks
            .iter()
            .find(|l| attribute(l, "DEFAULT").is_some_and(|d| d.eq_ignore_ascii_case("YES")))
            .or(tracks.first());
        if default.is_some_and(|l| attribute(l, "URI").is_some()) {
            return Some("this stream carries its sound separately from its picture");
        }
    }
    None
}

/// Every rendition a master playlist lists, in the order it lists them.
pub fn variants(text: &str, base: &str) -> Vec<Variant> {
    let rows = lines(text);
    let mut out = Vec::new();

    for (i, row) in rows.iter().enumerate() {
        if !row.to_ascii_uppercase().starts_with("#EXT-X-STREAM-INF") {
            continue;
        }
        let Some(uri) = rows.get(i + 1) else { continue };
        if uri.starts_with('#') {
            continue;
        }
        let Ok(url) = join(base, uri) else { continue };

        out.push(Variant {
            url,
            height: attribute(row, "RESOLUTION")
                .and_then(|r| r.split('x').nth(1).and_then(|h| h.parse().ok()))
                .unwrap_or(0),
            bandwidth: attribute(row, "BANDWIDTH")
                .and_then(|b| b.parse().ok())
                .unwrap_or(0),
            audio: attribute(row, "AUDIO"),
        });
    }
    out
}

/// Chooses a rendition. `None` when the playlist offered none.
pub fn pick(offered: Vec<Variant>, quality: Quality) -> Option<Variant> {
    if offered.is_empty() {
        return None;
    }

    /* One element out of a list, so a pick rather than a sort: each arm used
     * to build a filtered copy, sort the whole of it, and take the front.
     *
     * `min_by_key(Reverse(..))` rather than `max_by_key`, because the two
     * break ties in opposite directions: `max_by_key` keeps the *last* of
     * equal candidates, and a playlist offering the same height at two
     * bitrates lists the cheaper one first. Under a cap especially, that is
     * the one to take. */
    match quality {
        // The smallest stream carrying no picture, else the smallest overall.
        Quality::Audio => {
            let silent = offered.iter().any(|v| v.height == 0);
            offered
                .into_iter()
                .filter(|v| !silent || v.height == 0)
                .min_by_key(|v| v.bandwidth)
        }
        Quality::Best => offered
            .into_iter()
            .min_by_key(|v| std::cmp::Reverse((v.height, v.bandwidth))),
        Quality::UpTo(target) => {
            if let Some(best) = offered
                .iter()
                .filter(|v| v.height > 0 && v.height <= target)
                .min_by_key(|v| std::cmp::Reverse(v.height))
            {
                return Some(best.clone());
            }
            /* Every rendition is taller than asked for, so the shortest of
             * them. A cap must never quietly fetch something larger than the
             * one the person capping it was trying to avoid. */
            offered
                .iter()
                .filter(|v| v.height > 0)
                .min_by_key(|v| v.height)
                .cloned()
                .or_else(|| offered.into_iter().next())
        }
    }
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
    let mut init: Option<Part> = None;
    let mut key = None;
    let mut container = "ts";
    let mut sequence = 0u64;

    for row in lines(text) {
        /* Segment URLs are the overwhelming majority of a media playlist and
         * none of the tags below can match one, so the cheap test comes
         * first — uppercasing every line allocated a copy of the whole
         * playlist for the sake of the handful of lines that start with #. */
        if !row.starts_with('#') {
            parts.push(Part {
                url: join(url, row)?,
                init: false,
                /* Numbered by media segments only.
                 *
                 * Counting the initialisation segment as one shifts every
                 * number by one, and the number *is* the initialisation
                 * vector for a playlist that declares none — so a stream that
                 * is both fMP4 and AES-128 would decrypt to noise. */
                sequence: sequence + parts.len() as u64,
            });
            continue;
        }
        let upper = row.to_ascii_uppercase();

        if upper.starts_with("#EXT-X-MEDIA-SEQUENCE") {
            sequence = row
                .split(':')
                .nth(1)
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            continue;
        }

        if upper.starts_with("#EXT-X-KEY") {
            let method = attribute(row, "METHOD")
                .unwrap_or_default()
                .to_ascii_uppercase();
            if method == "NONE" {
                key = None;
                continue;
            }
            if method != "AES-128" {
                return Err(Error::Internal(format!(
                    "this stream uses {method} encryption, which dm cannot decrypt"
                )));
            }
            let uri = attribute(row, "URI").ok_or_else(|| {
                Error::Internal("the stream is encrypted but names no key".into())
            })?;
            /* Plain AES-128 is HLS's own scrambling, its key handed to any
             * player that asks, as every player reads it. A key
             * kept by a rights system (FairPlay's skd://, Widevine's or
             * PlayReady's key formats) is copy protection, and dm does not
             * get around copy protection: the stream is refused. */
            let format = attribute(row, "KEYFORMAT").unwrap_or_default();
            if protected_key(&format, &uri) {
                return Err(Error::Internal(PROTECTED.into()));
            }
            key = Some(Key {
                uri: join(url, &uri)?,
                iv: attribute(row, "IV"),
            });
            continue;
        }

        if upper.starts_with("#EXT-X-MAP") {
            if let Some(uri) = attribute(row, "URI") {
                // Restated, it is the same one: `beyond_dm` has already
                // sent a stream whose initialisation segment changes to
                // yt-dlp, so the first is the only one.
                if init.is_none() {
                    init = Some(Part {
                        url: join(url, &uri)?,
                        init: true,
                        sequence: 0,
                    });
                }
                container = "mp4";
            }
            continue;
        }
    }

    if parts.is_empty() {
        return Err(Error::Internal("that playlist listed no segments".into()));
    }

    // fMP4 segments concatenate into a valid MP4; MPEG-TS into a valid TS.
    if parts.iter().any(|p| {
        p.url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .ends_with(".m4s")
    }) {
        container = "mp4";
    }
    /* Sound alone is named for what its segments are, not for the quality
     * asked: fMP4 sound is an `.m4a`, and sound in a transport stream stays
     * a `.ts`, which the remux puts into an MP4 where it can. Called `.aac`
     * whatever it was, neither opened as what its name said. Packed audio
     * (`.aac` segments) joins into an `.aac` and is one. */
    let packed_audio = parts.iter().all(|p| {
        p.url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .ends_with(".aac")
    });
    if packed_audio {
        container = "aac";
    } else if quality == Quality::Audio && container == "mp4" {
        container = "m4a";
    }

    // The initialisation segment leads the file or nothing will decode.
    if let Some(init) = init {
        parts.insert(0, init);
    }

    Ok(Stream {
        filename: name_for(name_from, container),
        parts,
        key,
        container: container.to_string(),
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
fn name_for(url: &str, container: &str) -> String {
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

#[cfg(test)]
mod tests;
