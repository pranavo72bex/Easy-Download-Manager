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
        Resolved::Live(playlist) => {
            return record_live(client, &playlist, url, options, report).await
        }
        // As a DASH stream is: yt-dlp reads these shapes properly; without
        // it the stream is refused rather than saved wrong and called done.
        Resolved::Beyond(_) if crate::tools::for_video(client).await.is_ok() => {
            return crate::ytdlp::download_ytdlp(url, options, report).await;
        }
        Resolved::Beyond(why) => {
            return Err(Error::Internal(format!(
                "{why}, which needs dm's video helper, and it could not be got ready"
            )));
        }
    };

    fs::create_dir_all(&options.parts_dir).await?;
    keep_only_parts_of(&options.parts_dir, &rendition).await?;
    if let Some(found) = first_segment_is(client, &listed, options).await {
        listed.found_to_be(found, url, options.quality);
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
        // No total: a playlist states none, and a bar that guessed one would
        // be a bar that lies.
        let downloaded = Arc::new(AtomicU64::new(0));
        let meter = report.map(|tx| watch(Arc::clone(&downloaded), None, tx));
        let outcome = fetch_segments(client, &listed, key.as_deref(), options, &downloaded).await;
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
/// more than a segment behind. It ends when the playlist says the stream
/// has, or when the person stops it, which keeps what was recorded rather
/// than pausing: a live stream cannot be resumed from where it was.
pub(super) async fn record_live(
    client: &reqwest::Client,
    playlist: &str,
    named_from: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    fs::create_dir_all(&options.parts_dir).await?;
    let recording = options.parts_dir.join("live.recording");
    let mut out = tokio::io::BufWriter::new(fs::File::create(&recording).await?);
    let downloaded = Arc::new(AtomicU64::new(0));
    let meter = report
        .clone()
        .map(|tx| watch(Arc::clone(&downloaded), None, tx));
    let mut written: Option<u64> = None;
    let mut init_written = false;
    let mut keys: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    let mut filename = String::new();
    let mut misses = 0u32;
    let ended = loop {
        let fetched = match fetch_playlist(client, playlist, options).await {
            Ok(fetched) => fetched,
            Err(e @ (Error::Paused | Error::Cancelled)) => break Err(e),
            // A playlist that does not answer once is asked again; one that
            // stops answering ends the recording with what it has.
            Err(_) if misses < 5 && written.is_some() => {
                misses += 1;
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(e) => break Err(e),
        };
        misses = 0;
        let (text, landed) = fetched;
        let listed = stream::media_playlist(&text, &landed, named_from, options.quality)?;
        if filename.is_empty() {
            filename = listed.filename.clone();
            if let Some(tx) = &report {
                let _ = tx
                    .send(Update::Named {
                        name: filename.clone(),
                        category: category::classify(&filename, None),
                    })
                    .await;
            }
        }
        for part in &listed.parts {
            let wanted = if part.init {
                !init_written
            } else {
                written.is_none_or(|last| part.sequence > last)
            };
            if !wanted {
                continue;
            }
            let body = match retry::run_up_to(&options.control, options.tries(), || {
                fetch_segment_range(client, &part.url, part.range, options)
            })
            .await
            {
                Ok(body) => body,
                // Asked for again with the playlist on the next round, or
                // the stop that ended it seen there.
                Err(_) => break,
            };
            let bytes = match &listed.key {
                Some(declared) if !part.init => {
                    if !keys.contains_key(&declared.uri) {
                        let key = fetch_capped(client, &declared.uri, 1024, options)
                            .await?
                            .body;
                        keys.insert(declared.uri.clone(), key);
                    }
                    let key = keys.get(&declared.uri).map(Vec::as_slice).unwrap_or(&[]);
                    stream::decrypt(key, &stream::iv_for(declared, part.sequence), &body)?
                }
                _ => body,
            };
            out.write_all(&bytes).await?;
            downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            if part.init {
                init_written = true;
            } else {
                written = Some(part.sequence);
            }
        }
        out.flush().await?;
        if !stream::is_live(&text) {
            break Ok(());
        }
        // Half a segment's length, so a new one is not left waiting long.
        let pause = Duration::from_millis(stream::target_duration(&text) * 500);
        if let Err(stop) = or_stop(&options.control, async {
            tokio::time::sleep(pause).await;
            Ok(())
        })
        .await
        {
            break Err(stop);
        }
    };
    out.flush().await?;
    drop(out);
    if let Some((handle, finished)) = meter {
        let _ = finished.send(());
        let _ = handle.await;
    }
    match ended {
        Ok(()) | Err(Error::Paused) => {}
        Err(Error::Cancelled) => {
            let _ = fs::remove_file(&recording).await;
            return Err(Error::Cancelled);
        }
        // Whatever stopped it, what was recorded is kept where there is any.
        Err(e) if written.is_none() => return Err(e),
        Err(_) => {}
    }
    if written.is_none() {
        return Err(Error::Internal(
            "the live stream sent nothing to record".into(),
        ));
    }
    // Stopped by the person: the recording is finished, not paused, so
    // placing it must not wait on the stop that ended it.
    let placing = Options {
        control: crate::control::Control::new(),
        ..options.clone()
    };
    let parts = vec![recording];
    let (parts, filename) = match remux_ts(&filename, &parts, &placing).await {
        Some((mp4, name)) => (vec![mp4], name),
        None => (parts, filename),
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
pub(crate) async fn or_stop<T>(
    control: &Control,
    work: impl std::future::Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::pin!(work);
    loop {
        control.check()?;
        tokio::select! {
            done = &mut work => return done,
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
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
        let gathered = join_parts(parts, &partial).await;
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
    /// A live stream, recorded as it goes: its media playlist's address.
    Live(String),
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
            return Ok(Resolved::Live(chosen.url));
        }
        let listed = stream::media_playlist(&media, &media_landed, url, quality)?;
        return Ok(Resolved::Own(listed, chosen.url));
    }
    if let Some(why) = stream::beyond_dm(None, None, &text, &landed) {
        return Ok(Resolved::Beyond(why));
    }
    if stream::is_live(&text) {
        return Ok(Resolved::Live(url.to_string()));
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
        return Ok(Resolved::Live(rendition.to_string()));
    }
    let (sound, sound_landed) = fetch_playlist(client, sound_url, options).await?;
    if stream::beyond_dm(None, None, &sound, &sound_landed).is_some() || stream::is_live(&sound) {
        return Ok(Resolved::Beyond(APART));
    }
    let picture = stream::media_playlist(media, media_landed, name_from, options.quality)?;
    let sound = stream::media_playlist(&sound, &sound_landed, name_from, Quality::Best)?;
    // Fragmented MP4 on both sides, scrambled or not, whole or cut by byte
    // ranges; or a transport-stream picture with packed AAC sound. Any other
    // mix is still beyond.
    let fragmented = |s: &stream::Stream| s.container == "mp4";
    let packed = picture.container == "ts" && sound.container == "aac";
    if !((fragmented(&picture) && fragmented(&sound)) || packed) {
        return Ok(Resolved::Beyond(APART));
    }
    Ok(Resolved::Pair(
        Box::new(picture),
        Box::new(sound),
        format!("{rendition}\n{sound_url}"),
    ))
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

/// Fetches every segment, in order, into its own part file.
///
/// Sequential on purpose. Segments arrive already split, so there is nothing
/// to gain from asking one server for eight at once beyond what its own
/// per-connection limit allows, and a stream fetched out of order is a file
/// that has to be held in memory to be put back in order.
pub(super) async fn fetch_segments(
    client: &reqwest::Client,
    listed: &stream::Stream,
    key: Option<&[u8]>,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<(), Error> {
    for (index, part) in listed.parts.iter().enumerate() {
        let done = part_path(&options.parts_dir, index);

        /* Written under a temporary name and moved when whole.
         *
         * A segment has no declared length, so a part file that is present
         * says nothing about whether it finished — half of one looks exactly
         * like all of one. Only a completed segment is ever given its real
         * name, which is what makes resuming a stream possible at all. */
        if let Ok(meta) = fs::metadata(&done).await {
            downloaded.fetch_add(meta.len(), Ordering::Relaxed);
            continue;
        }

        options.control.check()?;

        let staging = done.with_extension("partial");
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
    }
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
