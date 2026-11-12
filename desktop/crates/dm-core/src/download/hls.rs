// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

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
    let (mut listed, rendition) = match resolve_stream(client, url, options).await? {
        Resolved::Own(listed, rendition) => (listed, rendition),
        Resolved::Pair(picture, sound, rendition) => {
            return super::dash::download_pair(
                client, &picture, &sound, &rendition, options, report,
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
    if let Some(span) = options.span {
        let (covered, _, _) = stream::cover(&listed, span.from, span.to).ok_or_else(|| {
            Error::Internal("that part is outside the stream's length".to_string())
        })?;
        rendition = format!("{rendition}|{}", span.words());
        listed = covered;
    }

    fs::create_dir_all(&options.parts_dir).await?;
    keep_only_parts_of(&options.parts_dir, &rendition).await?;
    if let Some(found) = first_segment_is(client, &listed, options).await {
        listed.found_to_be(found, url, options.quality);
    }
    // Trimmed exactly only where the muxer can: a fragmented MP4.
    let cut = options
        .span
        .filter(|_| listed.container == "mp4" || listed.container == "m4a")
        .map(|span| span.millis());
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

    let key = match &listed.key {
        // Once, not once per segment: a playlist of a thousand segments
        // would otherwise be a thousand requests for the same sixteen bytes.
        Some(key) => Some(fetch_capped(client, &key.uri, 1024, options).await?.body),
        None => None,
    };

    fs::create_dir_all(&options.dest_dir).await?;

    /* Stopped after every segment had arrived — paused during the remux, or
     * dm quit or killed then — the segments are in the gathered stream, and
     * once remuxed, in the MP4: neither is fetched again. The gathered
     * stream is named only once whole (it is joined under `.partial`), and
     * removed only once the MP4 is, so whichever is here is complete. */
    let gathered = options.parts_dir.join(GATHERED);
    let remuxed = options.parts_dir.join(REMUXED);
    // Only where the remux would run now: turned off since, or with a
    // digest that names the `.ts` bytes, an MP4 from before is not the file
    // asked for, and the stream is fetched again from its segments.
    let remuxing = remux_wanted(&listed.filename, options);
    if !remuxing {
        let _ = fs::remove_file(&remuxed).await;
    }
    if remuxing
        && fs::metadata(&gathered).await.is_err()
        && fs::metadata(&remuxed).await.is_ok_and(|m| m.len() > 0)
    {
        let stem = ts_stem(&listed.filename);
        let (path, bytes, category) =
            finish(&format!("{stem}.mp4"), None, &[remuxed], options).await?;
        return Ok(Finished {
            left_behind: None,
            path,
            bytes,
            connections: 1,
            category,
            verified: options.checksum.is_some(),
            mirrors: 1,
        });
    }

    let parts: Vec<PathBuf> = if fs::metadata(&gathered).await.is_ok() {
        vec![gathered]
    } else {
        // A playlist states no size: the row shows one about, from the pieces fetched so far
        // and how many there are, marked as such (gaps row Y22), rather than none at all.
        let downloaded = Arc::new(AtomicU64::new(0));
        let estimate = Arc::new(AtomicU64::new(0));
        let meter = report.map(|tx| {
            crate::download::ranged::watch_estimating(
                Arc::clone(&downloaded),
                None,
                Some(Arc::clone(&estimate)),
                tx,
            )
        });
        let outcome = fetch_segments(
            client,
            &listed,
            key.as_deref(),
            options,
            &downloaded,
            Some(&estimate),
        )
        .await;
        if let Some((handle, finished)) = meter {
            let _ = finished.send(());
            let _ = handle.await;
        }
        outcome?;
        (0..listed.parts.len())
            .map(|i| part_path(&options.parts_dir, i))
            .collect()
    };
    // A transport stream into an MP4 where asked and able; the `.ts` where
    // not, which is still the stream, whole.
    let (parts, filename) = match remux_ts(&listed.filename, &parts, options).await {
        Some((mp4, name)) => (vec![mp4], name),
        None => (parts, listed.filename.clone()),
    };
    let parts = match cut {
        Some(window) => vec![cut_fragments(&parts, window, options).await?],
        None => parts,
    };
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

/// A live stream recorded as it goes: its playlist asked for again at the
/// pace its segments come (half their length), each segment not yet
/// written fetched once, by its sequence number, decrypted where it is
/// scrambled, and added to one growing file, so what is recorded is never
/// more than a segment behind. Where the master keeps the sound in a
/// playlist of its own (`sound`), that playlist is recorded the same way
/// into a file of its own, asked for in the same rounds just after the
/// picture's, and the two are joined into one MP4 when the recording ends.
/// It ends when the picture's playlist says the stream has, or when the
/// person stops it, which keeps what was recorded rather than pausing: a
/// live stream cannot be resumed from where it was.
pub(super) async fn record_live(
    client: &reqwest::Client,
    playlist: &str,
    sound: Option<&str>,
    named_from: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    fs::create_dir_all(&options.parts_dir).await?;
    let sound_file = options.parts_dir.join("live.sound");
    let mut picture = LiveTrack::open(playlist, options.parts_dir.join("live.recording")).await?;
    let mut heard = match sound {
        Some(url) => Some(LiveTrack::open(url, sound_file.clone()).await?),
        None => None,
    };
    let downloaded = Arc::new(AtomicU64::new(0));
    let meter = report
        .clone()
        .map(|tx| watch(Arc::clone(&downloaded), None, tx));
    let mut filename = String::new();
    // Pieces of the picture the row has been told were missed.
    let mut told: u64 = 0;
    let ended = loop {
        let round = picture
            .round(client, named_from, options.quality, options, &downloaded)
            .await;
        // Pieces the playlist moved past before dm fetched them: the recording has a gap
        // there, which the row says (gaps row Y23).
        if picture.missed > told {
            told = picture.missed;
            if let Some(tx) = &report {
                let said = crate::download::NOTES[1].replace("{}", &told.to_string());
                let _ = tx.send(Update::Note(said)).await;
            }
        }
        let round = match round {
            Ok(round) => round,
            Err(e) => break Err(e),
        };
        if filename.is_empty() {
            if let Some(name) = &picture.named {
                filename = name.clone();
                if let Some(tx) = &report {
                    let _ = tx
                        .send(Update::Named {
                            name: filename.clone(),
                            category: category::classify(&filename, None),
                        })
                        .await;
                }
            }
        }
        /* The sound just after the picture, so it is taken at least as far
         * as the picture got, on the picture's last round too. A sound
         * playlist that fails five rounds running is left where it got to,
         * and the picture goes on without it. */
        if let Some(track) = heard.as_mut().filter(|t| !t.done) {
            match track
                .round(client, named_from, Quality::Best, options, &downloaded)
                .await
            {
                Ok(Round::Ended) => track.done = true,
                Ok(_) => {}
                Err(e @ (Error::Paused | Error::Cancelled)) => break Err(e),
                Err(_) => {
                    track.misses += 1;
                    track.done = track.misses > 5;
                }
            }
        }
        let pause = match round {
            Round::Ended => break Ok(()),
            // A playlist that did not answer is asked again shortly.
            Round::Missed => Duration::from_secs(2),
            Round::More(pause) => pause,
        };
        if let Err(stop) = or_stop(&options.control, async {
            tokio::time::sleep(pause).await;
            Ok(())
        })
        .await
        {
            break Err(stop);
        }
    };
    let flushed = picture.out.flush().await;
    let picture_at = picture.path.clone();
    let recorded = picture.written.is_some();
    drop(picture);
    // The sound is joined only where some of it was written whole.
    let sound_at = match heard {
        Some(mut track) => {
            let whole = track.out.flush().await.is_ok();
            (whole && track.written.is_some()).then(|| track.path.clone())
        }
        None => None,
    };
    if let Some((handle, finished)) = meter {
        let _ = finished.send(());
        let _ = handle.await;
    }
    match ended {
        Ok(()) | Err(Error::Paused) => {}
        Err(Error::Cancelled) => {
            let _ = fs::remove_file(&picture_at).await;
            let _ = fs::remove_file(&sound_file).await;
            return Err(Error::Cancelled);
        }
        // Whatever stopped it, what was recorded is kept where there is any.
        Err(e) if !recorded => {
            let _ = fs::remove_file(&sound_file).await;
            return Err(e);
        }
        Err(_) => {}
    }
    if !recorded {
        let _ = fs::remove_file(&sound_file).await;
        return Err(Error::Internal(
            "the live stream sent nothing to record".into(),
        ));
    }
    flushed?;
    // Stopped by the person: the recording is finished, not paused, so
    // placing it must not wait on the stop that ended it.
    let placing = Options {
        control: crate::control::Control::new(),
        ..options.clone()
    };
    let joined = match &sound_at {
        Some(sound) => join_live(&picture_at, sound, &filename, &placing).await,
        None => None,
    };
    let (parts, filename) = match joined {
        Some((mp4, name)) => (vec![mp4], name),
        None => {
            let parts = vec![picture_at];
            match remux_ts(&filename, &parts, &placing).await {
                Some((mp4, name)) => (vec![mp4], name),
                None => (parts, filename),
            }
        }
    };
    let (path, bytes, category) = finish(&filename, None, &parts, &placing).await?;
    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: false,
        mirrors: 1,
    })
}

/// What one round of asking a live playlist came to.
enum Round {
    /// Taken as far as it goes; more is to come, worth asking for after
    /// this long.
    More(Duration),
    /// The playlist says the stream has ended: everything in it is taken.
    Ended,
    /// The playlist did not answer this time; it is asked again.
    Missed,
}

/// One playlist of a live stream being recorded: where it is asked for,
/// the file its segments are added to, and how far it has got.
struct LiveTrack {
    playlist: String,
    path: PathBuf,
    out: tokio::io::BufWriter<fs::File>,
    /// The sequence number of the last segment written.
    written: Option<u64>,
    init_written: bool,
    /// Each key fetched once, by its address.
    keys: std::collections::HashMap<String, Vec<u8>>,
    /// Rounds in a row the playlist did not answer.
    misses: u32,
    /// Segments the playlist moved past before they were fetched.
    missed: u64,
    /// Asked for no more: its playlist ended, or stopped answering.
    done: bool,
    /// The name its first playlist gave the recording.
    named: Option<String>,
}

impl LiveTrack {
    async fn open(playlist: &str, path: PathBuf) -> Result<Self, Error> {
        let out = tokio::io::BufWriter::new(fs::File::create(&path).await?);
        Ok(LiveTrack {
            playlist: playlist.to_string(),
            path,
            out,
            written: None,
            init_written: false,
            keys: std::collections::HashMap::new(),
            misses: 0,
            missed: 0,
            done: false,
            named: None,
        })
    }

    /// The playlist asked for once, and each segment in it not yet written
    /// fetched and added to the file. A playlist that does not answer once
    /// is a miss, asked again; one that stops answering (five misses), or
    /// fails before anything was recorded, is the error that ends it. A
    /// segment that cannot be had ends the round, and is asked for again
    /// with the playlist on the next.
    async fn round(
        &mut self,
        client: &reqwest::Client,
        named_from: &str,
        quality: Quality,
        options: &Options,
        downloaded: &AtomicU64,
    ) -> Result<Round, Error> {
        let (text, landed) = match fetch_playlist(client, &self.playlist, options).await {
            Ok(fetched) => fetched,
            Err(e @ (Error::Paused | Error::Cancelled)) => return Err(e),
            Err(_) if self.misses < 5 && self.written.is_some() => {
                self.misses += 1;
                return Ok(Round::Missed);
            }
            Err(e) => return Err(e),
        };
        self.misses = 0;
        let listed = stream::media_playlist(&text, &landed, named_from, quality)?;
        if self.named.is_none() {
            self.named = Some(listed.filename.clone());
        }
        for part in &listed.parts {
            let wanted = if part.init {
                !self.init_written
            } else {
                self.written.is_none_or(|last| part.sequence > last)
            };
            if !wanted {
                continue;
            }
            let Ok(body) = retry::run_up_to(&options.control, options.tries(), || {
                fetch_segment_range(client, &part.url, part.range, options)
            })
            .await
            else {
                break;
            };
            let bytes = match &listed.key {
                Some(declared) if !part.init => {
                    if !self.keys.contains_key(&declared.uri) {
                        let key = fetch_capped(client, &declared.uri, 1024, options)
                            .await?
                            .body;
                        self.keys.insert(declared.uri.clone(), key);
                    }
                    let key = self
                        .keys
                        .get(&declared.uri)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    stream::decrypt(key, &stream::iv_for(declared, part.sequence), &body)?
                }
                _ => body,
            };
            self.out.write_all(&bytes).await?;
            downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            if part.init {
                self.init_written = true;
            } else {
                if let Some(last) = self
                    .written
                    .filter(|last| part.sequence > last.saturating_add(1))
                {
                    self.missed += part.sequence - last - 1;
                }
                self.written = Some(part.sequence);
            }
        }
        self.out.flush().await?;
        if !stream::is_live(&text) {
            return Ok(Round::Ended);
        }
        // Half a segment's length, so a new one is not left waiting long.
        Ok(Round::More(Duration::from_millis(
            stream::target_duration(&text) * 500,
        )))
    }
}

/// A live picture and its sound, each recorded into a file of its own,
/// joined into one MP4 beside them: a transport stream with packed AAC by
/// the transport-stream muxer, fragmented MP4 by the MP4 muxer. With the
/// name the joined file is placed under. `None` where they cannot be
/// joined: the picture is then kept alone, as a recording cannot be asked
/// for again.
async fn join_live(
    picture: &Path,
    sound: &Path,
    filename: &str,
    options: &Options,
) -> Option<(PathBuf, String)> {
    let joined = options.parts_dir.join("live.joined.mp4");
    let _ = fs::remove_file(&joined).await;
    let (picture_owned, sound_owned) = (picture.to_path_buf(), sound.to_path_buf());
    let made = if crate::tsmux::is_transport_stream(picture) && crate::tsmux::is_packed_aac(sound) {
        crate::tsmux::join_packed_async(picture_owned, sound_owned, joined.clone()).await
    } else {
        crate::mp4mux::join_async(vec![picture_owned, sound_owned], joined.clone()).await
    };
    let _ = fs::remove_file(sound).await;
    if made.is_ok() && fs::metadata(&joined).await.is_ok_and(|m| m.len() > 0) {
        let _ = fs::remove_file(picture).await;
        return Some((joined, format!("{}.mp4", media_stem(filename))));
    }
    let _ = fs::remove_file(&joined).await;
    None
}

/// A recording's name without its `.ts`, `.mp4` or `.m4a`, in any case.
fn media_stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    for extension in [".ts", ".mp4", ".m4a"] {
        if lower.ends_with(extension) {
            return name.get(..name.len() - extension.len()).unwrap_or(name);
        }
    }
    name
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
    if listed.key.is_some() || listed.parts.iter().any(|p| p.init) {
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
/// stream.
pub(super) async fn remux_ts(
    name: &str,
    parts: &[PathBuf],
    options: &Options,
) -> Option<(PathBuf, String)> {
    if !remux_wanted(name, options) {
        return None;
    }
    options.control.check().ok()?;
    let joined = options.parts_dir.join(GATHERED);
    let mp4 = options.parts_dir.join(REMUXED);
    /* The segments are gathered into one stream with each freed as it is
     * consumed, as the ordinary join does: segments, a joined copy and the
     * MP4 side by side needed three times the file at the end. Now the peak
     * is the stream and its MP4. Joined under `.partial` and named only once
     * whole, so a resume that finds the name finds every segment in it; a
     * join cut short leaves the segments it had not reached, and the ones it
     * had are fetched again. Already gathered (a resume), it is used as is. */
    if parts != [joined.clone()] {
        let partial = options.parts_dir.join(format!("{GATHERED}.partial"));
        let gathered = join_parts(parts, &partial, &options.control).await;
        if gathered.is_err() {
            let _ = fs::remove_file(&partial).await;
            return None;
        }
        if fs::rename(&partial, &joined).await.is_err() {
            return None;
        }
    }
    let ts_name = name.to_string();
    let keep_ts = || async {
        // The stream, whole, as the one part to place: what not remuxing
        // would have saved.
        Some((joined.clone(), ts_name.clone()))
    };
    if options.control.check().is_err() {
        return keep_ts().await;
    }
    /* Not raced against a pause: it runs at the disk's speed, and a blocking task cut loose would go on writing the file a
     * resume makes afresh. */
    let ours = crate::tsmux::to_mp4_async(joined.clone(), mp4.clone()).await;
    if ours.is_ok() && fs::metadata(&mp4).await.is_ok_and(|m| m.len() > 0) {
        let _ = fs::remove_file(&joined).await;
        return Some((mp4, format!("{}.mp4", ts_stem(name))));
    }
    // A stream dm cannot put into an MP4 (an MPEG-2 picture) is kept as
    // the transport stream it came as, which plays as it is.
    let _ = fs::remove_file(&mp4).await;
    keep_ts().await
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
    Own(stream::Stream, String),
    /// A live stream, recorded as it goes: its media playlist's address,
    /// and its sound's where the master keeps that in a playlist of its own
    /// that dm can join to the picture.
    Live(String, Option<String>),
    /// Why dm does not assemble it itself (`stream::beyond_dm`).
    Beyond(&'static str),
    /// A picture and its sound as two playlists of fragmented MP4, which dm
    /// fetches as two tracks and joins; the rendition's address names them.
    Pair(Box<stream::Stream>, Box<stream::Stream>, String),
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
        if let Some(sound_url) = apart {
            return resolve_pair(
                client,
                url,
                &chosen.url,
                &media,
                &media_landed,
                &sound_url,
                options,
            )
            .await;
        }
        if let Some(why) =
            stream::beyond_dm(Some(&text), chosen.audio.as_deref(), &media, &media_landed)
        {
            return Ok(Resolved::Beyond(why));
        }
        if stream::is_live(&media) {
            return Ok(Resolved::Live(chosen.url, None));
        }
        let listed = stream::media_playlist(&media, &media_landed, url, quality)?;
        return Ok(Resolved::Own(listed, chosen.url));
    }
    if let Some(why) = stream::beyond_dm(None, None, &text, &landed) {
        return Ok(Resolved::Beyond(why));
    }
    if !text.contains("#EXTINF") {
        return Err(Error::Internal("that playlist lists no media".into()));
    }
    if stream::is_live(&text) {
        return Ok(Resolved::Live(url.to_string(), None));
    }
    let listed = stream::media_playlist(&text, &landed, url, quality)?;
    Ok(Resolved::Own(listed, url.to_string()))
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
    if let Some(why) = stream::beyond_dm(None, None, media, media_landed) {
        return Ok(Resolved::Beyond(why));
    }
    if stream::is_live(media) {
        let sound = live_sound(client, name_from, media, media_landed, sound_url, options).await?;
        return Ok(Resolved::Live(rendition.to_string(), sound));
    }
    let (sound, sound_landed) = fetch_playlist(client, sound_url, options).await?;
    if stream::beyond_dm(None, None, &sound, &sound_landed).is_some() || stream::is_live(&sound) {
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
    ))
}

/// Whether dm joins this picture and sound into one file: fragmented MP4 on
/// both sides, scrambled or not, whole or cut by byte ranges; or a
/// transport-stream picture with packed AAC sound. Any other mix is beyond.
fn joinable(picture: &stream::Stream, sound: &stream::Stream) -> bool {
    let fragmented = |s: &stream::Stream| s.container == "mp4";
    let packed = picture.container == "ts" && sound.container == "aac";
    (fragmented(picture) && fragmented(sound)) || packed
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
    if stream::beyond_dm(None, None, &sound, &sound_landed).is_some() {
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
pub(super) async fn fetch_segments(
    client: &reqwest::Client,
    listed: &stream::Stream,
    key: Option<&[u8]>,
    options: &Options,
    downloaded: &AtomicU64,
    estimate: Option<&AtomicU64>,
) -> Result<(), Error> {
    // Workers taking the next unfetched index, rather than a stream of
    // futures: each worker is one plain future, which keeps this `Send`.
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let worker = || async {
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(part) = listed.parts.get(index) else {
                return Ok::<(), Error>(());
            };
            fetch_one_segment(client, listed, key, options, downloaded, index, part).await?;
            // The stream's size, about: what its pieces so far came to, as many times over as
            // it has pieces (gaps row Y22). Surer with each piece; from the second.
            let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(estimate) = estimate.filter(|_| finished >= 2) {
                let so_far = downloaded.load(Ordering::Relaxed);
                let pieces = listed.parts.len() as u64;
                estimate.store(so_far / finished as u64 * pieces, Ordering::Relaxed);
            }
        }
    };
    // Connections per file, as a file's ranges take them: a fixed eight held a stream there
    // whatever the setting said (gaps row Y23).
    let at_once = options.connections.max(1);
    futures_util::future::try_join_all((0..at_once).map(|_| worker())).await?;
    Ok(())
}

async fn fetch_one_segment(
    client: &reqwest::Client,
    listed: &stream::Stream,
    key: Option<&[u8]>,
    options: &Options,
    downloaded: &AtomicU64,
    index: usize,
    part: &stream::Part,
) -> Result<(), Error> {
    let done = part_path(&options.parts_dir, index);

    /* Written under a temporary name and moved when whole.
     *
     * A segment has no declared length, so a part file that is present
     * says nothing about whether it finished — half of one looks exactly
     * like all of one. Only a completed segment is ever given its real
     * name, which is what makes resuming a stream possible at all. */
    if let Ok(meta) = fs::metadata(&done).await {
        downloaded.fetch_add(meta.len(), Ordering::Relaxed);
        return Ok(());
    }

    options.control.check()?;

    let staging = done.with_extension("partial");
    let client = &crate::second_path::client_for_piece(client, &options.parts_dir, index);
    let fetched = retry::run_up_to(&options.control, options.tries(), || {
        fetch_segment_range(client, &part.url, part.range, options)
    })
    .await?;

    let bytes = match (key, &listed.key) {
        (Some(key), Some(declared)) if !part.init => {
            // The initialisation segment is never encrypted.
            stream::decrypt(key, &stream::iv_for(declared, part.sequence), &fetched)?
        }
        _ => fetched,
    };

    downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    fs::write(&staging, &bytes).await?;
    fs::rename(&staging, &done).await?;
    Ok(())
}

/// One segment, whole or the bytes of a file that it is, held because it
/// has to be decrypted before it can be kept: exactly the bytes asked for,
/// or an error. A server that ignores the range and sends the whole file
/// would otherwise be written in as the piece.
pub(super) async fn fetch_segment_range(
    client: &reqwest::Client,
    url: &str,
    range: Option<(u64, u64)>,
    options: &Options,
) -> Result<Vec<u8>, Error> {
    /// The largest a segment is sized for up front. A segment grown by
    /// doubling from empty is copied about fourteen times on the way, so the
    /// hint is worth taking — but only as far as a real segment goes.
    /// Content-Length is the server's claim, and a claim of a terabyte would
    /// otherwise be an allocation of a terabyte.
    const HINT_CAP: u64 = 32 * 1024 * 1024;

    let mut request = get(
        client,
        url,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    );
    if let Some((first, last)) = range {
        request = request.header(reqwest::header::RANGE, format!("bytes={first}-{last}"));
    }
    let res = or_stop(&options.control, crate::auth::send(client, request, url)).await?;
    let res = Error::refused_or(res).await?;
    if range.is_some() && res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(Error::Internal(format!(
            "{url} did not honour the byte range this stream is cut by"
        )));
    }
    let hint = res.content_length().unwrap_or(0).min(HINT_CAP);
    let mut body = Vec::with_capacity(hint as usize);
    let mut chunks = res.bytes_stream();
    while let Some(chunk) = next_or_stop(&mut chunks, &options.control).await? {
        let chunk = chunk?;
        // A segment is seconds of video. One that keeps coming is not one,
        // and is not held in memory until the process runs out.
        if body.len() + chunk.len() > SEGMENT_CAP {
            return Err(Error::Internal(format!(
                "{url} is far larger than a stream's segment should be"
            )));
        }
        body.extend_from_slice(&chunk);
        or_stop(&options.control, async {
            options.limiter.take(chunk.len() as u64).await;
            Ok(())
        })
        .await?;
    }
    if let Some((first, last)) = range {
        if body.len() as u64 != last - first + 1 {
            return Err(Error::Internal(format!(
                "{url} answered a byte range with the wrong number of bytes"
            )));
        }
    }
    Ok(body)
}

/// A file name with the span it holds before its extension:
/// "show.mp4" -> "show (1m30s-2m05s).mp4".
pub(super) fn named_for_span(name: &str, span: crate::clip::Span) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 => {
            format!("{stem} ({}).{ext}", span.words())
        }
        _ => format!("{name} ({})", span.words()),
    }
}

