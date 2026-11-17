// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The qualities a YouTube watch page lists in its own player data.
//!
//! a full reading takes seconds over a page (the player's script, its challenge,
//! several API calls), and the panel's list waits for it. The page itself
//! already holds every picture and sound the video comes in, with sizes, in
//! `ytInitialPlayerResponse`: one request of under a second. Only the list
//! is read this way. The addresses are not there (the player streams by
//! SABR), so what is downloaded is still the full reading of the page. Where
//! the data is missing or says anything but "playable", nothing is returned
//! and the caller fails the download, saying why.

use serde_json::Value;

use super::Choice;

/// Whether `url` is one YouTube's player data is read from: a watch page, a
/// short link, or a video's embed, live, no-cookie or YouTube Music address
/// (read from its watch page, `watch_address`); not a playlist, a channel or
/// another site.
pub(super) fn is_watch_page(url: &str) -> bool {
    watch_address(url).is_some()
}

/// The watch page the qualities of `url` are read from: `url` itself where it
/// is one, else the watch page of the video an embed, live, no-cookie or
/// YouTube Music address names, so a site with a YouTube player in it lists
/// them too (gaps row Y12).
pub(super) fn watch_address(url: &str) -> Option<String> {
    if is_own_watch_page(url) {
        return Some(url.trim().to_string());
    }
    crate::youtube::video_id(url).map(|id| format!("https://www.youtube.com/watch?v={id}"))
}

/// A watch page or a short link as it stands.
fn is_own_watch_page(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    match host.as_str() {
        "youtu.be" => parsed.path().len() > 1,
        "youtube.com" | "www.youtube.com" | "m.youtube.com" => {
            (parsed.path() == "/watch"
                && parsed.query_pairs().any(|(k, v)| k == "v" && !v.is_empty()))
                || parsed.path().starts_with("/shorts/")
        }
        _ => false,
    }
}

/// Fetches the page and lists what it holds, or `None` for anything that
/// should be failed instead: no connection in time, a consent or sign-in
/// page, a live stream, a video the page does not play.
pub(super) async fn fetch_choices(
    url: &str,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Option<Vec<Choice>> {
    let mut builder = crate::with_tls(reqwest::Client::builder())
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
        )
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10));
    // A configuration script's choice is asked for the page's site.
    if let Some(proxy) = crate::proxy::route_for(proxy, url) {
        builder = builder.proxy(reqwest::Proxy::all(proxy).ok()?);
    }
    let client = builder.build().ok()?;
    // `SOCS` answers the consent page some countries are sent to first.
    let mut cookie = String::from("SOCS=CAI");
    if let Some(theirs) = crate::cookies::header_with(sign_in, url) {
        cookie = format!("{theirs}; {cookie}");
    }
    let reply = client
        .get(url.trim())
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Cookie", cookie)
        .send()
        .await
        .ok()?;
    if !reply.status().is_success() {
        return None;
    }
    let page = reply.text().await.ok()?;
    let list = choices_of_page(&page);
    (!list.is_empty()).then_some(list)
}

/// The choices in a watch page's HTML, in the same shape and order as
/// [`super::choices`] gives from a full reading: one per picture height,
/// tallest first, MP4 where there is one, then the best sound alone. Empty
/// where the page's data is absent, unplayable or live.
pub(super) fn choices_of_page(html: &str) -> Vec<Choice> {
    const MARK: &str = "ytInitialPlayerResponse = ";
    let Some(at) = html.find(MARK) else {
        return Vec::new();
    };
    let mut data =
        serde_json::Deserializer::from_str(&html[at + MARK.len()..]).into_iter::<Value>();
    let Some(Ok(player)) = data.next() else {
        return Vec::new();
    };
    choices_of_player(&player)
}

