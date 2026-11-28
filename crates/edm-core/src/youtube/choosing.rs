// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Which streams are taken: the dubs set aside, a format read from the player API, and the picture
//! and sound chosen by the quality asked for.

use super::*;

/// Sets every sound track but the video's own aside: the original where one
/// is named, else the one the player would play. Where that would leave no
/// sound at all, nothing is set aside. With a language `want`ed (Options, gaps
/// row Y10) the tracks speaking it are kept instead (the original before a
/// dub, a plain one before a described one), where any does; where none does,
/// the video's own as above.
pub(super) fn set_dubs_aside(formats: &mut [Format], want: Option<&str>) {
    if let Some(want) = want {
        let speaks = |f: &Format| {
            !f.video
                && !f.secondary
                && f.lang
                    .as_deref()
                    .is_some_and(|lang| crate::sound_language::speaks(lang, want))
        };
        // Among them the video's own original first, then a plain track before a described one.
        let rank = |f: &Format| (f.voice == Voice::Original, !f.described);
        if let Some(best) = formats.iter().filter(|f| speaks(f)).map(rank).max() {
            let keep: Vec<bool> = formats
                .iter()
                .map(|f| f.video || (speaks(f) && rank(f) == best))
                .collect();
            for (f, keep) in formats.iter_mut().zip(keep) {
                f.secondary |= !keep;
            }
            return;
        }
    }
    let named_original = formats.iter().any(|f| f.voice == Voice::Original);
    let own = |voice: Voice| match voice {
        Voice::Only | Voice::Original => true,
        Voice::Default => !named_original,
        Voice::Other => false,
    };
    let sound_left = formats
        .iter()
        .any(|f| !f.video && !f.secondary && own(f.voice));
    if !sound_left {
        return;
    }
    for f in formats.iter_mut().filter(|f| !own(f.voice)) {
        f.secondary = true;
    }
}

/// The reading of a video played from YouTube's own HLS playlist of it (`playlist`): a live
/// stream, or one that has just ended. Marked live, which hands it to the engine's HLS
/// download (`videopage::download_page`): that fetches it whole where the playlist ends, and
/// records it as it goes where it does not.
pub(super) fn played_from_playlist(
    id: &str,
    player: &Value,
    playlist: &str,
    staging: &Path,
) -> String {
    let title = player
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .unwrap_or("live");
    json!({
        "id": id,
        "title": title,
        "extractor": "youtube",
        "extractor_key": "Youtube",
        "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
        "is_live": true,
        "url": playlist,
        "protocol": "m3u8_native",
        "ext": "mp4",
        "filename": staging
            .join(format!("{} [{id}].mp4", safe_title(title)))
            .display()
            .to_string(),
        "read_by": "EDM",
    })
    .to_string()
}

/// The HLS playlist of a stream that has just ended (`isPostLiveDvr`, or a live video's
/// `isLiveContent` that is no longer `isLive`), where the player names one.
pub(super) fn ended_stream_playlist(player: &Value) -> Option<&str> {
    let said = |at: &str| player.pointer(at).and_then(Value::as_bool).unwrap_or(false);
    let ended = said("/videoDetails/isPostLiveDvr")
        || (said("/videoDetails/isLiveContent") && !said("/videoDetails/isLive"));
    player
        .pointer("/streamingData/hlsManifestUrl")
        .and_then(Value::as_str)
        .filter(|u| ended && playlist_address(u))
}

