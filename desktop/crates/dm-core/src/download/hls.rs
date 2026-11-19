// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

mod live;
mod segments;
mod steps;

use live::{join_live, media_stem, record_live};
use segments::{cut_fragments, cut_fragments_with, fetch_segments};
pub(super) use segments::{fetch_segment_range, fetch_segment_to, join_parts_copy, named_for_span};

/// Fetches an adaptive stream as one file.
///
/// Segments rather than ranges, so almost none of the machinery above
/// applies: there is no length to plan against, no offset to resume from
/// within a segment, and nothing to ask a server for a second copy of. What
/// carries over is the retry policy, the speed ceiling, and being able to
/// stop.
pub(super) async fn download_stream(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let (mut listed, rendition, subtitles) = match resolve_stream(client, url, options).await? {
        Resolved::Own(listed, rendition, subtitles) => (listed, rendition, subtitles),
        Resolved::Pair(picture, sound, rendition, subtitles) => {
            // Carried inside the file where Options says so (gaps row Y20).
            let texts = if options.subtitles && options.quality != Quality::Audio {
                super::hls_text::fetch(client, &subtitles, options).await?
            } else {
                super::hls_text::HlsTexts::default()
            };
            return super::dash::download_pair(
                client, &picture, &sound, &rendition, texts, options, report,
            )
            .await;
        }
        Resolved::Live(playlist, sound) => {
            return record_live(client, &playlist, sound.as_deref(), url, options, report).await
        }
        Resolved::Beyond(why) => {
            return Err(Error::Internal(format!(
                "{why}: this stream shape is not supported yet"
            )));
        }
    };

    /* A part of the stream: only the segments whose cumulative lengths
     * (EXTINF) overlap the span are fetched. A fragmented MP4 is then trimmed
     * exactly, by the muxer's edit lists; a transport stream is joined whole
     * at the nearest segments, as it has no frame index to trim by here. */
    let mut rendition = rendition;
    // Where in the stream the segments taken start, which subtitles with no map of their own
    // are timed from (gaps row Y20).
    let mut skipped = 0.0;
    if let Some(span) = options.span {
        let (covered, starts, _) = stream::cover(&listed, span.from, span.to).ok_or_else(|| {
            Error::Internal("that part is outside the stream's length".to_string())
        })?;
        skipped = starts;
        rendition = format!("{rendition}|{}", span.words());
        listed = covered;
    }

    fs::create_dir_all(&options.parts_dir).await?;
    keep_only_parts_of(&options.parts_dir, &rendition).await?;
    if let Some(found) = first_segment_is(client, &listed, options).await {
        listed.found_to_be(found, url, options.quality);
    }
    listed.filename = stream::titled(
        &listed.filename,
        options.title_hint.as_deref(),
        options.save_name.as_deref(),
    );
    // Trimmed exactly only where the muxer can: a fragmented MP4.
    let fragmented = listed.container == "mp4" || listed.container == "m4a";
    let cut = options
        .span
        .filter(|_| fragmented)
        .map(|span| span.millis());
    /* An initialisation segment that changes part-way (an advert break) makes runs joined as one
     * MP4 by the muxer, each with its own set-up; samples scrambled by SAMPLE-AES are put in the
     * clear once the segments are joined (gaps row Y16). */
    let runs = listed.parts.iter().filter(|p| p.init).count() > 1;
    let sealed = listed
        .parts
        .iter()
        .any(|p| p.key.as_ref().is_some_and(|k| k.sample));
    if let Some(span) = options.span {
        listed.filename = named_for_span(&listed.filename, span);
    }

    if let Some(tx) = &report {
        let _ = tx
            .send(Update::Named {
                name: listed.filename.clone(),
                category: category::classify(&listed.filename, None),
            })
            .await;
    }

    let keys = steps::fetch_keys(client, &listed, options).await?;

    fs::create_dir_all(&options.dest_dir).await?;

    let gathered = options.parts_dir.join(GATHERED);
    // Only where the remux would run now: turned off since, or with a
    // digest that names the `.ts` bytes, an MP4 from before is not the file
    // asked for, and the stream is fetched again from its segments.
    let remuxing = remux_wanted(&listed.filename, options);
    if let Some(done) = steps::remuxed_before(&listed, remuxing, &gathered, options).await? {
        return Ok(done);
    }

    let texts = steps::carried_texts(
        client, &listed, &subtitles, remuxing, sealed, skipped, options,
    )
    .await?;

    let parts =
        steps::gathered_or_fetched(client, &listed, &keys, gathered, options, report).await?;
    // A transport stream into an MP4 where asked and able; the `.ts` where
    // not, which is still the stream, whole. One scrambled by SAMPLE-AES is put in the clear
    // first, and must be: what it was saved as would not play.
    let (parts, filename) = if sealed {
        super::hls_sealed::clear(&listed, &keys, &parts, options).await?
    } else {
        match remux_ts_with(&listed.filename, &parts, &texts, options).await? {
            Some((mp4, name)) => (vec![mp4], name),
            None => (parts, listed.filename.clone()),
        }
    };
    let parts = steps::cut_or_joined(parts, cut, runs, fragmented, &texts, options).await?;
    let (path, bytes, category) = finish(&filename, None, &parts, options).await?;

    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
    })
}

