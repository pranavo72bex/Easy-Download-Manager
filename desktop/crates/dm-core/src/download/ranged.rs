use super::*;

mod helping;
mod staged;

use helping::*;
pub(super) use staged::*;

/// Samples the byte count and reports a rate until the download ends.
///
/// A sampler rather than a report per chunk: chunks arrive thousands of times
/// a second and nothing can draw that fast, so the engine counts and this
/// decides how often anyone hears about it.
pub(crate) fn watch(
    downloaded: Arc<AtomicU64>,
    total: Option<u64>,
    tx: tokio::sync::mpsc::Sender<Update>,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(async move {
        let started = downloaded.load(Ordering::Relaxed);
        let mut meter = Meter::new(total, started);
        let mut last = Instant::now();

        // One immediately, so a caller has a row to draw before the first
        // interval has passed rather than an empty space.
        if tx
            .send(Update::Progress(meter.sample(started, Duration::ZERO)))
            .await
            .is_err()
        {
            return;
        }

        loop {
            /* Told when the download ends rather than merely cancelled.
             *
             * A download over a fast link finishes inside one interval, and a
             * watcher that is only aborted reports nothing at all — the bar
             * never moves and no final state ever arrives. Waiting on both
             * means the last sample is always sent. */
            let finished = tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(250)) => false,
                _ = &mut done_rx => true,
            };

            let now = Instant::now();
            let progress = meter.sample(downloaded.load(Ordering::Relaxed), now - last);
            last = now;

            // A receiver that has gone away ends the reporting, not the
            // download.
            if tx.send(Update::Progress(progress)).await.is_err() || finished {
                return;
            }
        }
    });

    (handle, done_tx)
}