/// The stream's `xtags` (`acont=original:lang=es:vb=1`) as pairs, from its
/// address or from the address inside the cipher it is locked behind.
pub(super) fn xtags(url: Option<&str>, cipher: Option<&str>) -> Vec<(String, String)> {
    let inside_cipher = || {
        url::form_urlencoded::parse(cipher?.as_bytes())
            .find(|(k, _)| k == "url")
            .map(|(_, v)| v.into_owned())
    };
    let Some(address) = url.map(str::to_string).or_else(inside_cipher) else {
        return Vec::new();
    };
    let Ok(parsed) = url::Url::parse(&address) else {
        return Vec::new();
    };
    let Some((_, tags)) = parsed.query_pairs().find(|(k, _)| k == "xtags") else {
        return Vec::new();
    };
    tags.split(':')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl Format {
    pub(super) fn of(f: &Value) -> Option<Format> {
        // A locked (DRM) stream, a live one. One served only as numbered pieces is read, and
        // fetched piece by piece (gaps row Y14): it was left out, and a video served that way
        // lost its higher qualities.
        if f.get("drmFamilies").is_some() || f.get("targetDurationSec").is_some() {
            return None;
        }
        let numbered = f.get("type").and_then(Value::as_str) == Some("FORMAT_STREAM_TYPE_OTF");
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
        // Neither: kept, marked, for the framed stream the answer may name (gaps row Y2); its
        // `xtags` are then the answer's own, written as the stream writes them.
        let framed = url.is_none() && cipher.is_none();
        let tags_written = f.get("xtags").and_then(Value::as_str).map(str::to_string);
        let mut tags = xtags(url.as_deref(), cipher.as_deref());
        if tags.is_empty() {
            if let Some(written) = tags_written.as_deref() {
                tags = super::framed::messages::xtag_pairs(written);
            }
        }
        let tag = |key: &str| tags.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        let voice = Voice::of(f, tag("acont"));
        let lang = tag("lang").map(str::to_string).or_else(|| {
            let id = f.pointer("/audioTrack/id")?.as_str()?;
            let lang = id.split('.').next()?.trim();
            (!lang.is_empty()).then(|| lang.to_string())
        });
        let described = tag("acont") == Some("descriptive");
        // The loudness-evened copy, and the voice-boosted one (`vb=1`), of a
        // track: the same bitrate as the plain one at times, so taken by
        // the highest-bitrate pick unless set aside.
        let drc =
            f.get("isDrc").and_then(Value::as_bool).unwrap_or(false) || tag("drc") == Some("1");
        let boosted = tag("vb") == Some("1");
        let range = |key: &str| {
            let r = f.get(key)?;
            let edge = |k: &str| {
                r.get(k)
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            };
            let (start, end) = (edge("start")?, edge("end")?);
            (end >= start).then_some((start, end))
        };
        let itag = number("itag")?;
        // A picture the answer lists by its format number alone is read from the table.
        let known = itags::describe(itag).filter(|k| video && k.kind != itags::Kind::Audio);
        let height = number("height")
            .or_else(|| known.map(|k| u64::from(k.height)))
            .unwrap_or(0);
        let width = number("width").unwrap_or(if number("height").is_none() {
            height * 16 / 9
        } else {
            0
        });
        Some(Format {
            itag,
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
            width,
            height,
            bitrate: number("averageBitrate")
                .or_else(|| number("bitrate"))
                .unwrap_or(0),
            size: number("contentLength"),
            fps: number("fps")
                .or_else(|| known.map(|k| u64::from(k.fps)))
                .unwrap_or(0),
            secondary: drc || boosted,
            voice,
            lang: lang.filter(|_| !video),
            described,
            init: range("initRange"),
            index: range("indexRange"),
            numbered,
            framed,
            lmt: number("lastModified"),
            tags_written,
        })
    }

    /// Its name, which the row and the parts carry.
    pub(super) fn ext(&self) -> &str {
        &self.container
    }
}

/// The picture and the sound (or the sound alone) for the quality asked,
/// chosen as `format_for` chooses: the biggest picture at or
/// under the quality, measured by its shorter side (a portrait video's
/// width: 720p is 720×1280 for a Short), in the container asked for where
/// there is one that size, with the best sound in the same container.
///
/// `frame_rate`, where the list gave the height two lines, keeps the picture to its side of 30
/// frames a second: above for the "60 fps" line, at or below for the other.
pub(super) fn choose(
    formats: &[Format],
    quality: Quality,
    container: Option<&str>,
    frame_rate: Option<u32>,
) -> Option<Vec<Format>> {
    choose_with(
        formats,
        quality,
        container,
        frame_rate,
        crate::prefer_h264::wanted(),
    )
}

/// [`choose`], `prefer_h264` saying whether the picture is taken in H.264 where the video has
/// one within the quality asked (gaps row Y8c): the tallest H.264 picture then, and AV1 or VP9
/// only where it has no H.264 at all.
pub(super) fn choose_with(
    formats: &[Format],
    quality: Quality,
    container: Option<&str>,
    frame_rate: Option<u32>,
    prefer_h264: bool,
) -> Option<Vec<Format>> {
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
    let mut all: Vec<&Format> = usable
        .iter()
        .filter(|f| f.video && f.height > 0 && side(f) <= limit)
        .copied()
        .collect();
    // H.264 preferred: the H.264 pictures alone, where there is one.
    let h264 = |f: &&Format| itags::codec_rank(&f.codec) == 1;
    if prefer_h264 && all.iter().any(h264) {
        all.retain(h264);
    }
    let biggest = all.iter().map(|f| side(f)).max()?;
    // The line's frame rate, where pictures that size have it; else what there is.
    let fast = |f: &Format| f.fps > 30;
    let pictures: Vec<&Format> = match frame_rate {
        Some(rate) => {
            let kept: Vec<&Format> = all
                .iter()
                .filter(|f| side(f) == biggest && fast(f) == (rate > 30))
                .copied()
                .collect();
            if kept.is_empty() {
                all
            } else {
                kept
            }
        }
        None => all,
    };
    // The container asked for at the biggest size, else whatever is biggest.
    let picture = pictures
        .iter()
        .filter(|f| side(f) == biggest && f.container == wanted)
        .max_by_key(|f| (f.height, f.fps, f.bitrate))
        .or_else(|| {
            pictures
                .iter()
                .filter(|f| side(f) == biggest)
                .max_by_key(|f| {
                    (
                        f.container == "mp4",
                        f.height,
                        f.fps,
                        itags::codec_rank(&f.codec),
                        f.bitrate,
                    )
                })
        })?;
    // A picture with its own sound (the formats with both in one file, which
    // name two codecs) needs none beside it: asking for one would fail every
    // video offered only that way with "no stream at that quality"
    // (coverage tests).
    if picture.video && picture.codec.contains(',') {
        return Some(vec![(*picture).clone()]);
    }
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

/// [`choose`], the picture a list line was built from first (gaps row Y9): the list is read
/// from the watch page's own data and the download from a client's answer, which can lack that
/// format or hold another at its height. Taken by its format number where the answer has it,
/// with the sound [`choose`] would join it with; else as [`choose`] takes the line's height,
/// frame rate and container, the next lower where that height is missing, and, where nothing
/// is that low, the nearest above rather than no stream at all.
pub(super) fn choose_named(
    formats: &[Format],
    format: Option<u64>,
    quality: Quality,
    container: Option<&str>,
    frame_rate: Option<u32>,
) -> Option<Vec<Format>> {
    let Some(itag) = format.filter(|_| quality != Quality::Audio) else {
        return choose(formats, quality, container, frame_rate);
    };
    let named = formats
        .iter()
        .find(|f| f.video && !f.secondary && f.height > 0 && f.itag == itag);
    if let Some(picture) = named {
        let with_sounds: Vec<Format> = formats
            .iter()
            .filter(|f| !f.video || f.itag == itag)
            .cloned()
            .collect();
        if let Some(chosen) = choose(
            &with_sounds,
            Quality::Best,
            Some(picture.container.as_str()),
            None,
        ) {
            return Some(chosen);
        }
    }
    choose(formats, quality, container, frame_rate).or_else(|| {
        let Quality::UpTo(limit) = quality else {
            return None;
        };
        let side = |f: &Format| {
            if f.width > 0 && f.width < f.height {
                f.width
            } else {
                f.height
            }
        };
        let above = formats
            .iter()
            .filter(|f| f.video && !f.secondary && f.height > 0)
            .map(side)
            .filter(|s| *s > u64::from(limit))
            .min()?;
        choose(
            formats,
            Quality::UpTo(u32::try_from(above).ok()?),
            container,
            frame_rate,
        )
    })
}