/// What the stream's first segment is, from its first few kilobytes; see
/// `stream::container_of_head`. `None` where it cannot be read or says
/// nothing, and the playlist's words stand: an initialisation segment
/// already says fMP4, and an encrypted segment's first bytes are
/// ciphertext. Read from what is staged where a resume has it, so a
/// resume asks the server for nothing it already has.
pub(super) async fn first_segment_is(
    client: &reqwest::Client,
    listed: &stream::Stream,
    options: &Options,
) -> Option<&'static str> {
    const HEAD: usize = 4096;
    if listed.parts.iter().any(|p| p.init || p.key.is_some()) {
        return None;
    }
    for staged in [
        part_path(&options.parts_dir, 0),
        options.parts_dir.join(GATHERED),
    ] {
        if let Ok(mut file) = fs::File::open(&staged).await {
            let mut head = Vec::with_capacity(HEAD);
            let read = tokio::io::AsyncReadExt::read_buf(
                &mut tokio::io::AsyncReadExt::take(&mut file, HEAD as u64),
                &mut head,
            )
            .await;
            if read.is_ok_and(|n| n > 0) {
                return stream::container_of_head(&head);
            }
        }
    }
    let first = listed.parts.first()?;
    let request = get(
        client,
        &first.url,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    )
    .header(reqwest::header::RANGE, format!("bytes=0-{}", HEAD - 1));
    let res = or_stop(
        &options.control,
        crate::auth::send(client, request, &first.url),
    )
    .await
    .ok()?;
    if !res.status().is_success() {
        return None;
    }
    // Read no further than the head, whether or not the range was honoured.
    let mut chunks = res.bytes_stream();
    let mut head = Vec::new();
    while head.len() < HEAD {
        match next_or_stop(&mut chunks, &options.control).await {
            Ok(Some(Ok(chunk))) => head.extend_from_slice(&chunk),
            _ => break,
        }
    }
    stream::container_of_head(&head)
}

