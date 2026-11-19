// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Fetching one address: the path it takes (a picture and its sound, a torrent, FTP, a live
//! stream, a manifest, a page, or a file in ranges), and a file's ranges run, checked and
//! put together.

use std::ops::ControlFlow;

use super::*;

/// Fetches `url`: by a path of its own where it is no plain file (a picture and its sound, a
/// torrent, FTP, a live stream, a manifest, a page), else probed and fetched in ranges.
pub(super) async fn fetch_reporting(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    // Before anything is fetched, by every route below.
    writable(&options.dest_dir).await?;

    // A picture and a sound a player fetched apart: both at once, then joined.
    if let Some(sound) = options.sound.as_deref() {
        return paired_or_page(client, url, sound, options, report).await;
    }

    /* A torrent is not fetched from a server at all — there is nothing to
     * probe, no length to split into ranges, no address to resume from. It
     * takes a path of its own, before the probe a URL over HTTP would get. */
    if crate::torrent::is_torrent(url) {
        return crate::torrent::download_torrent(client, url, options, report).await;
    }
    // FTP has no HTTP probe either: it asks the server itself.
    if crate::ftp::is_ftp(url) {
        return crate::ftp::download_ftp(url, options, report).await;
    }
    // Nor do RTMP, RTSP and MMS streams, which dm records itself.
    if crate::live::is_live(url) {
        return crate::live::download_live(url, options, report).await;
    }

    /* A file-storage service's share address (Dropbox, Google Drive, MediaFire) is a page
     * about the file: what its Download button fetches is fetched in its place, under the
     * name the service gives the file, a large Drive file's "can't scan for viruses" question
     * answered (gaps row W7). Found again at each start, as the address a service gives for
     * the file may not last. Every other address is fetched as it is. */
    let found = crate::filehost::file_address(client, url);
    let shared = or_stop(&options.control, found).await?;
    let url = shared.as_deref().unwrap_or(url);

    // A site with a limit of its own is fetched on that many connections,
    // whatever the setting says; decided once, here, for every path below. So is a server found
    // in this run of dm to serve no more at once (`agents::remember_ceiling`).
    let options = &Options {
        connections: options
            .connections_for(url)
            .min(crate::agents::ceiling_for(url).unwrap_or(usize::MAX)),
        ..options.clone()
    };

    let (mut info, mut first_response) = match probe_file(client, url, options, &report).await? {
        ControlFlow::Break(done) => return Ok(done),
        ControlFlow::Continue(found) => found,
    };

    match elsewhere(url, &info, options)? {
        Some(Elsewhere::Mp4Part) => {
            return plain::download_mp4_part(client, url, &info, options, report).await;
        }
        Some(Elsewhere::Dash) => return dash::download_dash(client, url, options, report).await,
        Some(Elsewhere::Stream) => return download_stream(client, url, options, report).await,
        None => {}
    }

    if let Some(done) = page_answer(client, url, &mut info, options, &report).await? {
        return Ok(done);
    }

    let mut checksum = options.checksum.clone();
    let mut listed_mirrors = false;
    let mut sources = vec![Source {
        url: info.final_url.clone(),
        validator: info.validator.clone(),
    }];

    /* A mirror list rather than a file. What was probed is a few hundred
     * bytes of XML; the download this call is really about has not started
     * yet, so everything gathered so far is replaced by what the document
     * names. */
    if metalink::is_metalink(url, info.content_type.as_deref()) {
        first_response = None;
        let (mirrors, listed) = from_metalink(client, options, &mut info).await?;
        sources = mirrors;
        // The caller's digest wins: it is the one a person typed in, and a
        // Metalink is a document from the same place as the file.
        checksum = checksum.or(listed);
        listed_mirrors = true;
    }

    let found = Found {
        info,
        first_response,
        sources,
        checksum,
        listed_mirrors,
    };
    fetch_found(client, url, options, report, found).await
}