/// The fetched fragments of a fragmented MP4 (its initialisation segment
/// first) joined, then cut to `window` (milliseconds) by the muxer's edit
/// lists, into one file in the parts directory.
async fn cut_fragments(
    parts: &[PathBuf],
    window: (u64, u64),
    options: &Options,
) -> Result<PathBuf, Error> {
    let whole = options.parts_dir.join("cut-input.mp4");
    let out = options.parts_dir.join("cut-output.mp4");
    let _ = fs::remove_file(&whole).await;
    let _ = fs::remove_file(&out).await;
    join_parts_copy(parts, &whole).await?;
    crate::mp4mux::join_cut_async(
        vec![whole.clone()],
        Vec::new(),
        out.clone(),
        Some(window),
        options.shape,
    )
    .await
    .map_err(Error::Internal)?;
    let _ = fs::remove_file(&whole).await;
    Ok(out)
}

/// The files one after another into `into`, leaving them in place.
pub(super) async fn join_parts_copy(parts: &[PathBuf], into: &Path) -> Result<(), Error> {
    use tokio::io::AsyncWriteExt;
    let mut out = tokio::io::BufWriter::new(fs::File::create(into).await?);
    for part in parts {
        let mut input = fs::File::open(part).await?;
        tokio::io::copy(&mut input, &mut out).await?;
    }
    out.flush().await?;
    Ok(())
}

