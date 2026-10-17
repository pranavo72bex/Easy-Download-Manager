//! Adaptive streams: a manifest listing segments rather than one file.
//!
//! An HLS playlist is not a download in the sense the rest of this engine
//! means. There is no length to split into ranges and no resume position — it
//! is an ordered list of small files that concatenate into a playable one. So
//! it gets its own path, and reuses only the parts that still apply: the
//! retry policy, the speed ceiling, and being able to stop.
//!
//! The rules follow `extension/src/stream.js`, for the same reason the naming
//! and category rules do: the two halves of the product should not disagree
//! about which rendition to fetch.

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
    /// `ts`, `mp4` or `aac` — what the concatenation will actually be.
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

/// Every rendition a master playlist lists, in the order it lists them.
/// Why dm should not assemble this stream itself, where it should not: a
/// shape whose segments, joined in order, are not the file. Each of these
/// was saved as the wrong file and reported Completed — the whole source
/// repeated, a silent film, ciphertext — so they go to yt-dlp, which reads
/// them properly, or are refused. `master` is the master playlist where
/// there was one; `media` the media playlist that would be fetched.
pub fn beyond_dm(
    master: Option<&str>,
    audio_group: Option<&str>,
    media: &str,
) -> Option<&'static str> {
    let upper = media.to_ascii_uppercase();
    if upper.contains("#EXT-X-BYTERANGE") || upper.contains("BYTERANGE=") {
        return Some("this stream is cut from one file by byte ranges");
    }
    // A playlist that says it is VOD is finished, end tag or not.
    if !upper.contains("#EXT-X-ENDLIST") && !upper.contains("#EXT-X-PLAYLIST-TYPE:VOD") {
        return Some("this is a live stream, still being added to");
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
            key = Some(Key {
                uri: join(url, &uri)?,
                iv: attribute(row, "IV"),
            });
            continue;
        }

        if upper.starts_with("#EXT-X-MAP") {
            if let Some(uri) = attribute(row, "URI") {
                init = Some(Part {
                    url: join(url, &uri)?,
                    init: true,
                    sequence: 0,
                });
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
    if quality == Quality::Audio {
        container = "aac";
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

/// Decrypts one segment in place, AES-128-CBC with PKCS#7 padding.
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
mod tests {
    #[test]
    fn streams_dm_cannot_assemble_itself_are_named() {
        let plain = "#EXTM3U\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n";
        assert_eq!(beyond_dm(None, None, plain), None);
        let one_key =
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n";
        assert_eq!(beyond_dm(None, None, one_key), None);
        let restated = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\na.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\nb.ts\n#EXT-X-ENDLIST\n";
        assert_eq!(
            beyond_dm(None, None, restated),
            None,
            "the same key said twice"
        );
        let fmp4 = "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:4,\na.m4s\n#EXT-X-ENDLIST\n";
        assert_eq!(beyond_dm(None, None, fmp4), None);
        let none_then_map =
            "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXT-X-MAP:URI=\"i.mp4\"\na.m4s\n#EXT-X-ENDLIST\n";
        assert_eq!(beyond_dm(None, None, none_then_map), None);
        let vod = "#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:4,\na.ts\n";
        assert_eq!(beyond_dm(None, None, vod), None, "VOD without an end tag");
        let ranged = "#EXTM3U\n#EXT-X-BYTERANGE:100@0\nmain.ts\n#EXT-X-ENDLIST\n";
        assert!(beyond_dm(None, None, ranged)
            .unwrap()
            .contains("byte ranges"));
        let ranged_map =
            "#EXTM3U\n#EXT-X-MAP:URI=\"m.mp4\",BYTERANGE=\"720@0\"\nm.mp4\n#EXT-X-ENDLIST\n";
        assert!(beyond_dm(None, None, ranged_map).is_some());
        let live = "#EXTM3U\n#EXTINF:4,\na.ts\n";
        assert!(beyond_dm(None, None, live).unwrap().contains("live"));
        let rotated = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k1\"\na.ts\n#EXT-X-KEY:METHOD=NONE\nb.ts\n#EXT-X-ENDLIST\n";
        assert!(beyond_dm(None, None, rotated)
            .unwrap()
            .contains("encryption"));
        let sealed_init = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXT-X-MAP:URI=\"i.mp4\"\na.m4s\n#EXT-X-ENDLIST\n";
        assert!(beyond_dm(None, None, sealed_init).is_some());
        let separate = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",DEFAULT=YES,URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\nv.m3u8\n";
        assert!(beyond_dm(Some(separate), Some("a"), plain)
            .unwrap()
            .contains("sound"));
        // Sound in the picture, with a commentary track listed apart.
        let commentary =
            "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",DEFAULT=YES\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"Commentary\",URI=\"c.m3u8\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\nv.m3u8\n";
        assert_eq!(beyond_dm(Some(commentary), Some("a"), plain), None);
        // A rendition with no audio group plays its own sound.
        assert_eq!(beyond_dm(Some(separate), None, plain), None);
    }

    use super::*;

    const MASTER: &str = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.4d401f,mp4a.40.2\"\n\
        low/index.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n\
        high/index.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS=\"mp4a.40.2\"\n\
        audio/index.m3u8\n";

    const MEDIA: &str = "#EXTM3U\n\
        #EXT-X-TARGETDURATION:10\n\
        #EXT-X-MEDIA-SEQUENCE:7\n\
        #EXTINF:9.9,\n\
        seg0.ts\n\
        #EXTINF:9.9,\n\
        seg1.ts\n\
        #EXT-X-ENDLIST\n";

    #[test]
    fn a_url_or_a_content_type_says_which_kind_of_manifest() {
        assert_eq!(kind_of("https://e.com/a.m3u8", None), Some(Kind::Hls));
        assert_eq!(kind_of("https://e.com/a.m3u8?t=1", None), Some(Kind::Hls));
        assert_eq!(kind_of("https://e.com/a.mpd", None), Some(Kind::Dash));
        assert_eq!(
            kind_of("https://e.com/x", Some("application/vnd.apple.mpegurl")),
            Some(Kind::Hls)
        );
        assert_eq!(kind_of("https://e.com/a.mp4", None), None);
    }

    #[test]
    fn attributes_survive_a_comma_inside_quotes() {
        /* CODECS="avc1.4d401f,mp4a.40.2" is the ordinary case, and splitting
         * the line on commas first reads the codec list as another
         * attribute. */
        let read = attributes(
            "#EXT-X-STREAM-INF:BANDWIDTH=800000,CODECS=\"avc1.4d401f,mp4a.40.2\",RESOLUTION=640x360",
        );
        assert_eq!(
            read,
            vec![
                ("BANDWIDTH".into(), "800000".to_string()),
                ("CODECS".into(), "avc1.4d401f,mp4a.40.2".to_string()),
                ("RESOLUTION".into(), "640x360".to_string()),
            ]
        );
    }

    #[test]
    fn a_master_playlist_lists_what_it_offers() {
        let found = variants(MASTER, "https://e.com/v/master.m3u8");
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].url, "https://e.com/v/low/index.m3u8");
        assert_eq!(found[0].height, 360);
        assert_eq!(found[1].height, 1080);
        assert_eq!(found[2].height, 0, "an audio rendition has no picture");
    }

    #[test]
    fn best_takes_the_tallest() {
        let chosen = pick(variants(MASTER, "https://e.com/v/m.m3u8"), Quality::Best).unwrap();
        assert_eq!(chosen.height, 1080);
    }

    #[test]
    fn a_cap_takes_the_tallest_at_or_below_it() {
        let chosen = pick(
            variants(MASTER, "https://e.com/v/m.m3u8"),
            Quality::UpTo(720),
        )
        .unwrap();
        assert_eq!(chosen.height, 360, "1080 is over the cap");
    }

    #[test]
    fn equal_renditions_are_taken_in_the_order_the_playlist_lists_them() {
        /* A playlist offering one height at two bitrates lists the cheaper
         * one first, and under a cap that is the one to take. `max_by_key`
         * keeps the last of equal candidates and would take the other. */
        let text = "#EXTM3U\n\
            #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n\
            cheap/index.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1280x720\n\
            dear/index.m3u8\n";
        let offered = variants(text, "https://e.com/v/m.m3u8");

        for quality in [Quality::UpTo(1080), Quality::UpTo(720)] {
            assert!(
                pick(offered.clone(), quality)
                    .unwrap()
                    .url
                    .contains("cheap"),
                "{quality:?} took the dearer of two equal renditions"
            );
        }
        // Best does rank by bandwidth, so there it is the dearer one.
        assert!(pick(offered, Quality::Best).unwrap().url.contains("dear"));
    }

    #[test]
    fn a_cap_below_everything_takes_the_shortest_rather_than_the_tallest() {
        // A cap must never quietly fetch something larger than the file the
        // person setting it was trying to avoid.
        let chosen = pick(
            variants(MASTER, "https://e.com/v/m.m3u8"),
            Quality::UpTo(240),
        )
        .unwrap();
        assert_eq!(chosen.height, 360);
    }

    #[test]
    fn audio_takes_the_smallest_stream_with_no_picture() {
        let chosen = pick(variants(MASTER, "https://e.com/v/m.m3u8"), Quality::Audio).unwrap();
        assert_eq!(chosen.bandwidth, 64000);
        assert_eq!(chosen.height, 0);
    }

    #[test]
    fn a_media_playlist_becomes_an_ordered_list_of_segments() {
        let stream = media_playlist(
            MEDIA,
            "https://e.com/v/index.m3u8",
            "https://e.com/v/index.m3u8",
            Quality::Best,
        )
        .unwrap();
        assert_eq!(stream.container, "ts");
        assert_eq!(stream.parts.len(), 2);
        assert_eq!(stream.parts[0].url, "https://e.com/v/seg0.ts");
        assert_eq!(stream.parts[0].sequence, 7, "the playlist starts at 7");
        assert_eq!(stream.parts[1].sequence, 8);
        assert_eq!(stream.filename, "v.ts", "index.m3u8 names nothing");
    }

    #[test]
    fn an_initialisation_segment_leads_the_file() {
        // fMP4 will not decode without it, wherever the playlist mentions it.
        let text = MEDIA.replace(
            "#EXTINF:9.9,\nseg0.ts",
            "#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:9.9,\nseg0.m4s",
        );
        let stream = media_playlist(
            &text,
            "https://e.com/v/i.m3u8",
            "https://e.com/v/i.m3u8",
            Quality::Best,
        )
        .unwrap();
        assert_eq!(stream.container, "mp4");
        assert!(stream.parts[0].init);
        assert_eq!(stream.parts[0].url, "https://e.com/v/init.mp4");
        assert_eq!(stream.parts.len(), 3);
    }

    #[test]
    fn the_initialisation_segment_does_not_shift_the_sequence_numbers() {
        /* The sequence number is the decryption vector for a playlist that
         * declares none, so counting the init segment as a media segment
         * decrypts every one of them to noise. */
        let text = MEDIA.replace(
            "#EXT-X-MEDIA-SEQUENCE:7",
            "#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-MAP:URI=\"init.mp4\"",
        );
        let stream = media_playlist(
            &text,
            "https://e.com/v/i.m3u8",
            "https://e.com/v/i.m3u8",
            Quality::Best,
        )
        .unwrap();
        let media: Vec<u64> = stream
            .parts
            .iter()
            .filter(|p| !p.init)
            .map(|p| p.sequence)
            .collect();
        assert_eq!(media, [7, 8]);
    }

    #[test]
    fn an_encryption_dm_cannot_undo_fails_loudly() {
        // Saving a file of undecryptable bytes under a .ts name would be
        // worse than refusing.
        let text = MEDIA.replace(
            "#EXT-X-TARGETDURATION:10",
            "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"",
        );
        let err = media_playlist(
            &text,
            "https://e.com/i.m3u8",
            "https://e.com/i.m3u8",
            Quality::Best,
        )
        .unwrap_err();
        assert!(err.to_string().contains("SAMPLE-AES"), "{err}");
    }

    #[test]
    fn a_key_is_resolved_against_the_playlist() {
        let text = MEDIA.replace(
            "#EXT-X-TARGETDURATION:10",
            "#EXT-X-KEY:METHOD=AES-128,URI=\"../k.bin\",IV=0x0123456789ABCDEF0123456789ABCDEF",
        );
        let stream = media_playlist(
            &text,
            "https://e.com/v/i.m3u8",
            "https://e.com/v/i.m3u8",
            Quality::Best,
        )
        .unwrap();
        let key = stream.key.unwrap();
        assert_eq!(key.uri, "https://e.com/k.bin");
        assert_eq!(
            iv_for(&key, 99),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB,
                0xCD, 0xEF
            ],
            "a declared IV wins over the sequence number"
        );
    }

    #[test]
    fn a_playlist_with_no_iv_uses_the_sequence_number() {
        let key = Key {
            uri: "https://e.com/k".into(),
            iv: None,
        };
        let mut expected = [0u8; 16];
        expected[8..].copy_from_slice(&7u64.to_be_bytes());
        assert_eq!(iv_for(&key, 7), expected);
    }

    #[test]
    fn a_playlist_listing_nothing_is_refused() {
        assert!(media_playlist(
            "#EXTM3U\n#EXT-X-ENDLIST",
            "https://e.com/i.m3u8",
            "https://e.com/i.m3u8",
            Quality::Best
        )
        .is_err());
    }

    #[test]
    fn the_name_comes_from_the_directory_when_the_playlist_is_called_index() {
        for (url, expected) in [
            ("https://e.com/holiday/index.m3u8", "holiday.ts"),
            ("https://e.com/a/b/master.m3u8", "b.ts"),
            ("https://e.com/the-film.m3u8", "the-film.ts"),
            ("https://e.com/my%20film/index.m3u8", "my film.ts"),
        ] {
            assert_eq!(name_for(url, "ts"), expected, "for {url}");
        }
    }

    #[test]
    fn a_segment_survives_being_encrypted_and_decrypted() {
        use cbc::cipher::{block_padding::Pkcs7, BlockModeEncrypt, InnerIvInit, KeyInit};

        let key = [7u8; 16];
        let iv = [3u8; 16];
        let plain = b"the quick brown fox jumps over the lazy dog, twice over".to_vec();

        let cipher = aes::Aes128::new_from_slice(&key).unwrap();
        let sealed = cbc::Encryptor::<aes::Aes128>::inner_iv_slice_init(cipher, &iv)
            .unwrap()
            .encrypt_padded_vec::<Pkcs7>(&plain);

        assert_eq!(decrypt(&key, &iv, &sealed).unwrap(), plain);
        // The wrong key must fail rather than produce plausible rubbish.
        assert!(decrypt(&[9u8; 16], &iv, &sealed).is_err());
    }
}
