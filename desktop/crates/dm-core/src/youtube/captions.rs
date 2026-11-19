// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A video's captions: those a person wrote and their translations, named on YouTube's own address
//! as WebVTT.

use super::*;

/// The most caption tracks a reading names: a video can list dozens of languages.
const CAPTIONS_MOST: usize = 8;

/// The caption tracks the player names, as the fetch step takes them (`requested_subtitles`:
/// each language's WebVTT address): those a person wrote, else the speech-recognised one, on
/// YouTube's own address alone (gaps row Y6).
pub(super) fn captions_named(player: &Value) -> serde_json::Map<String, Value> {
    let mut named = serde_json::Map::new();
    for track in captions_chosen(player) {
        if named.len() >= CAPTIONS_MOST {
            break;
        }
        let (Some(language), Some(base)) = (
            track.get("languageCode").and_then(Value::as_str),
            track.get("baseUrl").and_then(Value::as_str),
        ) else {
            continue;
        };
        let Ok(url) = caption_address(base, "vtt") else {
            continue;
        };
        named
            .entry(language.to_string())
            .or_insert_with(|| json!({ "url": url, "ext": "vtt" }));
    }
    named
}

/// The caption tracks a reading takes, by the choice `captions_named` says, in the player's
/// order.
fn captions_chosen(player: &Value) -> Vec<&Value> {
    let tracks: Vec<&Value> = player
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let asr = |t: &&Value| t.get("kind").and_then(Value::as_str) == Some("asr");
    let written: Vec<&Value> = tracks.iter().copied().filter(|t| !asr(t)).collect();
    if written.is_empty() {
        tracks.into_iter().filter(asr).take(1).collect()
    } else {
        written
    }
}

/// The translated caption tracks the player offers (`translated_subtitles`: each language's
/// WebVTT address and name), listed only, never fetched: the first chosen track YouTube can
/// translate (`isTranslatable`), in each language the player's `translationLanguages` names
/// that no chosen track is already in, as YouTube's own `tlang`; at most 8, on YouTube's own
/// address alone (gaps row Y6).
pub(super) fn translations_named(player: &Value) -> serde_json::Map<String, Value> {
    let chosen = captions_chosen(player);
    let mut named = serde_json::Map::new();
    let Some(base) = chosen
        .iter()
        .find(|t| t.get("isTranslatable").and_then(Value::as_bool) == Some(true))
        .and_then(|t| t.get("baseUrl").and_then(Value::as_str))
    else {
        return named;
    };
    let own: Vec<&str> = chosen
        .iter()
        .filter_map(|t| t.get("languageCode").and_then(Value::as_str))
        .collect();
    let languages = player
        .pointer("/captions/playerCaptionsTracklistRenderer/translationLanguages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for language in languages {
        if named.len() >= CAPTIONS_MOST {
            break;
        }
        let Some(code) = language.get("languageCode").and_then(Value::as_str) else {
            continue;
        };
        let plain = !code.is_empty()
            && code
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        if !plain || own.contains(&code) || named.contains_key(code) {
            continue;
        }
        let Ok(url) = caption_address(base, "vtt") else {
            return named;
        };
        let Ok(mut url) = url::Url::parse(&url) else {
            return named;
        };
        let kept: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| k != "tlang")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        url.query_pairs_mut()
            .clear()
            .extend_pairs(kept)
            .append_pair("tlang", code);
        let name = language
            .pointer("/languageName/simpleText")
            .and_then(Value::as_str)
            .unwrap_or(code);
        named.insert(
            code.to_string(),
            json!({ "url": url.to_string(), "ext": "vtt", "name": name }),
        );
    }
    named
}

/// A caption track's address in `format` (`vtt`, `ttml`): on YouTube's own `https` address
/// alone (or a test's stand-in, `DM_YOUTUBE_HOST`, named in full), its own `fmt` replaced;
/// else why not.
fn caption_address(base: &str, format: &str) -> Result<String, &'static str> {
    let mut url = url::Url::parse(base)
        .or_else(|_| url::Url::parse(&format!("https://www.youtube.com{base}")))
        .map_err(|_| "the captions' address is not one")?;
    let youtube = url.host_str() == Some("www.youtube.com") && url.scheme() == "https";
    let stand_in = crate::env::get("DM_YOUTUBE_HOST")
        .filter(|v| !v.is_empty())
        .and_then(|v| url::Url::parse(&v).ok())
        .is_some_and(|o| o.origin() == url.origin());
    if !youtube && !stand_in {
        return Err("the captions are not on YouTube");
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "fmt")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("fmt", format);
    Ok(url.to_string())
}

/// The reading for a caption line: the track of `language` (the speech-recognised one where
/// `asr`) the watch page's player data names, fetched as TTML into a file of its own named
/// after the video and the language. Only YouTube's own caption address is taken.
pub(super) fn caption_reading(
    id: &str,
    page: &str,
    language: &str,
    asr: bool,
    staging: &Path,
) -> Result<String, Error> {
    const MARK: &str = "ytInitialPlayerResponse = ";
    let player: Value = page
        .find(MARK)
        .and_then(|at| json_object_at(page[at + MARK.len()..].trim_start()))
        .and_then(|object| serde_json::from_str(object).ok())
        .ok_or_else(|| Error::Internal("the page names no captions".into()))?;
    let track = player
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|t| {
            t.get("languageCode").and_then(Value::as_str) == Some(language)
                && (t.get("kind").and_then(Value::as_str) == Some("asr")) == asr
        })
        .ok_or_else(|| Error::Internal(format!("the video has no {language} captions now")))?;
    let base = track
        .get("baseUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Internal("the captions have no address".into()))?;
    let url = caption_address(base, "ttml").map_err(|why| Error::Internal(why.into()))?;
    let title = player
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .unwrap_or("video");
    let name = format!("{} [{id}].{language}.ttml", safe_title(title));
    Ok(json!({
        "id": id,
        "title": title,
        "extractor": "youtube",
        "extractor_key": "Youtube",
        "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
        "ext": "ttml",
        "filename": staging.join(name).display().to_string(),
        "url": url.to_string(),
        "protocol": "https",
        "format_id": format!("caption-{language}"),
        "read_by": "dm",
    })
    .to_string())
}
