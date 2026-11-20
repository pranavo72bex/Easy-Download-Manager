// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The pieces of reading a watch page (`read_within`): the page, the player script, the
//! streams the player names, those chosen, and the reading said.

use super::*;

/// The address a video's watch page is asked at: past the check some videos' pages ask first
/// (`bpctr`, `has_verified`).
pub(super) fn watch_address_of(origin: &str, id: &str) -> String {
    format!("{origin}/watch?v={id}&bpctr=9999999999&has_verified=1")
}

/// The watch page of video `id`, asked as every read of it asks, the panel's list too
/// ([`watch_page_of`]), and kept a moment for the next ask of the same, so the download of a
/// line the list showed does not fetch the page again (gaps row Y13).
pub(super) async fn watch_page(
    client: &reqwest::Client,
    origin: &str,
    id: &str,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<String, Error> {
    let watch = watch_address_of(origin, id);
    // The consent answer some countries' pages ask for first, and the
    // person's own session where the page handed one over.
    let mut cookie = String::from("PREF=hl=en&tz=UTC; SOCS=CAI");
    if let Some(theirs) = crate::cookies::header_with(sign_in, &watch) {
        cookie = format!("{theirs}; {cookie}");
    }
    if let Some(kept) = kept_page(&watch, &cookie) {
        return Ok(kept);
    }
    let page = crate::redirects::send(
        client
            .get(&watch)
            .header(reqwest::header::USER_AGENT, PAGE_AGENT)
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(reqwest::header::COOKIE, &cookie),
    )
    .await?;
    let page = Error::refused_or(page).await?;
    let page = text_capped(page, PAGE_MOST).await?;
    keep_page(&watch, &cookie, &page);
    Ok(page)
}

/// The watch page of the video `url` names, as a download's reading asks it ([`watch_page`]):
/// what the panel's list reads the video's qualities from, kept for the reading that follows.
pub(crate) async fn watch_page_of(
    client: &reqwest::Client,
    url: &str,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<String, Error> {
    let id = video_id(url).ok_or_else(|| Error::Internal("not a YouTube video address".into()))?;
    watch_page(client, &origin(), &id, sign_in).await
}

/// The player script, where its timestamp is wanted now (the page gave none) and for the
/// solver later: its path, and the addresses it is fetched from.
pub(super) fn player_script(facts: &PageFacts, origin: &str) -> (Option<String>, Vec<String>) {
    // Only YouTube's own player path is fetched and run: the solver runs
    // the player's code, so a page naming a script elsewhere is not
    // followed there.
    let js_path = facts
        .player_js
        .as_deref()
        .map(|named| {
            named
                .strip_prefix("https://www.youtube.com")
                .unwrap_or(named)
        })
        .filter(|path| player_path(path))
        .map(str::to_string);
    // The build every engine reads first, then the one the page named.
    let js_urls: Vec<String> = js_path
        .iter()
        .flat_map(|path| plain_build(path).into_iter().chain(Some(path.clone())))
        .map(|path| format!("{origin}{path}"))
        .collect();
    (js_path, js_urls)
}

/// The player script, from the first of `js_urls` that answers.
pub(super) async fn fetch_script(
    client: &reqwest::Client,
    js_urls: &[String],
) -> Result<String, Error> {
    // Fetched once a session for every video it plays (gaps row Y13): a playlist's videos
    // share one player.
    if let Some(kept) = js_urls.iter().find_map(|url| kept_script(url)) {
        return Ok(kept);
    }
    let mut failed = None;
    for url in js_urls {
        let answer = async {
            let script = crate::redirects::send(
                client
                    .get(url)
                    .header(reqwest::header::USER_AGENT, PAGE_AGENT),
            )
            .await?;
            text_capped(Error::refused_or(script).await?, PAGE_MOST).await
        };
        match answer.await {
            Ok(text) => {
                keep_script(url, &text);
                return Ok(text);
            }
            Err(e) => failed = Some(e),
        }
    }
    Err(failed.unwrap_or_else(|| Error::Internal("the page names no player script".into())))
}

/// The streams `player` names, and whether they are the formats with both in one file.
pub(super) fn formats_of(player: &Value) -> (Vec<Format>, bool) {
    // The picture and the sound apart (`adaptiveFormats`), as a player
    // `bv+ba` takes them; the formats with both in one file only where
    // there is nothing apart, and then one alone (a picture with its own
    // sound and a sound beside it made a file with two sound tracks).
    let without_numbered = WITHOUT_NUMBERED.try_with(|()| ()).is_ok();
    let streams_at = |at: &str| -> Vec<Format> {
        player
            .pointer(at)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Format::of)
            .filter(|f| !(without_numbered && f.numbered))
            .collect()
    };
    let mut formats = streams_at("/streamingData/adaptiveFormats");
    let muxed = formats.is_empty();
    if muxed {
        formats = streams_at("/streamingData/formats");
    }
    set_dubs_aside(&mut formats, crate::sound_language::wanted().as_deref());
    (formats, muxed)
}

/// The streams a part can be cut from, of those `formats` names.
pub(super) fn cuttable_of(
    formats: &[Format],
    mp4_only: bool,
    codec: Option<&str>,
) -> Result<Vec<Format>, Error> {
    // A part is cut from MP4 streams alone (their pieces are indexed): a
    // height only WebM has gives way to the tallest MP4.
    let cuttable: Vec<Format> = formats
        .iter()
        .filter(|f| !mp4_only || f.container == "mp4" || f.container == "m4a")
        // Nor from numbered pieces, which have no index to cut by.
        .filter(|f| !mp4_only || !f.numbered)
        // A picture in the codec asked for (AV1, for a GIF made on Linux).
        .filter(|f| !f.video || codec.is_none_or(|c| f.codec.starts_with(c)))
        .cloned()
        .collect();
    if let Some(codec) = codec {
        if !cuttable.iter().any(|f| f.video) {
            return Err(Error::Internal(if codec == "av01" {
                "this video has no AV1 stream, which a GIF is made from on Linux".into()
            } else {
                format!("this video has no {codec} stream")
            }));
        }
    }
    Ok(cuttable)
}

/// The streams chosen of those a part can be cut from.
pub(super) fn chosen_of(
    id: &str,
    cuttable: &[Format],
    muxed: bool,
    quality: Quality,
    container: Option<&str>,
    frame_rate: Option<u32>,
) -> Result<Vec<Format>, Error> {
    // The picture the list line was built from, by its format number (gaps row Y9).
    let asked = FORMAT_ASKED.try_with(|format| *format).ok();
    let chosen = if muxed {
        choose_named(cuttable, asked, quality, container, frame_rate).map(|mut pair| {
            pair.truncate(1);
            pair
        })
    } else {
        choose_named(cuttable, asked, quality, container, frame_rate)
    }
    .ok_or_else(|| Error::Internal("no stream at that quality".into()))?;
    if let Some(asked) = asked.filter(|a| !chosen.iter().any(|f| f.itag == *a)) {
        // Nothing new is said on the row; the log says what was taken instead.
        let took: Vec<String> = chosen.iter().map(|f| f.itag.to_string()).collect();
        crate::note!(
            info,
            "dm: {id}: format {asked} is not in this reading; took {}",
            took.join("+")
        );
    }
    Ok(chosen)
}

/// The reading said, of `player`'s answer and the streams `chosen` of its `formats`.
pub(super) fn reading_of(
    id: &str,
    page: &str,
    player: &Value,
    formats: &[Format],
    chosen: &[Format],
    staging: &Path,
    js_path: Option<&str>,
) -> String {
    // The TV client's answer names no title; the page does.
    let title = player
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| page_title(page))
        .unwrap_or_else(|| "video".into());
    let duration = player
        .pointer("/videoDetails/lengthSeconds")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
    let mut reading = reading_json(id, &title, duration, formats, chosen, staging);
    // The channel, written into the joined file with the title (gaps row Y11).
    if let Some(author) = player
        .pointer("/videoDetails/author")
        .and_then(Value::as_str)
    {
        reading["uploader"] = json!(author);
    }
    // Its picture, the largest the player names, and the chapters its description lists,
    // written into the file (gaps row Y11).
    if let Some(picture) = largest_thumbnail(player) {
        reading["thumbnail"] = json!(picture);
    }
    if let Some(day) = publish_date(page) {
        reading["upload_date"] = json!(day);
    }
    let chapters = player
        .pointer("/videoDetails/shortDescription")
        .and_then(Value::as_str)
        .map(chapters_in)
        .unwrap_or_default();
    if !chapters.is_empty() {
        reading["chapters"] = Value::Array(
            chapters
                .into_iter()
                .map(|(start, title)| json!({ "start_time": start, "title": title }))
                .collect(),
        );
    }
    // The video's captions, for Options' "carry subtitles" to put inside it (gaps row Y6).
    let captions = captions_named(player);
    if !captions.is_empty() {
        reading["requested_subtitles"] = Value::Object(captions);
    }
    // The languages YouTube translates them into, listed and not fetched.
    let translated = translations_named(player);
    if !translated.is_empty() {
        reading["translated_subtitles"] = Value::Object(translated);
    }
    // The player script read, for a record of which one answered (the weekly check).
    if let Some(path) = js_path {
        reading["player"] = json!(path);
    }
    reading.to_string()
}
