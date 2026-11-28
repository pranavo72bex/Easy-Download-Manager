// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A live broadcast read: played from YouTube's own HLS playlist of it where the player names
//! one, by the engine's live HLS recording (gaps row Y5); else, where the player names its
//! framed stream and formats served by that alone, those formats read and chosen as a video's
//! are and the reading marked live, recorded through the framed stream
//! (`framed::record_live`, gaps row Y2). Refused, as before, where it names neither.

use super::*;

/// The quality, the container and the frame rate a reading is asked for.
pub(super) type Asked<'a> = (Quality, Option<&'a str>, Option<u32>);

/// The reading of the live broadcast `player` answers about, asked as `asked_as`. The outer
/// error ends the reading; the inner one is the solver's, for which the next client is asked.
#[allow(clippy::too_many_arguments)]
pub(super) async fn reading(
    client: &reqwest::Client,
    id: &str,
    page: &str,
    player: &Value,
    asked_as: &PlayerClient,
    js_urls: &[String],
    script: &mut Option<String>,
    staging: &Path,
    (quality, container, frame_rate): Asked<'_>,
) -> Result<Result<String, Error>, Error> {
    let playlist = player
        .pointer("/streamingData/hlsManifestUrl")
        .and_then(Value::as_str)
        .filter(|u| playlist_address(u));
    if let Some(playlist) = playlist {
        return Ok(Ok(played_from_playlist(id, player, playlist, staging)));
    }
    let mut stream = framed::session_of(player, asked_as);
    let mut formats = framed_formats(player);
    if stream.is_none() || formats.is_empty() {
        return Err(Error::Internal("a live stream".into()));
    }
    if let Err(e) = unlock(client, js_urls, script, &mut formats, &mut stream).await? {
        return Ok(Err(e));
    }
    let chosen = chosen_of(id, &formats, false, quality, container, frame_rate)?;
    let title = player
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| page_title(page))
        .unwrap_or_else(|| "live".into());
    let mut reading = reading_json(id, &title, None, &formats, &chosen, staging);
    framed::mark_reading(&mut reading, stream.as_ref(), &chosen);
    reading["is_live"] = json!(true);
    // No list of qualities: a live stream is recorded at the quality asked, as one played from
    // its playlist is.
    if let Some(fields) = reading.as_object_mut() {
        fields.remove("formats");
    }
    if let Some(author) = player
        .pointer("/videoDetails/author")
        .and_then(Value::as_str)
    {
        reading["uploader"] = json!(author);
    }
    if let Some(picture) = largest_thumbnail(player) {
        reading["thumbnail"] = json!(picture);
    }
    Ok(Ok(reading.to_string()))
}

/// The broadcast's formats served by its framed stream alone (neither address nor cipher), read
/// as a video's are, a live format's `targetDurationSec` set aside (it leaves a format out of a
/// video's), the sound tracks of other languages set aside as a video's are.
fn framed_formats(player: &Value) -> Vec<Format> {
    let mut formats: Vec<Format> = player
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let mut f = f.clone();
            if let Some(fields) = f.as_object_mut() {
                fields.remove("targetDurationSec");
            }
            Format::of(&f)
        })
        .filter(|f| f.framed)
        .collect();
    set_dubs_aside(&mut formats, crate::sound_language::wanted().as_deref());
    formats
}