/// A picture and a sound a player fetched apart: both at once, then joined; read from the
/// YouTube page instead where the player's streams are refused.
async fn paired_or_page(
    client: &reqwest::Client,
    url: &str,
    sound: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let paired = dash::download_media_pair(client, url, sound, options, report.clone()).await;
    /* Addresses taken from a player can stop serving anyone but that
     * player (YouTube's ask for a proof-of-origin token after about a
     * megabyte): a YouTube page is read by dm itself, and its streams
     * fetched by dm all the same. */
    let page = options
        .referrer
        .as_deref()
        .filter(|page| page.starts_with("https://") || page.starts_with("http://"));
    match (paired, page) {
        (Err(why), Some(page))
            if !matches!(why, Error::Paused | Error::Cancelled)
                && crate::youtube::video_id(page).is_some() =>
        {
            crate::note!(
                info,
                "dm: the player's streams were refused ({why}); reading {page} instead"
            );
            discard_staged(&options.parts_dir).await;
            let page_options = Options {
                sound: None,
                ..options.clone()
            };
            crate::videopage::download_page(client, page, &page_options, report).await
        }
        (outcome, _) => outcome,
    }
}

/// The probe's word on the file, and its answer for range 0, the body not yet read.
type Probed = (Probe, Option<ranged::First>);