fn choices_of_player(player: &Value) -> Vec<Choice> {
    let status = player
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str);
    let live = player
        .pointer("/videoDetails/isLive")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || player
            .pointer("/videoDetails/isLiveContent")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && player.pointer("/streamingData/hlsManifestUrl").is_some();
    if status != Some("OK") || live {
        return Vec::new();
    }
    let Some(formats) = player
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    // "video/mp4; codecs=\"avc1.640028\"" is an MP4 picture, "audio/mp4" an
    // M4A sound; WebM is WebM, as players name them.
    let kind = |f: &Value| -> Option<(bool, String)> {
        let mime = f.get("mimeType")?.as_str()?;
        let (kind, rest) = mime.split_once('/')?;
        let container = rest.split(';').next()?.trim().to_ascii_lowercase();
        match kind {
            "video" => Some((true, container)),
            "audio" => Some((
                false,
                if container == "mp4" {
                    "m4a".to_string()
                } else {
                    container
                },
            )),
            _ => None,
        }
    };
    let size = |f: &Value| {
        f.get("contentLength")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
    };
    let audio = formats
        .iter()
        .filter(|f| kind(f).is_some_and(|(video, _)| !video))
        .max_by_key(|f| {
            (
                kind(f).is_some_and(|(_, ext)| ext == "m4a"),
                f.get("averageBitrate")
                    .or_else(|| f.get("bitrate"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            )
        });
    let audio_size = audio.and_then(size);
    // Height, faster than 30 frames a second, label, container, size, frame rate: a height at
    // two frame rates is two lines, as reference A lists "720p HD 60 fps" and "720p HD".
    let mut by_height: Vec<(u32, bool, String, String, Option<u64>, u64)> = Vec::new();
    for f in formats {
        let Some((true, ext)) = kind(f) else {
            continue;
        };
        let Some(height) = f.get("height").and_then(Value::as_u64).map(|h| h as u32) else {
            continue;
        };
        if height == 0 {
            continue;
        }
        let width = f.get("width").and_then(Value::as_u64).unwrap_or(0);
        let label = if width > 0 {
            format!("{} {width}×{height}", ext.to_ascii_uppercase())
        } else {
            format!("{} {height}p", ext.to_ascii_uppercase())
        };
        // A picture alone is joined with the sound, so its size counts both.
        let total = size(f).map(|s| s + audio_size.unwrap_or(0));
        let fps = f.get("fps").and_then(Value::as_u64).unwrap_or(0);
        let fast = fps > 30;
        let rank = |ext: &str, size: Option<u64>| (ext == "mp4", size);
        match by_height
            .iter_mut()
            .find(|slot| slot.0 == height && slot.1 == fast)
        {
            Some(slot) if rank(&ext, total) > rank(&slot.3, slot.4) => {
                *slot = (height, fast, label, ext, total, fps)
            }
            Some(_) => {}
            None => by_height.push((height, fast, label, ext, total, fps)),
        }
    }
    by_height.sort_by_key(|slot| (std::cmp::Reverse(slot.0), std::cmp::Reverse(slot.1)));
    let twice: Vec<u32> = by_height
        .iter()
        .filter(|slot| slot.1)
        .map(|slot| slot.0)
        .filter(|h| by_height.iter().any(|slot| slot.0 == *h && !slot.1))
        .collect();
    let mut list: Vec<Choice> = by_height
        .into_iter()
        .map(|(height, fast, label, ext, size, fps)| {
            // The quality word is the picture's shorter side, which is what
            // the chooser and the selector measure: a portrait video's
            // width (720 for 720x1280).
            let side = label
                .rsplit(' ')
                .next()
                .and_then(|wh| wh.split_once('×'))
                .and_then(|(w, _)| w.parse::<u32>().ok())
                .filter(|w| *w > 0 && *w < height)
                .unwrap_or(height);
            let shown = super::choices::shown_label(&label, &ext, height);
            let (label, quality) = if fast {
                (format!("{shown} {fps} fps"), format!("{side}/{ext}/{fps}"))
            } else if twice.contains(&height) {
                (shown, format!("{side}/{ext}/30"))
            } else {
                (shown, format!("{side}/{ext}"))
            };
            Choice {
                label,
                quality,
                size,
            }
        })
        .collect();
    if let Some((_, ext)) = audio.and_then(kind) {
        list.push(Choice {
            label: format!("{} audio", ext.to_ascii_uppercase()),
            quality: format!("audio/{ext}"),
            size: audio_size,
        });
    }
    list.extend(caption_lines(player));
    list
}

/// A line for each caption track the page names, as reference A lists them: "TTML file, EN
/// subtitles", and "(ASR)" after YouTube's own made by speech recognition (gaps row 72c).
fn caption_lines(player: &Value) -> Vec<Choice> {
    player
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|track| {
            let language = track.get("languageCode")?.as_str()?;
            let word = format!("caption/{language}");
            let (language, _) = crate::stream::Quality::caption_of(&word)?;
            let asr = track.get("kind").and_then(Value::as_str) == Some("asr");
            Some(Choice {
                label: format!(
                    "TTML file, {} subtitles{}",
                    language.to_ascii_uppercase(),
                    if asr { " (ASR)" } else { "" }
                ),
                quality: if asr { format!("{word}/asr") } else { word },
                size: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