/// What only this module reaches: a remux whose segments cannot be gathered,
/// and a part's name for a file with no extension.
#[cfg(test)]
mod coverage_tests {
    use super::*;
    use crate::{Categories, Limiter};

    fn options(dir: &Path) -> Options {
        Options {
            connections: 1,
            control: Control::new(),
            dest_dir: dir.join("out"),
            parts_dir: dir.join("parts"),
            limiter: Limiter::unlimited(),
            room: Default::default(),
            categories: Categories::default(),
            checksum: None,
            quality: crate::stream::Quality::Best,
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            media: false,
            referrer: None,
            proxy: None,
            container: None,
            attempts: 1,
            host_limits: Vec::new(),
            proxy_bypass: Vec::new(),
            subtitles: false,
            sign_in: None,
            save_name: None,
            accept_page: false,
            torrent: Default::default(),
            system_proxy: false,
            title_hint: None,
            remux: true,
            server_dates: false,
            retry_wait: 0,
            record_limit: None,
            frame_rate: None,
            caption: None,
        }
    }

    #[tokio::test]
    async fn segments_that_cannot_be_gathered_are_left_for_the_ordinary_join() {
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        fs::create_dir_all(&options.parts_dir).await.unwrap();
        // A segment that is not there: nothing is gathered, and the
        // half-gathered stream is not left behind.
        let gone = options.parts_dir.join("part-0000");
        assert_eq!(remux_ts("show.ts", &[gone], &options).await, None);
        assert!(!options.parts_dir.join("joined.ts.partial").exists());
        assert!(!options.parts_dir.join(GATHERED).exists());

        // Gathered, but its name is taken by a folder that cannot be
        // replaced: the parts stay as they are.
        let part = options.parts_dir.join("part-0001");
        fs::write(&part, b"segment").await.unwrap();
        let taken = options.parts_dir.join(GATHERED);
        fs::create_dir_all(taken.join("inside")).await.unwrap();
        assert_eq!(
            remux_ts("show.ts", std::slice::from_ref(&part), &options).await,
            None
        );
        assert!(taken.is_dir());

        // Not a transport stream, or not asked: never touched.
        assert_eq!(
            remux_ts("show.mp4", std::slice::from_ref(&part), &options).await,
            None
        );
        let off = Options {
            remux: false,
            ..options.clone()
        };
        assert_eq!(remux_ts("show.ts", &[part], &off).await, None);
    }

    #[test]
    fn a_part_of_a_file_with_no_extension_is_named_after_the_whole_name() {
        let span = crate::clip::Span::new(90.0, 125.0).unwrap();
        let words = span.words();
        assert_eq!(named_for_span("stream", span), format!("stream ({words})"));
        // A leading dot is no extension, nor is a long tail after a dot.
        assert_eq!(
            named_for_span(".hidden", span),
            format!(".hidden ({words})")
        );
        assert_eq!(
            named_for_span("show.episode", span),
            format!("show.episode ({words})")
        );
        assert_eq!(
            named_for_span("show.mp4", span),
            format!("show ({words}).mp4")
        );
        assert_eq!(ts_stem("Show.TS"), "Show");
        assert_eq!(ts_stem("show.mp4"), "show.mp4");
    }
}