/// A request's answer, or the stop asked for meanwhile. A server that takes
/// the connection and never answers holds a request until the read timeout,
/// which is a setting and can be ten minutes; a pause does not wait for it.
///
/// Work that is done at once (a chunk let through by a limiter with room, or by none) is taken
/// without a timer: each connection asks it for every chunk, thousands a second over a fast
/// link, and each made and dropped a timer entry for a wait that never ran out (gaps row S7).
/// Work still to finish is waited on in quarter seconds, the work looked at first.
pub(crate) async fn or_stop<T>(
    control: &Control,
    work: impl std::future::Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    use futures_util::FutureExt as _;
    tokio::pin!(work);
    control.check()?;
    if let Some(done) = (&mut work).now_or_never() {
        return done;
    }
    loop {
        tokio::select! {
            biased;
            done = &mut work => return done,
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
        control.check()?;
    }
}

/// The next item of a body, or the stop asked for meanwhile.
///
/// A body that has gone quiet is waited on in short spells, with the control
/// looked at between them: checked only between chunks, a pause waited for
/// the read timeout, which is a setting now and can be ten minutes. Waiting
/// out a spell drops nothing — the stream yields a chunk only once it has
/// one, and the next spell picks it up.
pub(crate) async fn next_or_stop<S>(
    stream: &mut S,
    control: &Control,
) -> Result<Option<S::Item>, Error>
where
    S: futures_util::Stream + Unpin,
{
    loop {
        control.check()?;
        if let Ok(item) = tokio::time::timeout(Duration::from_millis(250), stream.next()).await {
            return Ok(item);
        }
    }
}

/// [`next_or_stop`] for a body read chunk after chunk: one timer for the
/// whole body, armed again only when it fires, rather than one made and
/// thrown away for every chunk.
///
/// A chunk that is there already is taken without touching a timer; one
/// still to come is waited on with the control looked at every quarter of a
/// second, as before. Over a fast link a connection reads thousands of
/// chunks a second, and each `timeout` registered and removed a timer entry
/// for a wait that almost never ran out.
pub(crate) struct Reader {
    tick: std::pin::Pin<Box<tokio::time::Sleep>>,
}

impl Reader {
    pub(crate) fn new() -> Reader {
        Reader {
            tick: Box::pin(tokio::time::sleep(STOP_LOOK)),
        }
    }

    pub(crate) async fn next<S>(
        &mut self,
        stream: &mut S,
        control: &Control,
    ) -> Result<Option<S::Item>, Error>
    where
        S: futures_util::Stream + Unpin,
    {
        loop {
            control.check()?;
            tokio::select! {
                biased;
                item = stream.next() => return Ok(item),
                () = &mut self.tick => {
                    self.tick.as_mut().reset(tokio::time::Instant::now() + STOP_LOOK);
                }
            }
        }
    }
}

/// How often a wait on a quiet body looks at the control.
const STOP_LOOK: Duration = Duration::from_millis(250);

/// The most of one stream segment held in memory.
pub(super) const SEGMENT_CAP: usize = 512 * 1024 * 1024;

/// A `.ts` stream's segments, gathered into one, under its parts folder.
pub(super) const GATHERED: &str = "joined.ts";
/// That stream put into an MP4.
pub(super) const REMUXED: &str = "joined.mp4";

/// Puts the segments of a `.ts` stream into one MP4, copying the picture
/// and sound as they are, by dm itself (`tsmux`, H.264 and AAC). `None`
/// where that is not asked for (a digest that names the `.ts` bytes, a name
/// that is not a `.ts`) or the segments could not be gathered, in which case
/// the parts are left for the ordinary join; the gathered `.ts` where dm
/// could not make an MP4 of it: a stream a person can play as `.ts` beats no
/// stream. An error only where a remux that freed segments as it went failed part-way: what is
/// left is not the stream, and a retry fetches the rest again.
pub(super) async fn remux_ts(
    name: &str,
    parts: &[PathBuf],
    options: &Options,
) -> Result<Option<(PathBuf, String)>, Error> {
    remux_ts_with(name, parts, &super::hls_text::HlsTexts::default(), options).await
}

/// [`remux_ts`], with an HLS stream's subtitles carried inside the MP4, timed from where it
/// starts (gaps row Y20); none where `texts` is empty, as before.
pub(super) async fn remux_ts_with(
    name: &str,
    parts: &[PathBuf],
    texts: &super::hls_text::HlsTexts,
    options: &Options,
) -> Result<Option<(PathBuf, String)>, Error> {
    if !remux_wanted(name, options) || options.control.check().is_err() {
        return Ok(None);
    }
    /* Put into the MP4 from the segments where they lie (gaps row Y21): no gathered `.ts`,
     * and no scratch file of each track's samples, which the stream, its samples and the MP4
     * side by side made near three times the file. The segments stay until the MP4 is whole
     * and named, so a remux cut short (dm quit, the computer off) has lost none of them and is
     * made again from them; the MP4 is written under `.partial`, and a resume that finds
     * `joined.mp4` finds it whole. The peak is the segments and the MP4. A disk without room
     * for both has each segment freed once read into the MP4 instead, the peak a little over
     * the stream, at the cost of fetching again what was freed if that remux is cut short;
     * one without room even for that keeps the stream as its `.ts`, gathered as before. A
     * stream gathered by an older dm (`joined.ts`) is the one segment. */
    let segments = if room_to_join(parts, &options.parts_dir, options).is_ok() {
        crate::tsmux::Segments::Kept
    } else if room_to_free(parts, options) {
        crate::tsmux::Segments::Freed
    } else {
        return Ok(kept_as_ts(name, parts, options).await);
    };
    let mp4 = options.parts_dir.join(REMUXED);
    let partial = options.parts_dir.join(format!("{REMUXED}.partial"));
    let texts = texts.clone();
    let place = move |clock| texts.timed_on(clock);
    /* Not raced against a pause: it runs at the disk's speed, and a blocking task cut loose would
     * go on writing the file a resume makes afresh. */
    let made =
        crate::tsmux::to_mp4_from_async(parts.to_vec(), partial.clone(), segments, place).await;
    let whole = made.is_ok() && fs::metadata(&partial).await.is_ok_and(|m| m.len() > 0);
    if whole && fs::rename(&partial, &mp4).await.is_ok() {
        for part in parts {
            let _ = fs::remove_file(part).await;
        }
        return Ok(Some((mp4, format!("{}.mp4", ts_stem(name)))));
    }
    let _ = fs::remove_file(&partial).await;
    if segments == crate::tsmux::Segments::Freed && !every_one_is_there(parts).await {
        let why = made
            .err()
            .unwrap_or_else(|| "the MP4 could not be named".into());
        return Err(Error::Internal(format!(
            "the stream could not be put into an MP4: {why}"
        )));
    }
    // A stream dm cannot put into an MP4 (an MPEG-2 picture) is kept as
    // the transport stream it came as, which plays as it is.
    Ok(kept_as_ts(name, parts, options).await)
}

/// The segments gathered into one `.ts`, the stream as it came, each freed as it is consumed, as
/// the ordinary join does: what is placed where dm does not put the stream into an MP4. Joined
/// under `.partial` and named only once whole, so a resume that finds the name finds every
/// segment in it; a join cut short leaves the segments it had not reached, and the ones it had
/// are fetched again. Already gathered (a resume), it is used as is. `None` where it could not
/// be gathered: the parts are left for the ordinary join.
async fn kept_as_ts(name: &str, parts: &[PathBuf], options: &Options) -> Option<(PathBuf, String)> {
    let joined = options.parts_dir.join(GATHERED);
    if parts != [joined.clone()] {
        let partial = options.parts_dir.join(format!("{GATHERED}.partial"));
        let gathered = join_parts(parts, None, &partial, &options.control).await;
        if gathered.is_err() {
            let _ = fs::remove_file(&partial).await;
            return None;
        }
        if fs::rename(&partial, &joined).await.is_err() {
            return None;
        }
    }
    // The stream, whole, as the one part to place: what not remuxing would have saved.
    Some((joined, name.to_string()))
}

/// Room to put a stream into an MP4 while freeing each segment once read into it (gaps row
/// Y21): what the MP4 holds beyond the segments freed, its index and the few segments between
/// where its picture and its sound are read, put at a sixteenth of the stream and three of its
/// largest segments.
fn room_to_free(parts: &[PathBuf], options: &Options) -> bool {
    let sizes: Vec<u64> = parts
        .iter()
        .filter_map(|part| std::fs::metadata(part).ok())
        .map(|m| m.len())
        .collect();
    let stream: u64 = sizes.iter().sum();
    let largest = sizes.iter().copied().max().unwrap_or(0);
    let need = stream / 16 + largest.saturating_mul(3);
    options
        .room
        .free_space(&options.parts_dir)
        .is_some_and(|free| free >= need)
}

/// Whether every one of `parts` is still on the disk.
async fn every_one_is_there(parts: &[PathBuf]) -> bool {
    for part in parts {
        if fs::metadata(part).await.is_err() {
            return false;
        }
    }
    true
}

/// Whether a stream of this name is put into an MP4: asked for, a `.ts`,
/// and no digest of the `.ts` bytes to check.
pub(super) fn remux_wanted(name: &str, options: &Options) -> bool {
    options.remux && options.checksum.is_none() && name.to_ascii_lowercase().ends_with(".ts")
}

/// A `.ts` name without its extension, in any case.
pub(super) fn ts_stem(name: &str) -> &str {
    if name.to_ascii_lowercase().ends_with(".ts") {
        name.get(..name.len() - 3).unwrap_or(name)
    } else {
        name
    }
}

/// A stream as resolved: dm's to assemble, or a shape it hands on.
pub(super) enum Resolved {
    /// A stream of one playlist, the rendition's address, and the subtitle renditions the
    /// master lists for it.
    Own(stream::Stream, String, Vec<stream::SubtitleRendition>),
    /// A live stream, recorded as it goes: its media playlist's address,
    /// and its sound's where the master keeps that in a playlist of its own
    /// that dm can join to the picture.
    Live(String, Option<String>),
    /// Why dm does not assemble it itself (`stream::beyond_dm`).
    Beyond(&'static str),
    /// A picture and its sound as two playlists of fragmented MP4, which dm
    /// fetches as two tracks and joins; the rendition's address names them. With the subtitle
    /// renditions the master lists for it.
    Pair(
        Box<stream::Stream>,
        Box<stream::Stream>,
        String,
        Vec<stream::SubtitleRendition>,
    ),
}

/// Reads the manifest, following a master playlist to the rendition wanted.
///
/// A stream dm assembles comes with its media playlist's URL, which is what
/// names the rendition the segments belong to.
pub(super) async fn resolve_stream(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<Resolved, Error> {
    let quality = options.quality;
    let (text, landed) = fetch_playlist(client, url, options).await?;
    if !text.contains("#EXTM3U") {
        return Err(Error::Internal("that is not an HLS playlist".into()));
    }

    // A master playlist lists renditions; a media playlist lists segments.
    // Either way the name comes from the URL the person reached for.
    if text.to_ascii_uppercase().contains("#EXT-X-STREAM-INF") {
        let chosen = stream::pick(stream::variants(&text, &landed), quality)
            .ok_or_else(|| Error::Internal("no playable rendition in that stream".into()))?;
        let (media, media_landed) = fetch_playlist(client, &chosen.url, options).await?;
        // A rendition is a media playlist. One that is another master (or the
        // same one: a playlist that names itself) has no segments to fetch
        // and, with no end marker, was taken for a live stream and watched
        // for ever.
        if media.to_ascii_uppercase().contains("#EXT-X-STREAM-INF") {
            return Err(Error::Internal(
                "that playlist's rendition is another master playlist, not one of segments".into(),
            ));
        }
        let apart = chosen
            .audio
            .as_deref()
            .and_then(|group| stream::audio_playlist(&text, group, &landed));
        // Its subtitles, which a pair or a stream of its own carries (gaps row Y20).
        let subtitles = chosen
            .subtitles
            .as_deref()
            .map(|group| stream::subtitle_renditions(&text, group, &landed))
            .unwrap_or_default();
        if let Some(sound_url) = apart {
            let resolved = resolve_pair(
                client,
                url,
                &chosen.url,
                &media,
                &media_landed,
                &sound_url,
                options,
            )
            .await?;
            return Ok(match resolved {
                Resolved::Pair(picture, sound, rendition, _) => {
                    Resolved::Pair(picture, sound, rendition, subtitles)
                }
                other => other,
            });
        }
        if let Some(why) =
            stream::beyond_dm(Some(&text), chosen.audio.as_deref(), &media, &media_landed)
        {
            return Ok(Resolved::Beyond(why));
        }
        if stream::is_live(&media) {
            if stream::sample_scrambled(&media) {
                return Ok(Resolved::Beyond(LIVE_SAMPLED));
            }
            return Ok(Resolved::Live(chosen.url, None));
        }
        let listed = stream::media_playlist(&media, &media_landed, url, quality)?;
        return Ok(Resolved::Own(listed, chosen.url, subtitles));
    }
    if let Some(why) = stream::beyond_dm(None, None, &text, &landed) {
        return Ok(Resolved::Beyond(why));
    }
    if !text.contains("#EXTINF") {
        return Err(Error::Internal("that playlist lists no media".into()));
    }
    if stream::is_live(&text) {
        if stream::sample_scrambled(&text) {
            return Ok(Resolved::Beyond(LIVE_SAMPLED));
        }
        return Ok(Resolved::Live(url.to_string(), None));
    }
    let listed = stream::media_playlist(&text, &landed, url, quality)?;
    Ok(Resolved::Own(listed, url.to_string(), Vec::new()))
}

/// A live stream whose samples are scrambled (SAMPLE-AES): recorded as it comes, it is never
/// joined whole, which is when they are put in the clear (gaps row Y16).
const LIVE_SAMPLED: &str = "this live stream's samples are scrambled (SAMPLE-AES)";

/// Why a playlist of a pair (picture and sound apart) is not dm's to join, where it is not: an
/// initialisation segment that changes part-way or samples scrambled by SAMPLE-AES, which only
/// a single playlist's join handles (gaps row Y16).
fn pair_beyond(media: &str, landed: &str) -> Option<&'static str> {
    if stream::changes_map(media, landed) {
        return Some("this stream changes its initialisation segment part-way through");
    }
    stream::sample_scrambled(media).then_some("this stream's samples are scrambled (SAMPLE-AES)")
}

/// A picture and the sound the master keeps apart from it. Fragmented MP4 on
/// both sides is dm's to join (encrypted with AES-128 or cut by byte ranges
/// as it may be), as is a transport-stream picture with packed AAC sound;
/// any other pair is `Beyond`.
async fn resolve_pair(
    client: &reqwest::Client,
    name_from: &str,
    rendition: &str,
    media: &str,
    media_landed: &str,
    sound_url: &str,
    options: &Options,
) -> Result<Resolved, Error> {
    const APART: &str = "this stream carries its sound separately from its picture";
    if let Some(why) = stream::beyond_dm(None, None, media, media_landed)
        .or_else(|| pair_beyond(media, media_landed))
    {
        return Ok(Resolved::Beyond(why));
    }
    if stream::is_live(media) {
        let sound = live_sound(client, name_from, media, media_landed, sound_url, options).await?;
        return Ok(Resolved::Live(rendition.to_string(), sound));
    }
    let (sound, sound_landed) = fetch_playlist(client, sound_url, options).await?;
    if stream::beyond_dm(None, None, &sound, &sound_landed).is_some()
        || pair_beyond(&sound, &sound_landed).is_some()
        || stream::is_live(&sound)
    {
        return Ok(Resolved::Beyond(APART));
    }
    let picture = stream::media_playlist(media, media_landed, name_from, options.quality)?;
    let sound = stream::media_playlist(&sound, &sound_landed, name_from, Quality::Best)?;
    if !joinable(&picture, &sound) {
        return Ok(Resolved::Beyond(APART));
    }
    Ok(Resolved::Pair(
        Box::new(picture),
        Box::new(sound),
        format!("{rendition}\n{sound_url}"),
        Vec::new(),
    ))
}

/// Whether dm joins this picture and sound into one file: fragmented MP4 on
/// both sides, scrambled or not, whole or cut by byte ranges; or a
/// transport-stream picture with packed AAC sound or a transport stream's. Any other mix is
/// beyond.
fn joinable(picture: &stream::Stream, sound: &stream::Stream) -> bool {
    let fragmented = |s: &stream::Stream| s.container == "mp4";
    // Packed AAC, or a transport stream of its own (gaps row Y16).
    let transport = picture.container == "ts" && matches!(sound.container.as_str(), "aac" | "ts");
    (fragmented(picture) && fragmented(sound)) || transport
}

/// The sound a live picture is recorded with: `sound_url`, where its
/// playlist reads and it and the picture are a pair dm joins. `None` where
/// not, and the picture is recorded alone: a recording with no sound is
/// still the stream's picture, where refusing it would leave nothing.
async fn live_sound(
    client: &reqwest::Client,
    name_from: &str,
    media: &str,
    media_landed: &str,
    sound_url: &str,
    options: &Options,
) -> Result<Option<String>, Error> {
    let (sound, sound_landed) = match fetch_playlist(client, sound_url, options).await {
        Ok(fetched) => fetched,
        Err(e @ (Error::Paused | Error::Cancelled)) => return Err(e),
        Err(_) => return Ok(None),
    };
    if stream::beyond_dm(None, None, &sound, &sound_landed).is_some()
        || pair_beyond(&sound, &sound_landed).is_some()
    {
        return Ok(None);
    }
    let picture = stream::media_playlist(media, media_landed, name_from, options.quality);
    let sound = stream::media_playlist(&sound, &sound_landed, name_from, Quality::Best);
    Ok(match (picture, sound) {
        (Ok(picture), Ok(sound)) if joinable(&picture, &sound) => Some(sound_url.to_string()),
        _ => None,
    })
}

/// A playlist's text and the address it was finally served from, which
/// its relative entries are joined against.
pub(super) async fn fetch_playlist(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<(String, String), Error> {
    let fetched = fetch_capped(client, url, MANIFEST_CAP, options).await?;
    Ok((fetched.text(), fetched.landed))
}

/// Discards staged segments that belong to a different rendition.
///
/// Segments are matched to the playlist by index, and an index says nothing
/// about which rendition it came from: with the quality setting changed
/// between a pause and a resume, the first segments were 1080p and the rest
/// 480p, joined as one file and reported Completed. The rendition is written
/// beside the parts, and parts of another are not adopted.
pub(super) async fn keep_only_parts_of(parts_dir: &Path, rendition: &str) -> Result<(), Error> {
    let marker = parts_dir.join("rendition");
    let staged = fs::read_to_string(&marker).await.ok();
    if staged.as_deref() != Some(rendition) {
        let mut listing = fs::read_dir(parts_dir).await?;
        while let Some(entry) = listing.next_entry().await? {
            let _ = fs::remove_file(entry.path()).await;
        }
        fs::write(&marker, rendition).await?;
    }
    Ok(())
}

/// Fetches every segment into its own part file, several at once.
///
/// A segment is a few seconds of video, so one at a time a stream spends
/// most of its time waiting a round trip per segment rather than carrying
/// bytes (on a distant server the link sat idle between them). As many are under way together
/// as Connections per file says (that setting's own bounds hold it), each into its own part named
/// by its index, so arrival order does not matter: the parts are joined by
/// index afterwards, as before. The first failure is the stream's, and the
/// segments still under way are dropped with it.
/// The keys a stream's segments are decrypted with, by their address.
pub(super) type Keys = std::collections::HashMap<String, Vec<u8>>;

/// The remux from a stream's segments where they lie, by the room the disk has (gaps row Y21).
#[cfg(test)]
mod remux_tests;

/// What only this module reaches: a remux whose segments cannot be gathered,
/// and a part's name for a file with no extension.
#[cfg(test)]
mod coverage_tests;