/// The probe, retried as a range would be: its word on the file and its answer, the body
/// not yet read; or, a YouTube page refused to dm's own request, the page read instead.
async fn probe_file(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: &Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<ControlFlow<Finished, Probed>, Error> {
    // The probe is as worth retrying as anything after it: a server that is
    // briefly busy should not fail a download before it has begun. Each
    // retry is said, as a range's is.
    let retrying = |attempt, of, error: &Error, wait| {
        if let Some(tx) = report {
            let why = error.to_string();
            let _ = tx.try_send(Update::Retrying {
                attempt,
                of,
                why,
                wait,
            });
        }
    };
    /* On several connections the first request asks for the file's first part only
     * (`probe::FIRST_ASK`): where range 0 is longer, the answer ends inside it and range 0's
     * connection asks for the rest of its range on. An answer running to the file's end went on
     * past range 0's end until dm let go of it, megabytes sent for nothing on a fast link (gaps
     * row S9). On one connection range 0 is the whole file, and the first answer runs to its end
     * as ever. Nor under a cap (`Job::chunk`): the answer is not read there, and a server that
     * caps each request is not asked for more than its cap in one. */
    let capped = HINTS.try_with(|hints| hints.chunk).ok().flatten().is_some();
    let first_ask = (options.connections > 1 && !capped).then_some(crate::probe::FIRST_ASK);
    let probed = retry::run_up_to_telling(&options.control, options.tries(), retrying, || {
        or_stop(
            &options.control,
            probe_get(
                client,
                url,
                options.referrer.as_deref(),
                options.sign_in.as_ref(),
                first_ask,
            ),
        )
    })
    .await;
    /* A YouTube page refused to dm's own request (a 429 bot check, a 403
     * to a client that is not a browser) is read by dm's YouTube reader,
     * which asks as a different client. Only YouTube; other sites' videos
     * come from the browser extension's caught requests. */
    let probed = match probed {
        Err(
            refused @ Error::Refused {
                status: 403 | 429, ..
            },
        ) if !is_stream() && !names_a_document(url) && crate::youtube::video_id(url).is_some() => {
            return match crate::videopage::download_page(client, url, options, report.clone()).await
            {
                Ok(done) => Ok(ControlFlow::Break(done)),
                Err(stopped @ (Error::Paused | Error::Cancelled)) => Err(stopped),
                Err(_) => Err(refused),
            };
        }
        other => {
            let ProbeWithBody {
                probe,
                response,
                brings,
            } = other?;
            (probe, Some(ranged::First { response, brings }))
        }
    };
    Ok(ControlFlow::Continue(probed))
}

/// A path of its own a probed file takes rather than ranges.
enum Elsewhere {
    /// A part cut from a plain MP4 by its sample tables.
    Mp4Part,
    /// A DASH manifest's picture and sound, fetched and joined.
    Dash,
    /// An HLS stream's segments.
    Stream,
}

/// Whether the probed file goes by a path of its own ([`Elsewhere`]); refused where a part
/// of it was asked for and it is no kind of file a part can be cut from.
fn elsewhere(url: &str, info: &Probe, options: &Options) -> Result<Option<Elsewhere>, Error> {
    // A part of a video is cut from a video page's streams (`clip`), from an
    // HLS or DASH stream's segments, or from a plain MP4 by its sample
    // tables (`plain`); any other file is not fetched whole in its place
    // without a word.
    if options.span.is_some()
        && !is_stream()
        && !crate::videopage::is_page(info.content_type.as_deref())
        && crate::youtube::video_id(url).is_none()
        && stream::kind_of(url, info.content_type.as_deref()).is_none()
    {
        if plain::is_mp4(&info.filename, info.content_type.as_deref()) {
            return Ok(Some(Elsewhere::Mp4Part));
        }
        return Err(Error::Internal(
            "a part can be cut only from a video page, an HLS or DASH stream or an MP4 file;              this address is another kind of file"
                .into(),
        ));
    }
    /* A manifest is not a file. It lists segments that concatenate into one,
     * with no length to split into ranges and no position to resume from, so
     * it takes a path of its own rather than being fetched as the few
     * kilobytes of text it literally is. */
    /* A DASH manifest routinely carries picture and sound as separate
     * representations; dm reads it, fetches both and joins them itself
     * (`dash`, `mp4mux`, `mkvmux`). Decided once, here, from the URL and the
     * content type together — a manifest served as `application/dash+xml`
     * from a URL that does not end in `.mpd` is DASH just the same. */
    let kind = stream::kind_of(url, info.content_type.as_deref());
    match kind {
        Some(stream::Kind::Dash) => Ok(Some(Elsewhere::Dash)),
        Some(stream::Kind::Hls) => Ok(Some(Elsewhere::Stream)),
        None => Ok(None),
    }
}

/// A web page where a file was asked for: read where it is YouTube's, kept as HTML where a
/// person said so, refused otherwise. The download's end where the page was read.
async fn page_answer(
    client: &reqwest::Client,
    url: &str,
    info: &mut Probe,
    options: &Options,
    report: &Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Option<Finished>, Error> {
    /* A web page, not a file — a video site's watch URL, say. YouTube
     * pages are read by dm itself; every other site's video comes from the
     * browser extension's caught player requests, not from reading the
     * page. */
    /* A `.zip` link answered as text/html is either the zip with a careless
     * type, which is fetched as the file it is, or a page standing in for
     * it — a sign-in, an expired link. The first bytes tell the two
     * apart. */
    let page = crate::videopage::is_page(info.content_type.as_deref());
    if page && is_stream() {
        return Err(Error::known(
            crate::FailKind::WebPage,
            "the stream answered with a web page rather than the file",
        ));
    }
    if page && names_a_file(url) {
        if starts_as_html(client, url, options).await {
            if options.accept_page {
                info.filename = html_name(&info.filename);
            } else {
                return Err(Error::known(
                    crate::FailKind::WebPage,
                    "that link answered with a web page rather than the file — it may have \
                     expired, or need a sign-in",
                ));
            }
        }
    } else if page {
        /* A YouTube page is read by dm itself; every other site's video
         * comes from the browser extension's caught player requests. An
         * address that names a document (`.html`, `.xml`) may be the
         * document itself, so a non-YouTube page that looks like one falls
         * through to be fetched as the file it literally is. */
        let document = names_a_document(url);
        let youtube = crate::youtube::video_id(url).is_some();
        if youtube {
            match crate::videopage::download_page(client, url, options, report.clone()).await {
                Err(e)
                    if document
                        && !options.media
                        && !matches!(e, Error::Paused | Error::Cancelled) => {}
                outcome => return outcome.map(Some),
            }
        } else if !document && options.media {
            /* A page the browser's panel handed over for its video (gaps row 155): dm reads
             * YouTube's pages only, and this site's video comes from its player's own files,
             * which the browser had not seen. Not an expired link, and not a page to save. */
            return Err(Error::Internal(crate::PAGE_NOT_READ.into()));
        } else if !document {
            /* Asked about in the window as an address that ends like a file is (contract row
             * B40, gaps row 136): a person who typed a page may want it, and Yes saves it as
             * HTML. The words say the asking's phrase, and where a video comes from. */
            if options.accept_page {
                info.filename = html_name(&info.filename);
            } else {
                return Err(Error::known(
                    crate::FailKind::WebPage,
                    "that address answered with a web page rather than the file; its video, if \
                     any, comes from the browser's Download video button when the page plays",
                ));
            }
        }
    }
    Ok(None)
}

/// A Metalink's mirrors that agree about the file, its word on the file put in `info`'s
/// place, and the digest it gives.
async fn from_metalink(
    client: &reqwest::Client,
    options: &Options,
    info: &mut Probe,
) -> Result<(Vec<Source>, Option<Checksum>), Error> {
    let mut listed = fetch_metalink(client, &info.final_url, options).await?;
    // A public mirror list's mirrors inside are passed over, as a redirect there would be
    // refused (`redirects::named_inside`, security audit).
    listed
        .mirrors
        .retain(|mirror| !crate::redirects::named_inside(&info.final_url, mirror));
    let (mirrors, agreed) = or_stop(&options.control, agreeing_mirrors(client, &listed)).await?;

    let _ = MIRRORS.try_with(|n| n.store(mirrors.len(), Ordering::SeqCst));
    let sources = match ONLY_MIRROR.try_with(|only| *only) {
        // The mirror counted on the first try may have stopped answering,
        // or stopped agreeing, since: that try fails as any other.
        Ok(only) => match mirrors.get(only) {
            Some(mirror) => vec![mirror.clone()],
            None => {
                return Err(Error::Connection(
                    "that mirror no longer answers, or no longer agrees about the file".into(),
                ))
            }
        },
        Err(_) => mirrors,
    };
    *info = agreed;
    if !listed.filename.is_empty() {
        info.filename = listed.filename.clone();
    }
    Ok((sources, listed.checksum))
}

/// What the probe found to fetch in ranges: its word on the file (a Metalink's, where it
/// was one), its answer for range 0, the mirrors and the digest to check.
struct Found {
    info: Probe,
    first_response: Option<ranged::First>,
    sources: Vec<Source>,
    checksum: Option<Checksum>,
    listed_mirrors: bool,
}

/// Fetches what the probe found in ranges, checks it and puts it together; fetched again
/// on one connection where the connections did not get the same file.
async fn fetch_found(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    found: Found,
) -> Result<Finished, Error> {
    let Found {
        info,
        mut first_response,
        sources,
        checksum,
        listed_mirrors,
    } = found;
    fs::create_dir_all(&options.parts_dir).await?;
    fs::create_dir_all(&options.dest_dir).await?;

    // Where the address led on to another server, that one's limit as found in this run of dm.
    let options = &Options {
        connections: options
            .connections
            .min(crate::agents::ceiling_for(&info.final_url).unwrap_or(usize::MAX)),
        ..options.clone()
    };
    let (job, planned) = new_job(url, &info, options, sources, &report).await?;

    // Anything already staged from a previous run counts as done. Read from
    // the files rather than remembered, because the files are the truth.
    adopt_staged(&job).await?;

    // A resume that already has bytes for segment 0 cannot use the probe's
    // response: it streams from byte 0, and the bytes on disk started there.
    // A capped stream (chunk) makes many small requests on purpose — each
    // under the cap — so the probe's full-file body defeats it. Several ranges under a speed
    // limit would have the body read ahead of the pacing, and the shown speed
    // overshoots; one range keeps it.
    if first_response.is_some()
        && (job.ledger(0) > 0 || job.chunk.is_some() || (job.limiter.is_limited() && planned > 1))
    {
        first_response = None;
    }

    let claimed = match info.size {
        Some(size) => Some(room_for(size, &job.downloaded, options)?),
        None => None,
    };
    // Once there is known to be room for it.
    preallocate(&job).await?;

    announce(&report, &info).await;
    let again = report.clone();
    let (outcome, workers) = run_watched(
        client,
        &job,
        &info,
        options,
        report,
        first_response,
        planned,
    )
    .await;
    // Not assembled: whatever comes next removes the parts, makes them again or leaves them.
    if outcome.is_err() || job.disagreed.load(std::sync::atomic::Ordering::SeqCst) {
        release_staged(&job, true).await;
    }
    /* A server that stopped serving ranges to several connections serves
     * one: the file again on a single connection, from the start. The parts
     * go first, or the recorded plan of many ranges would be resumed. */
    if matches!(outcome, Err(Error::RangesIgnored)) && options.connections > 1 {
        clear_parts(&options.parts_dir).await?;
        let one = Options {
            connections: 1,
            ..options.clone()
        };
        // Its own claim is made again by the run on one connection.
        drop(claimed);
        return once_more(client, url, &one, again).await;
    }
    outcome?;

    /* A range's last bytes, asked for again, were not the bytes on disk: the
     * connections did not all get the same file. Fetched again from the
     * start on one connection, where there is no seam, and the row says why. */
    if job.disagreed.load(std::sync::atomic::Ordering::SeqCst)
        && options.connections > 1
        && workers > 1
    {
        clear_parts(&options.parts_dir).await?;
        say_fetched_again(&again).await;
        let one = Options {
            connections: 1,
            ..options.clone()
        };
        drop(claimed);
        return once_more(client, url, &one, again).await;
    }

    let (checksum, from_beside) =
        checksum_beside(client, url, &info, options, checksum, listed_mirrors).await;
    /* A Metalink supplies its own digest where the caller gave none, so the
     * options handed to assembly carry whichever applies — one place decides
     * whether a file is the file. */
    let verified = checksum.is_some();
    let options = &Options {
        checksum,
        ..options.clone()
    };
    crate::finish_trace::mark("reported");
    let assembled = assemble(&job, &info, options).await;
    crate::finish_trace::mark("assembled");
    // Placed, or never to be: the staged file is let go of, in the background once it is placed.
    release_staged(&job, assembled.is_err()).await;
    /* Bytes that differ across a seam when asked for a second time: the
     * connections did not all get the same file, or a server answers ranges
     * with other bytes. The parts are gone (`assemble` removed them); the
     * file is fetched again from the start on one connection, where there
     * is no seam, and the row says why. */
    if matches!(assembled, Err(Error::DishonestRanges)) && options.connections > 1 && workers > 1 {
        say_fetched_again(&again).await;
        let one = Options {
            connections: 1,
            ..options.clone()
        };
        drop(claimed);
        return once_more(client, url, &one, again).await;
    }
    let (path, bytes, category) = assembled?;
    // Said on the row only once the file has passed: a mismatch fails it as a typed one does.
    if from_beside {
        if let Some(to) = &again {
            let _ = to.send(Update::Note(CHECKED_BESIDE.into())).await;
        }
    }
    set_server_date(options, &info, &path);

    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: workers,
        category,
        verified,
        mirrors: job.sources.len(),
    })
}