/// Picks up where a previous run stopped.
pub(super) async fn adopt_staged(job: &Job) -> Result<(), Error> {
    let mut segments = job.segments.lock().await;
    let staged = job.staged();
    let ranges: Vec<(u64, u64)> = segments.iter().map(|s| (s.start, s.span())).collect();
    let (dir, had_staged) = (job.parts.clone(), staged.exists());
    // Part files an earlier dm left go into the staged file first. A ledger
    // with no staged file under it vouches for bytes that are not there.
    let adopted = tokio::task::spawn_blocking(move || {
        let mut adopted = Vec::new();
        for (index, &(start, span)) in ranges.iter().enumerate() {
            if part_path(&dir, index).exists() {
                adopted.push((index, adopt_part(&dir, index, start, span)));
            } else if !had_staged {
                let _ = std::fs::remove_file(ledger_path(&dir, index));
            }
        }
        adopted
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?;
    for (index, result) in adopted {
        result.map_err(|e| {
            Error::Internal(format!(
                "the part of range {index} could not be adopted: {e}"
            ))
        })?;
    }
    for index in 0..segments.len() {
        let segment = &mut segments[index];
        /* A ledger cannot say more than its range under the plan that was
         * recorded beside it; one that does was written by something that
         * did not keep to the plan. Held to the range, so what is adopted
         * is exactly what assembly will use. */
        segment.done = job.ledger(index).min(segment.span());
        if segment.done > 0 {
            job.downloaded.fetch_add(segment.done, Ordering::Relaxed);
            // Claimed, so a split never takes bytes already on disk.
            if let Some(live) = job.live.get(index) {
                crate::lock(live).reserved = segment.done;
            }
        }
    }
    Ok(())
}

/// The staged file, made as long as the file will be, so every range's
/// region is there to be written; on most file systems the room is only
/// taken as bytes land. Never cut: a longer staged file holds nothing a
/// ledger vouches for past its ranges.
pub(super) async fn preallocate(job: &Job) -> Result<(), Error> {
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(job.staged())
        .await?;
    if let Some(size) = job.size {
        if file.metadata().await?.len() < size {
            file.set_len(size).await?;
        }
    }
    Ok(())
}

/// Runs every range, and keeps running until none are left unfinished.
///
/// `workers` connections take the pieces in file order, each the next one
/// not yet taken as soon as it is done with its own: the fast connections
/// fetch most of the file, and a slow one holds a single piece. Once none is
/// left untaken, a connection that is free halves the largest range still
/// under way and takes its back half (dynamic segmentation),
/// so no connection sits idle behind a slow one.
///
/// `helpers` more connections than there are pieces (a small file, on few
/// ranges) start late: each waits [`HELP_AFTER`], and halves a range only if
/// the file is slow enough that more than [`HELP_WORTH_SECS`] is left. A
/// server that serves it fast never sees them, and one that holds each
/// connection down is fetched on all of them within a fraction of a second.
pub(super) async fn run(
    client: &reqwest::Client,
    job: &Arc<Job>,
    planned: usize,
    workers: usize,
    helpers: usize,
    resumable: bool,
) -> Result<(), Error> {
    let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Ranges a connection gave up on, for the others to take.
    let orphans = Arc::new(std::sync::Mutex::new(Vec::<usize>::new()));
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(workers.max(1)));
    let mut running = tokio::task::JoinSet::new();
    // When the connections with ranges of their own are done, helpers still
    // waiting have nothing to wait for.
    let first_wave = Arc::new(std::sync::atomic::AtomicUsize::new(workers.max(1)));
    let (wave_over, wave_over_seen) = tokio::sync::watch::channel(false);
    // Set once the server has turned a connection away: it limits them, and
    // a helper is not worth asking it for.
    let refused = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let started = tokio::time::Instant::now();
    for ordinal in 0..workers.max(1) + helpers {
        let helper = ordinal >= workers.max(1);
        let (client, job, next) = (client.clone(), Arc::clone(job), Arc::clone(&next));
        let (orphans, active) = (Arc::clone(&orphans), Arc::clone(&active));
        let refused = Arc::clone(&refused);
        let (first_wave, wave_over, mut wave_over_seen) = (
            Arc::clone(&first_wave),
            wave_over.clone(),
            wave_over_seen.clone(),
        );
        running.spawn(async move {
            // A helper that has been let in: it is one of the active, to the
            // connection that steps aside for a server that limits them.
            let mut joined = false;
            // Whether this connection, left alone, has had its second go.
            let mut alone_again = false;
            if helper {
                tokio::select! {
                    () = tokio::time::sleep(HELP_AFTER) => {}
                    _ = wave_over_seen.wait_for(|over| *over) => return Ok(()),
                }
                job.control.check()?;
            }
            loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let orphan = if index < planned {
                    None
                } else {
                    crate::lock(&orphans).pop()
                };
                let index = if index < planned {
                    index
                } else if let Some(index) = orphan {
                    index
                } else {
                    if helper
                        && (refused.load(Ordering::SeqCst)
                            || !slow_enough_to_help(&job, started).await
                            // Looked at again after the watching: a refusal
                            // while it went on is as good as one before.
                            || refused.load(Ordering::SeqCst))
                    {
                        break;
                    }
                    match split_largest(&job).await {
                        Some(index) => index,
                        None => break,
                    }
                };
                if helper && !joined {
                    joined = true;
                    active.fetch_add(1, Ordering::SeqCst);
                }
                crate::lock(&job.live[index]).taken = true;
                match fetch_segment(&client, &job, index, resumable, &active).await {
                    Ok(()) => {}
                    /* The server would not take this connection (a 429 or
                     * 503 at once, a refused connect after its retries)
                     * while others are working: this one steps aside and
                     * leaves its range to them, as a server that limits
                     * connections asks, rather than failing a download the
                     * rest are fetching. */
                    Err(e) if crate::retry::transient(&e) && active.load(Ordering::SeqCst) > 1 => {
                        refused.store(true, Ordering::SeqCst);
                        crate::lock(&job.live[index]).taken = false;
                        crate::lock(&orphans).push(index);
                        break;
                    }
                    /* The others stepped aside between the judgement and
                     * now, and this is the last connection: its range is
                     * nobody else's to fetch, so it is tried once more
                     * here, with the tries a connection alone gets. */
                    Err(e) if crate::retry::transient(&e) && !alone_again => {
                        alone_again = true;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
                alone_again = false;
            }
            if !helper || joined {
                active.fetch_sub(1, Ordering::SeqCst);
            }
            if !helper && first_wave.fetch_sub(1, Ordering::SeqCst) == 1 {
                let _ = wave_over.send(true);
            }
            Ok::<(), Error>(())
        });
    }

    /* The first failure is the download's. The other connections are then
     * stopped and waited out before it is returned: dropping their handles
     * leaves them running, still writing parts and the plan beside them —
     * under a download that has already failed, or under the fresh attempt
     * a changed file starts, which would find the old version's parts
     * adopted again. A pause or a cancel stops every connection by itself,
     * each flushing what it has, so those get a moment to; one stalled
     * where the stop does not reach it is stopped after that. */
    let mut failed = None;
    // Once a pause or a cancel is in, how long the rest get to flush.
    let mut grace: Option<tokio::time::Instant> = None;
    // As each connection ends, in whatever order: a failure is seen when it
    // happens, not after the connections spawned before it are done.
    loop {
        let ended = match grace {
            Some(until) => match tokio::time::timeout_at(until, running.join_next()).await {
                Ok(ended) => ended,
                // One stalled where no stop reaches it: stopped here.
                Err(_) => {
                    running.abort_all();
                    grace = None;
                    continue;
                }
            },
            None => running.join_next().await,
        };
        let Some(ended) = ended else { break };
        // A worker that panicked is a bug, not a network condition, and is
        // worth surfacing as itself rather than as a download failure. One
        // stopped here on purpose is not news.
        let outcome = match ended {
            Ok(outcome) => outcome,
            Err(e) if e.is_cancelled() => continue,
            Err(e) => Err(Error::Internal(e.to_string())),
        };
        if let Err(e) = outcome {
            if failed.is_none() {
                if matches!(e, Error::Paused | Error::Cancelled) {
                    grace = Some(tokio::time::Instant::now() + STOP_GRACE);
                } else {
                    running.abort_all();
                }
                failed = Some(e);
            }
        }
    }
    if let Some(e) = failed {
        return Err(e);
    }
    // A range given up on after the others had finished: fetched now, on
    // one connection, its failure this download's.
    let left: Vec<usize> = std::mem::take(&mut *crate::lock(&orphans));
    for index in left {
        crate::lock(&job.live[index]).taken = true;
        fetch_segment(client, job, index, resumable, &AtomicUsize::new(1)).await?;
    }
    Ok(())
}

/// Halves the range with the most left to fetch, under the connection
/// fetching it, and returns the new range made of its back half; `None` when
/// nothing is worth halving.
///
/// The plan beside the parts is written again before the new range fetches
/// a byte, so a resume puts every part at the offset it was fetched for.
pub(super) async fn split_largest(job: &Job) -> Option<usize> {
    let mut segments = job.segments.lock().await;
    let index = segments.len();
    if index >= job.live.len() || !job.resumable {
        return None;
    }
    let left = |l: &Live| {
        (l.end - l.start)
            .saturating_add(1)
            .saturating_sub(l.reserved)
    };
    let victim = (0..index)
        .filter(|&i| {
            let l = crate::lock(&job.live[i]);
            l.taken && left(&l) >= 2 * MIN_SPLIT
        })
        .max_by_key(|&i| left(&crate::lock(&job.live[i])))?;
    let (start, end, old_end) = {
        let mut l = crate::lock(&job.live[victim]);
        let rest = left(&l);
        if rest < 2 * MIN_SPLIT {
            return None;
        }
        let old_end = l.end;
        l.end = l.start + l.reserved + rest / 2 - 1;
        (l.end + 1, old_end, old_end)
    };
    let mut planned = segments.clone();
    planned[victim].end = start - 1;
    planned.push(Segment {
        start,
        end,
        done: 0,
    });
    let (dir, size, resumable, validator, written) = (
        job.parts.clone(),
        job.size,
        job.resumable,
        job.validator.clone(),
        planned.clone(),
    );
    let recorded = tokio::task::spawn_blocking(move || {
        plan::record(&dir, size, resumable, validator.as_deref(), &written)
    })
    .await;
    if !matches!(recorded, Ok(Ok(()))) {
        // Unrecorded, the split is undone: the range keeps its whole length,
        // which nothing past the new end can have been claimed against.
        crate::lock(&job.live[victim]).end = old_end;
        return None;
    }
    segments[victim].end = start - 1;
    segments.push(Segment {
        start,
        end,
        done: 0,
    });
    // A new range starts with nothing written down for it.
    let _ = fs::remove_file(ledger_path(&job.parts, index)).await;
    *crate::lock(&job.live[index]) = Live {
        start,
        end,
        reserved: 0,
        taken: true,
    };
    Some(index)
}

/// Fetches one range into its place in the staged file, retrying what is
/// worth retrying.
pub(super) async fn fetch_segment(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    resumable: bool,
    active: &AtomicUsize,
) -> Result<(), Error> {
    // A part found whole on a resume is still checked at its end where the
    // server can be asked for it, since a power cut can tear a whole part
    // as easily as a half one.
    if !resumable && job.segments.lock().await[index].finished() {
        return Ok(());
    }

    /* Each attempt reads the range's ledger rather than anything
     * remembered, so a stream that died halfway resumes from where it
     * actually stopped. Trusting an in-memory figure would seek past bytes
     * never written and leave a hole in the middle of the range. */
    // Progress counts only where an attempt continues the last: from a
    // server without ranges every attempt starts again from nothing, and
    // one that got further than the last is not progress toward the end.
    let on_disk = || if resumable { job.ledger(index) } else { 0 };
    // Said while it waits, so the row tells a retry from a stall.
    let retrying = |attempt, of, error: &Error, wait| {
        if let Some(tx) = &job.report {
            let why = error.to_string();
            let _ = tx.try_send(Update::Retrying {
                attempt,
                of,
                why,
                wait,
            });
        }
    };
    /* A server that takes only so many connections at once answers the
     * rest 429 or 503 at once, while the others go on being served (seen
     * with Hetzner's mirror: two at a time). Waited out with backoff, ten
     * times over, that held most of the download for minutes at a seventh
     * of the speed one connection gets; stepping aside at once leaves the
     * range to those the server takes. */
    // Where the whole download and this range stood when the attempt
    // began, for telling a server that holds this connection from a link
    // that has gone quiet for everyone.
    let at_start = (AtomicU64::new(0), AtomicU64::new(0));
    let busy = |e: &Error| {
        let refused = matches!(
            e,
            Error::Refused {
                status: 429 | 503,
                ..
            }
        );
        /* A request the server took and never answered: a server that holds
         * connections past its limit open rather than refusing them (seen
         * with proof.ovh.net at 16 connections, 2026-10-30). Waited out,
         * dm's tries on each held the download for minutes and then failed
         * it, where fewer connections finished it. Stepped aside while
         * others are served, the range goes to them. Only where this
         * request got no byte while the others did: a link gone quiet for
         * everyone (the computer asleep, a router restarting) is retried
         * on every connection as before, and a response that stalled after
         * bytes came is this range's to try again. */
        let (whole_before, own_before) = (
            at_start.0.load(Ordering::Relaxed),
            at_start.1.load(Ordering::Relaxed),
        );
        let unanswered = matches!(e, Error::Network(e) if e.is_timeout())
            && on_disk() == own_before
            && job.downloaded.load(Ordering::Relaxed) > whole_before;
        (refused || unanswered) && active.load(Ordering::SeqCst) > 1
    };
    // Only a capped stream is asked again slice by slice; without a cap the
    // ledger is not read here at all (it is a file, opened and read).
    let sliced = job.chunk.is_some() && resumable;
    let outcome = loop {
        let before = if sliced { on_disk() } else { 0 };
        let outcome = retry::run_making_progress_unless(
            &job.control,
            job.tries,
            on_disk,
            retrying,
            busy,
            || {
                // Counted before the attempt, so the next one picks a different
                // mirror where there is one to pick.
                job.attempts[index].fetch_add(1, Ordering::Relaxed);
                at_start
                    .0
                    .store(job.downloaded.load(Ordering::Relaxed), Ordering::Relaxed);
                at_start.1.store(on_disk(), Ordering::Relaxed);
                attempt_segment(client, job, index, resumable)
            },
        )
        .await;
        /* A capped request that came whole leaves the rest of its range to
         * the next one, on this connection. Only while bytes arrive, so a
         * server that answers nothing new cannot hold this loop; and the
         * slice was no failed try, so it is not counted as one. */
        if sliced && outcome.is_ok() {
            let now = on_disk();
            let more = {
                let l = crate::lock(&job.live[index]);
                now < (l.end - l.start).saturating_add(1)
            };
            if more && now > before {
                job.attempts[index].fetch_sub(1, Ordering::Relaxed);
                continue;
            }
        }
        break outcome;
    };

    match outcome {
        // Asked to stop, so stop — including not treating it as a short range
        // to be waited out.
        Err(Error::Paused) => Err(Error::Paused),
        Err(Error::Cancelled) => Err(Error::Cancelled),
        /* A range that keeps arriving short is not this function's to judge.
         * Assembly compares the whole against the length the server stated,
         * so one place decides whether a file is complete rather than two
         * disagreeing about it. */
        Err(Error::Incomplete { .. }) => Ok(()),
        other => other,
    }
}

/// One attempt at one range.
pub(super) async fn attempt_segment(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    resumable: bool,
) -> Result<(), Error> {
    // Read at each attempt: a split may have shortened the range since.
    let (start, end) = {
        let l = crate::lock(&job.live[index]);
        (l.start, l.end)
    };
    let span = (end - start).saturating_add(1);
    // The ledger, read fresh: what it says is on disk is what has arrived.
    let already = job.ledger(index).min(span);
    let mut ledger = job.open_ledger(index).await?;
    {
        let mut l = crate::lock(&job.live[index]);
        l.reserved = l.reserved.max(already);
    }
    // A whole part needs nothing more, unless its tail is to be checked
    // against the server below.
    if already >= span && !resumable {
        job.segments.lock().await[index].done = span;
        return Ok(());
    }

    /* A server that does not serve ranges is asked for the whole file, so
     * the whole file is what arrives — from byte 0. Seeking past what an
     * earlier attempt left wrote the body at that offset: the part only grew
     * and every retry after the first drop was corrupt. Start again. */
    let already = if !resumable && already > 0 {
        put(&mut ledger, 0).await?;
        job.downloaded.fetch_sub(already, Ordering::Relaxed);
        0
    } else {
        already
    };

    // A range after the first, starting fresh, asks for one byte more at
    // its front: the last byte of the range before, checked at assembly.
    let lead = resumable && index > 0 && already == 0 && start > 0;
    /* What a part already holds is asked for again at its end, and compared.
     * Its ledger says how much is there, and after a power cut the ledger can
     * be on disk while the last bytes are not — zeros, on NTFS, exFAT and
     * others — which a resume would have kept and built on, and called the
     * file finished. Where the two differ the part is cut back to the last
     * byte they agree on and fetched from there: repaired, not restarted. */
    let check = if resumable {
        already.min(RESUME_CHECK)
    } else {
        0
    };
    let from = if lead {
        start - 1
    } else {
        start + already - check
    };

    /* Where this request stops. Without a cap, at the range's end, as ever.
     * With one (`Job::chunk`, from the reading of a video page), a slice of
     * it: the connection asks for the next when this one ends, and the
     * range, the ledger and what a split sees are those of the whole. */
    let asked_end = match job.chunk.filter(|_| resumable) {
        Some(cap) => end.min(from.saturating_add(cap.max(MIN_CHUNK)) - 1),
        None => end,
    };
    let source = job.source(index);
    let mut request = get(
        client,
        &source.url,
        job.referrer.as_deref(),
        job.sign_in.as_ref(),
    );
    request = job.headers.onto(
        request,
        crate::cookies::header_with(job.sign_in.as_ref(), &source.url).is_some(),
        job.referrer.is_some(),
    );
    if resumable {
        request = request.header(reqwest::header::RANGE, format!("bytes={from}-{asked_end}"));
        // Only ever alongside a range: it is a condition on the range, and a
        // republished file answers 200 instead, which is how a changed file
        // is noticed rather than spliced into the old one. The validator is
        // this mirror's own — another server's would simply not match.
        // Never a weak ETag (`W/"…"`): `If-Range` takes only a strong one
        // (RFC 9110), and a strict server answers a weak one with the whole
        // file, every range, so the download fell back to one connection
        // and a resume failed as though ranges had stopped. Without the
        // condition, a changed file is still caught: by the validator the
        // answer carries, and by the bytes the resume compares.
        if let Some(validator) = source.validator.as_ref().filter(|v| !v.starts_with("W/")) {
            request = request.header(reqwest::header::IF_RANGE, validator);
        }
    }

    let res = Error::refused_or(
        or_stop(
            &job.control,
            crate::auth::send(client, request, &source.url),
        )
        .await?,
    )
    .await?;
    let status = res.status();
    let stated = res.content_length();
    if resumable && status != reqwest::StatusCode::PARTIAL_CONTENT {
        // The server answered the whole file to a request for part of it.
        // Under the validator the ranges were planned under, it is the same
        // file served whole; under another, everything staged belongs to a
        // version that is gone.
        let header = |name| {
            res.headers()
                .get(name)
                .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
        };
        let answered =
            header(reqwest::header::ETAG).or_else(|| header(reqwest::header::LAST_MODIFIED));
        let same = source.validator.is_some() && answered == source.validator.as_deref();
        // Asked for every byte, it is the range: Wikimedia answers a range
        // of the whole file that way, and that is the file.
        let all_of_it = from == 0 && stated == Some(end + 1);
        if !(same && all_of_it) {
            if !same {
                forget_plan(job).await;
            }
            return Err(if same {
                Error::RangesIgnored
            } else {
                Error::Changed
            });
        }
    }
    /* A 206 for another range than the one asked — a server rounding to its
     * own blocks — said so in its Content-Range, and its bytes were written
     * at the asked offset regardless. Refused, from the zoo. */
    if resumable {
        let said_start = res
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().strip_prefix("bytes "))
            .and_then(|v| v.split(['-', '/']).next())
            .and_then(|v| v.trim().parse::<u64>().ok());
        if said_start.is_some_and(|s| s != from) {
            return Err(Error::DishonestRanges);
        }
    }

    /* Created if absent, never truncated.
     *
     * clippy suggests `.truncate(true)` here and it would be exactly wrong:
     * the staged file holds every range, so emptying it on any attempt
     * would throw away the bytes of all of them. Stated explicitly so the
     * next person to read the lint does not take its advice. */
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(job.staged())
        .await?;

    /* Buffered, because chunks arrive far smaller than a useful write.
     *
     * Safe because the ledger is written down only after the buffer is
     * flushed: a retry starts from what the ledger says reached the disk, so
     * a buffer lost to a crash costs the bytes it held and nothing else. */
    // Undone here where a server compressed what it was not asked to: with a
    // count of what came over the wire, so that a body which unpacks to far
    // more is stopped (`decode.rs`).
    let stopped: crate::decode::Stopped = Default::default();
    let mut stream: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>,
    > = match crate::decode::coding_of(res.headers()) {
        Some(coding) => {
            crate::decode::decoded(coding, Box::pin(res.bytes_stream()), stopped.clone())
        }
        None => Box::pin(res.bytes_stream()),
    };
    // The part of a chunk past the bytes compared, still to be written.
    let mut carry = None;
    let already = if check > 0 {
        let mut tail = vec![0u8; check as usize];
        let mut on_disk = tokio::fs::File::open(job.staged()).await?;
        on_disk
            .seek(std::io::SeekFrom::Start(start + already - check))
            .await?;
        tokio::io::AsyncReadExt::read_exact(&mut on_disk, &mut tail).await?;
        drop(on_disk);
        let (agree, rest) = compare_tail(&mut stream, &tail, &job.control).await?;
        /* What a power cut leaves is blocks never written: zeros, a whole
         * disk block at a time, and not always the last ones — a disk puts
         * blocks down in its own order. So the block the first difference
         * falls in decides. All zeros, it never reached the disk, and the
         * part is cut back to there and fetched again. Written and still
         * different, it is another version of the file — republished by a
         * server that names no version, so nothing else noticed — and
         * cutting back would splice the two into one file and call it
         * finished. Everything staged belongs to the old one. */
        let differs = (agree as usize) < tail.len() && rest.is_some();
        if differs {
            let base = already - check;
            let at = base + agree;
            // From the first difference to the end of its block: the bytes
            // before it agree, so they say nothing either way.
            let block_end = (at - at % DISK_BLOCK + DISK_BLOCK).min(already);
            let written = tail
                .get(agree as usize..(block_end - base) as usize)
                .unwrap_or(&[]);
            if written.iter().any(|&b| b != 0) {
                drop(file);
                forget_plan(job).await;
                return Err(Error::Changed);
            }
            /* Zeros from the front of the window to the difference: what
             * was lost may reach further back than the window does. A power
             * cut that lost a quarter of a megabyte had its zeros before the
             * window trusted, and the file came out the right length, wrong,
             * and Completed. The part is cut back to where the zeros begin,
             * to a block, and the next attempt checks the window there.
             *
             * Only where the disk's zeros run unbroken from the window's
             * front through the byte that differed — a zero observed where
             * the server has something else, not merely an empty stretch
             * before it. And no further than `ZERO_RUN_STEP` back: a file
             * can be zeros for gigabytes (a disk image, a sparse archive),
             * and following them all to their start fetched every one of
             * them again for a torn last block. A loss longer than a step
             * is still found, a step at a time: the window at the cut opens
             * on zeros the server does not have, and is cut again. */
            let zeros_through_difference = tail
                .get(..=agree as usize)
                .is_some_and(|run| run.iter().all(|&b| b == 0));
            if base > 0 && zeros_through_difference {
                let floor = base.saturating_sub(ZERO_RUN_STEP);
                // In the staged file, at this range's offset.
                let zeros_from =
                    zeros_ending_at(&job.staged(), start + base, start + floor).await? - start;
                let cut = zeros_from - zeros_from % DISK_BLOCK;
                if cut < base {
                    put(&mut ledger, cut).await?;
                    drop(file);
                    // Its connection goes before the next is opened: a site
                    // held to one connection would refuse the repair's.
                    drop(stream);
                    job.downloaded.fetch_sub(already - cut, Ordering::Relaxed);
                    crate::lock(&job.live[index]).reserved = cut;
                    /* Straight on to the window at the cut, rather than
                     * as a failed attempt: a repair stepping back through a
                     * long loss is getting somewhere, and spending the
                     * retry budget on it failed the range part-way. Each
                     * step leaves the part shorter, so it ends. */
                    return Box::pin(attempt_segment(client, job, index, resumable)).await;
                }
            }
        }
        carry = rest;
        let keep = already - check + agree;
        if keep < already {
            put(&mut ledger, keep).await?;
            job.downloaded.fetch_sub(already - keep, Ordering::Relaxed);
            crate::lock(&job.live[index]).reserved = keep;
        }
        keep
    } else {
        already
    };

    /* Small: tokio's file keeps a copy of every write it hands to the
     * disk, so each connection holds this twice. 256 KiB cost 20–30 MB more
     * over ten downloads of eight connections than 16 KiB, and fetched 2 GB
     * over loopback no faster nor with less CPU (qa/after.md). */
    let mut file = tokio::io::BufWriter::with_capacity(16 * 1024, file);
    // At this range's own place in the staged file.
    file.seek(std::io::SeekFrom::Start(start + already)).await?;

    let mut written = already;
    // Where the free space was last looked at (`DISK_LOOK_EVERY`).
    let mut disk_looked = already;
    // What the ledger says and when it said it, so it is written down
    // again every few megabytes or every second rather than every chunk.
    let mut recorded = already;
    let mut recorded_at = std::time::Instant::now();
    // Where this range ends now: sooner than asked, once a split has taken
    // its back half.
    let mut span_now = span;
    let mut want_lead = lead;
    let mut reader = Reader::new();
    let mut window = Window::open(written, job);

    loop {
        // A connection far slower than the rest is dropped and asked again,
        // rather than held to the read timeout while the others finish.
        if window.over() {
            // Only for the first tries of a range: one the server keeps
            // serving slowly is then waited out, not retried to a failure.
            if job.attempts[index].load(Ordering::Relaxed) <= 2 && window.stalled(written, job) {
                file.flush().await?;
                put(&mut ledger, written).await?;
                return Err(Error::Incomplete {
                    got: written,
                    expected: span_now,
                });
            }
            window = Window::open(written, job);
        }
        let chunk = match carry.take() {
            Some(rest) => Ok(rest),
            None => {
                match tokio::time::timeout_at(window.ends(), reader.next(&mut stream, &job.control))
                    .await
                {
                    // The window ran out with nothing arriving: judged at the top.
                    Err(_) => continue,
                    Ok(Ok(Some(chunk))) => chunk,
                    Ok(Ok(None)) => break,
                    // Stopped while waiting: what is in the write buffer is on
                    // disk before the range lets go, or a resume fetches it again.
                    Ok(Err(stopped)) => {
                        file.flush().await?;
                        put(&mut ledger, written).await?;
                        if matches!(stopped, Error::Paused) {
                            job.segments.lock().await[index].done = written.min(span_now);
                        }
                        return Err(stopped);
                    }
                }
            }
        };
        // A stream that dies mid-range leaves what it wrote; flushing before
        // giving up is what lets the retry pick up from there instead of
        // fetching those bytes again.
        let mut chunk = match chunk {
            Ok(c) => c,
            // Every byte the range asked for is here, and the server hung
            // up short of the length it stated: the range is whole. Taken as
            // a failure, it was asked for again and again, each time whole,
            // the row at 100 % and running for ever (BUGLOG.md, BUG-001).
            Err(_) if written >= span_now => break,
            Err(e) => {
                file.flush().await?;
                put(&mut ledger, written).await?;
                return Err(Error::Network(e));
            }
        };
        // The overlap byte is kept aside, not written into this range.
        if want_lead && !chunk.is_empty() {
            if let (Some(first), Some(slot)) = (chunk.first(), job.overlaps.get(index)) {
                slot.store(u16::from(*first), Ordering::Relaxed);
            }
            chunk = chunk.slice(1..);
            want_lead = false;
        }

        /* Checked between chunks rather than mid-write, so what is flushed
         * and what the ledger records stay obviously in step. */
        if let Err(stopped) = job.control.check() {
            // Flushed first either way: the bytes already written are what a
            // resume picks up from, and a pause also records where it got to.
            file.flush().await?;
            put(&mut ledger, written).await?;
            if matches!(stopped, Error::Paused) {
                job.segments.lock().await[index].done = written.min(span_now);
            }
            return Err(stopped);
        }

        if written + chunk.len() as u64 > span {
            /* More bytes than the range asked for.
             *
             * A server holding a compressed representation ranges over the
             * *encoded* bytes and hands back the decoded ones, and nothing in
             * the response says the two do not correspond. Trimming to length
             * here is what makes that silent: every part ends up exactly as
             * long as planned, assembled in order, and wrong all the way
             * through. A file of the right size and the wrong contents is the
             * one thing a download manager must never produce. */
            file.flush().await?;
            put(&mut ledger, written).await?;
            return Err(Error::DishonestRanges);
        }

        // Claimed before it is written, against where the range ends now:
        // a split under this connection stops it at the new end.
        let fits = {
            let mut l = crate::lock(&job.live[index]);
            span_now = (l.end - l.start).saturating_add(1);
            let fits = (chunk.len() as u64).min(span_now.saturating_sub(written));
            l.reserved = written + fits;
            fits
        };
        let cut = fits < chunk.len() as u64;
        if cut {
            chunk = chunk.slice(..fits as usize);
        }
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        // With no length to have planned the room for, the disk is looked at as
        // the bytes come, and the download stopped before it fills it.
        if job.size.is_none() && written - disk_looked >= DISK_LOOK_EVERY {
            disk_looked = written;
            if job
                .room
                .free_space(&job.parts)
                .is_some_and(|free| free < DISK_FLOOR)
            {
                file.flush().await?;
                put(&mut ledger, written).await?;
                return Err(Error::Internal(format!(
                    "stopped: the disk has less than {} free and this download has no length to say when it ends",
                    amount(DISK_FLOOR)
                )));
            }
        }
        // Written down every few megabytes or every second: a crash then
        // costs no more than that of this range, fetched again.
        if written > recorded
            && (written - recorded >= LEDGER_EVERY || recorded_at.elapsed() >= LEDGER_EVERY_SECOND)
        {
            file.flush().await?;
            put(&mut ledger, written).await?;
            recorded = written;
            recorded_at = std::time::Instant::now();
        }
        if cut {
            job.downloaded
                .fetch_add(chunk.len() as u64, Ordering::Relaxed);
            // The rest belongs to the range split off; this one is whole.
            break;
        }

        /* Applied after the bytes have arrived, because there is nowhere
         * earlier to stand: the only lever on an HTTP body is how fast it is
         * read, and a reader that pauses is a sender that stops.
         * Counted as the ceiling lets it through rather than as it arrived:
         * counted on arrival, every connection's chunk was in the count a
         * whole wait early, and the shown speed and ETA ran at twice the
         * limit for the first seconds. */
        let mut through = 0;
        let stopping = or_stop(&job.control, async {
            job.limiter
                .take_counting(chunk.len() as u64, &job.downloaded, &mut through)
                .await;
            Ok(())
        })
        .await;
        if let Err(stopped) = stopping {
            // It is on disk: the count is a ledger of what is, and a retry
            // of this range starts from what its ledger records.
            job.downloaded
                .fetch_add(chunk.len() as u64 - through, Ordering::Relaxed);
            file.flush().await?;
            put(&mut ledger, written).await?;
            if matches!(stopped, Error::Paused) {
                job.segments.lock().await[index].done = written.min(span_now);
            }
            return Err(stopped);
        }
    }

    file.flush().await?;
    put(&mut ledger, written).await?;
    // A body that stopped being undone is the download's failure, not a
    // short file that ended.
    let why = stopped.lock().ok().and_then(|slot| slot.clone());
    if let Some(why) = why {
        return Err(Error::Internal(why));
    }

    /* The shared plan is touched once, at the end, rather than per chunk.
     *
     * One mutex guards every range, so a lock per chunk is every connection
     * queueing behind every other a hundred thousand times over a large
     * download — to publish a number nothing reads while it runs. */
    {
        // Read under the segment lock, which a split holds from shortening
        // this range to recording the plan: a split whose plan could not be
        // written gives the range its end back before letting go, and the
        // end read here is the one that stands.
        let mut segments = job.segments.lock().await;
        let l = crate::lock(&job.live[index]);
        span_now = (l.end - l.start).saturating_add(1);
        drop(l);
        if let Some(segment) = segments.get_mut(index) {
            segment.done = written.min(span_now);
        }
    }

    /* The whole file, asked for without a range, ended cleanly: that is its
     * end, whatever the plan guessed. An unknown length is planned to
     * `u64::MAX` and an empty file as one byte, and both sat through every
     * retry as "short" before this. A length the response stated is still
     * held to, and assembly holds the total to the probed one. */
    if !resumable && stated.is_none_or(|stated| stated == written) {
        return Ok(());
    }

    // A slice that came whole is not a short range: the next is asked for.
    if asked_end < end && written < span_now && start + written > asked_end {
        return Ok(());
    }

    // A stream that ended early without erroring is not a finished range.
    if written < span_now {
        return Err(Error::Incomplete {
            got: written,
            expected: span_now,
        });
    }
    Ok(())
}

/* ------------------------------------------------------------- streams */