/// The download's shared state: its ranges as planned (or as an earlier run left them), with
/// room for the splits to come, and how many were planned.
async fn new_job(
    url: &str,
    info: &Probe,
    options: &Options,
    sources: Vec<Source>,
    report: &Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<(Arc<Job>, usize), Error> {
    let segments = planned_segments(info, options).await?;
    let planned = segments.len();
    // Room for the ranges splitting may add, where the length is known and
    // the server serves ranges; none otherwise.
    let capacity = if info.resumable && info.size.is_some() {
        planned.max(MAX_RANGES)
    } else {
        planned
    };
    let live = segments.clone();
    // A plain file a range where the download folder is on another disk (gaps row 166).
    let by_range = {
        let (parts, dest) = (options.parts_dir.clone(), options.dest_dir.clone());
        let by_ranges = info.resumable && info.size.is_some_and(|size| size > 0);
        tokio::task::spawn_blocking(move || stage_by_range(&parts, &dest, by_ranges))
            .await
            .unwrap_or(false)
    };

    let job = Arc::new(Job {
        disagreed: std::sync::atomic::AtomicBool::new(false),
        seams_checked: (0..capacity)
            .map(|_| std::sync::atomic::AtomicBool::new(false))
            .collect(),
        end_checked: std::sync::atomic::AtomicBool::new(false),
        handed: Handed::default(),
        holding: std::sync::Mutex::new(None),
        stopping: Arc::default(),
        segments: Mutex::new(segments),
        limiter: options.limiter.clone(),
        sources,
        attempts: (0..capacity).map(|_| AtomicU64::new(0)).collect(),
        overlaps: (0..capacity).map(|_| std::sync::Mutex::new(None)).collect(),
        live: (0..capacity)
            .map(|i| {
                std::sync::Mutex::new(live.get(i).map_or(Live::default(), |s| Live {
                    start: s.start,
                    end: s.end,
                    reserved: 0,
                    taken: false,
                    trusted: false,
                    reach: 0,
                    going: Going::default(),
                }))
            })
            .collect(),
        size: info.size,
        resumable: info.resumable,
        validator: info.validator.clone(),
        parts: options.parts_dir.clone(),
        by_range,
        chunk: HINTS.try_with(|hints| hints.chunk).ok().flatten(),
        headers: stream_headers(),
        downloaded: Arc::new(AtomicU64::new(0)),
        // Spread over the connections asked for: those of a small file, fewer, still ask no
        // more than once a round each.
        together: retry::Together::new(options.connections.clamp(1, plan::MAX_CONNECTIONS)),
        moved: tokio::sync::Notify::new(),
        first_byte_us: AtomicU64::new(0),
        last_ended: std::sync::Mutex::new(None),
        limits: Limits::default(),
        control: options.control.clone(),
        // The ranges go straight to where the probe landed: past a redirect
        // to another host they carry what the hop carried, the page's origin.
        referrer: crate::redirects::referrer_for(options.referrer.as_deref(), url, &info.final_url),
        tries: options.tries(),
        sign_in: options.sign_in.clone(),
        report: report.clone(),
        room: options.room.clone(),
    });
    Ok((job, planned))
}

/// The file's name and its validator, told before the first byte.
async fn announce(report: &Option<tokio::sync::mpsc::Sender<Update>>, info: &Probe) {
    if let Some(tx) = report {
        // Before the first byte: this is the whole point of sending it.
        let _ = tx
            .send(Update::Named {
                name: info.filename.clone(),
                category: category::classify(&info.filename, info.content_type.as_deref()),
            })
            .await;
        if let Some(validator) = &info.validator {
            let _ = tx.send(Update::Validator(validator.clone())).await;
        }
    }
}

/// Runs the ranges, sampled for the row as they go: how it ended, and on how many
/// connections it began.
async fn run_watched(
    client: &reqwest::Client,
    job: &Arc<Job>,
    info: &Probe,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    first_response: Option<ranged::First>,
    planned: usize,
) -> (Result<(), Error>, usize) {
    let reporter = report.map(|tx| watch(Arc::clone(&job.downloaded), info.size, tx));
    let wanted = options.connections.clamp(1, plan::MAX_CONNECTIONS);
    // No more at once than `OPENED_AT_ONCE`, even on a plan an earlier run recorded with more.
    let workers = wanted.min(planned).min(OPENED_AT_ONCE);
    // Connections beyond the ranges planned, for a file small enough to be one
    // or two: they join late and only if the server is slow (`ranged::run`);
    // and those past `OPENED_AT_ONCE`, only while they raise the rate (`ranged::grow`).
    let helpers = if info.resumable && info.size.is_some() {
        wanted - workers
    } else {
        0
    };
    let outcome = run(
        client,
        job,
        planned,
        workers,
        helpers,
        info.resumable,
        first_response,
    )
    .await;
    crate::finish_trace::mark("run returned");
    // The next file from a server that turned requests away opens no more than it served at once.
    if job.limits.limited() && job.sources.len() == 1 {
        crate::agents::remember_ceiling(&job.sources[0].url, job.together.most());
    }

    if let Some((handle, finished)) = reporter {
        // Ask for a closing sample, then wait for it to be sent.
        let _ = finished.send(());
        let _ = handle.await;
    }
    (outcome, workers)
}

/// Removes every staged part, for the file to be fetched again from the start.
async fn clear_parts(parts_dir: &Path) -> Result<(), Error> {
    let mut listing = fs::read_dir(parts_dir).await?;
    while let Some(entry) = listing.next_entry().await? {
        let _ = fs::remove_file(entry.path()).await;
    }
    Ok(())
}

/// Tells the row the file is fetched again over one connection, and why.
async fn say_fetched_again(again: &Option<tokio::sync::mpsc::Sender<Update>>) {
    if let Some(to) = again {
        let _ = to
            .send(Update::Retrying {
                attempt: 1,
                of: 1,
                why: "the pieces fetched over several connections did not agree when \
                      checked again, so the file is fetched again over one"
                    .into(),
                wait: std::time::Duration::ZERO,
            })
            .await;
    }
}

/// The digest to check where none was typed and no Metalink gave one: the server's own word
/// for it, or a checksum file it publishes beside the file. Whether it came from beside.
async fn checksum_beside(
    client: &reqwest::Client,
    url: &str,
    info: &Probe,
    options: &Options,
    mut checksum: Option<Checksum>,
    listed_mirrors: bool,
) -> (Option<Checksum>, bool) {
    /* Nothing typed and no Metalink's: a checksum file the server publishes beside the file
     * (`dm.zip.sha256`, `SHA256SUMS`) is the digest, checked as a typed one is (gaps row F5).
     * Not for a video caught from a page or a web page saved as one, which no release
     * vouches for. */
    let beside = checksum.is_none()
        && !listed_mirrors
        && !is_stream()
        && !options.media
        && !options.accept_page;
    /* The server's own word for the file's digest, where its answer gave one (`Repr-Digest`,
     * `Digest`, `Content-MD5`), is taken first under the same rules: it costs no request, and no
     * checksum file is then asked for (gaps row W19). A mismatch fails the file as a checksum
     * file's does; the row says nothing more of a match than a typed digest's. */
    let from_header = beside && info.digest.is_some();
    if from_header {
        checksum = info.digest.clone();
    } else if beside {
        // A pause or a cancel meanwhile ends the look with nothing found; the assembly below
        // honours it as it does any other.
        checksum =
            sidecar::published_beside(client, url, &info.final_url, &info.filename, options).await;
    }
    let from_beside = beside && !from_header && checksum.is_some();
    (checksum, from_beside)
}

/// The server's date on the finished file, where asked.
fn set_server_date(options: &Options, info: &Probe, path: &Path) {
    // The server's date on the file, where asked: a file a person sorts by
    // date then sorts by when it was made, not when it was fetched.
    if options.server_dates {
        if let Some(when) = info.modified.as_deref().and_then(http_date) {
            // A file system that keeps no such date leaves the download's
            // own, which is no reason to fail a whole file.
            let _ = std::fs::File::options()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(when));
        }
    }
}

/// [`download_reporting`] again, boxed as a `Send` future: the download asks for itself
/// (`fetch_found` going on from a changed file or a bad checksum), and a recursive async call
/// needs its future's type and its `Send` named, or rustc cannot prove the spawned run `Send`.
pub(crate) fn once_more<'a>(
    client: &'a reqwest::Client,
    url: &'a str,
    options: &'a Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Finished, Error>> + Send + 'a>> {
    Box::pin(download_reporting(client, url, options, report))
}
