// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::sync::atomic::AtomicBool;

use super::*;

mod attempt;
mod helping;
mod limits;
mod staged;

use attempt::attempt_segment;
use helping::*;
pub(super) use limits::Limits;
use limits::*;
pub(super) use staged::*;

/// An answer a range's attempt reads where it asks for none of its own.
pub(super) enum Answer {
    /// The probe's, which runs from the file's first byte.
    Probe(First),
    /// The rest of one that brought the range before this one whole and goes on.
    Carried(Carried),
}

/// The probe's answer, for whichever connection takes range 0.
pub(super) struct First {
    /// The answer, its body not yet read.
    pub(super) response: reqwest::Response,
    /// The last byte it brings, where it was asked for the file's first part
    /// only (`probe::FIRST_ASK`); `None` where it runs to the file's end.
    pub(super) brings: Option<u64>,
}

/// A body as a range's attempt reads it.
type BodyStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>;

/// An answer read to the end of one range and going on into the next (`limits::continue_into`).
pub(super) struct Carried {
    stream: BodyStream,
    /// What came past the range's end in the last piece read, the next range's first bytes.
    rest: Option<bytes::Bytes>,
    /// The last byte the answer brings.
    reach: u64,
    /// Its request, still at the server.
    out: Out,
}

/// How often a running download's byte count is sampled and its progress reported: four
/// times a second, as often as a row's numbers are worth drawing.
pub(crate) const SAMPLE_EVERY: Duration = Duration::from_millis(250);

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
    watch_estimating(downloaded, total, None, tx)
}

/// [`watch`], with an estimate of the total that its fetch updates as it learns (`0`: none
/// yet), used where the server states none: a stream's (gaps row Y22).
pub(crate) fn watch_estimating(
    downloaded: Arc<AtomicU64>,
    total: Option<u64>,
    estimate: Option<Arc<AtomicU64>>,
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
                _ = tokio::time::sleep(SAMPLE_EVERY) => false,
                _ = &mut done_rx => true,
            };

            let now = Instant::now();
            if let Some(estimate) = &estimate {
                let about = estimate.load(Ordering::Relaxed);
                if about > 0 {
                    meter.set_total(Some(about), true);
                }
            }
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
    // The disk is read from a copy of the plan, and the lock taken only to
    // write down what was found: no lock is held across the disk's work
    // (gaps row 184), and no connection runs yet to change the plan.
    let plan = job.segments.lock().await.clone();
    if job.by_range {
        adopt_ranges(job, &plan).await?;
    } else {
        adopt_into_staged(job, &plan).await?;
    }
    let (dir, ranges) = (job.parts.clone(), plan.len());
    let ledgers: Vec<u64> = tokio::task::spawn_blocking(move || {
        (0..ranges)
            .map(|index| read_ledger(&dir, index).unwrap_or(0))
            .collect()
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?;
    let mut segments = job.segments.lock().await;
    for index in 0..segments.len() {
        let segment = &mut segments[index];
        /* A ledger cannot say more than its range under the plan that was
         * recorded beside it; one that does was written by something that
         * did not keep to the plan. Held to the range, so what is adopted
         * is exactly what assembly will use. */
        segment.done = ledgers.get(index).copied().unwrap_or(0).min(segment.span());
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

/// Each range's ledger held to what the range's own file holds: a power cut can leave a ledger
/// on disk while the end of the bytes it speaks for, or the whole file, is not, and reading them
/// back for the resume's check would then fail at the file's end rather than find them missing.
/// A ledger with no file under it vouches for nothing and goes.
async fn adopt_ranges(job: &Job, segments: &[Segment]) -> Result<(), Error> {
    let (dir, ranges) = (job.parts.clone(), segments.len());
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        for index in 0..ranges {
            let Some(said) = read_ledger(&dir, index) else {
                continue;
            };
            let held = std::fs::metadata(range_path(&dir, index)).map_or(0, |m| m.len());
            if held == 0 {
                let _ = std::fs::remove_file(ledger_path(&dir, index));
            } else if said > held {
                std::fs::write(ledger_path(&dir, index), format!("{held:020}"))?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(())
}

/// Part files an earlier dm left go into the staged file first. A ledger with no staged file
/// under it vouches for bytes that are not there.
async fn adopt_into_staged(job: &Job, segments: &[Segment]) -> Result<(), Error> {
    let staged = job.staged();
    let ranges: Vec<(u64, u64)> = segments.iter().map(|s| (s.start, s.span())).collect();
    let (dir, had_staged) = (job.parts.clone(), staged.exists());
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
    Ok(())
}

/// The staged file, made as long as the file will be, so every range's
/// region is there to be written; on most file systems the room is only
/// taken as bytes land. Never cut: a longer staged file holds nothing a
/// ledger vouches for past its ranges. None where each range has a file of
/// its own, made by its first write and grown as it is written front to back.
/// Kept open by the job until the file is placed (`Job::holding`).
pub(super) async fn preallocate(job: &Job) -> Result<(), Error> {
    if job.by_range {
        return Ok(());
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(job.staged())
        .await?;
    #[cfg(windows)]
    super::files::sparse(&file);
    if let Some(size) = job.size {
        if file.metadata().await?.len() < size {
            file.set_len(size).await?;
        }
    }
    *crate::lock(&job.holding) = Some(file.into_std().await);
    Ok(())
}

/// Lets go of the staged file the job held open (`Job::holding`), on a blocking thread, since
/// on macOS that close writes out what is still in memory. Waited for where the parts are removed
/// or made again next: Windows keeps a file being deleted for as long as a handle is open, and
/// a new one of the same name cannot be made meanwhile. Not waited for once the file is placed:
/// the row is Completed while the system puts the bytes down, as it does after any write.
pub(super) async fn release_staged(job: &Job, wait: bool) {
    let Some(file) = crate::lock(&job.holding).take() else {
        return;
    };
    let closing = tokio::task::spawn_blocking(move || drop(file));
    if wait {
        let _ = closing.await;
    }
}

/// Runs every range, and keeps running until none are left unfinished.
///
/// `workers` connections take the pieces, each the next one not yet taken as
/// soon as it is done with its own: the fast connections fetch most of the
/// file, and a slow one holds a single piece. Once none is left untaken, a
/// connection that is free halves the largest range still under way and takes
/// its back half (dynamic segmentation), so no connection sits idle behind a
/// slow one. The first piece is the probe's answer's; the rest are asked for
/// from the file's end backwards ([`in_order`]).
///
/// `helpers` more connections than there are pieces (a small file, on few
/// ranges) start late: each waits [`HELP_AFTER`], and halves a range only if
/// the file is slow enough that more than [`HELP_WORTH_SECS`] is left. A
/// server that serves it fast never sees them, and one that holds each
/// connection down is fetched on all of them within a fraction of a second.
///
/// Once the server has turned a request away while serving others, the
/// connections keep to what it serves (`limits`): no more at work than it has
/// served at once, the others waiting for a place; each request at the
/// server's pace; and the work taken as [`pick_limited`] finds it.
pub(super) async fn run(
    client: &reqwest::Client,
    job: &Arc<Job>,
    planned: usize,
    workers: usize,
    helpers: usize,
    resumable: bool,
    first_body: Option<First>,
) -> Result<(), Error> {
    /* The probe's answer streams the file from byte 0: it is range 0's, for
     * whichever connection takes range 0. Given to the first connection
     * started, it would be used only if that one also took range 0; the
     * connections start together, and when another took it first the
     * answer would be held, unread, for as long as that connection ran: a
     * ninth connection beside eight (QA connections_1_8_32). */
    let probe = Arc::new(std::sync::Mutex::new(first_body));
    let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Ranges a connection gave up on, for the others to take.
    let orphans = Arc::new(std::sync::Mutex::new(Vec::<usize>::new()));
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(workers.max(1)));
    let mut running = tokio::task::JoinSet::new();
    // When the connections with ranges of their own are done, helpers still
    // waiting have nothing to wait for.
    let first_wave = Arc::new(std::sync::atomic::AtomicUsize::new(workers.max(1)));
    let (wave_over, wave_over_seen) = tokio::sync::watch::channel(false);
    let started = tokio::time::Instant::now();
    // A second network path, where one is chosen: every other connection
    // goes out on it (`second_path`). The ranges are taken as connections
    // free up, so the faster path fetches more of them.
    let second = crate::second_path::second_client();
    /* The connections past `OPENED_AT_ONCE` wait to be let in by `grow`, which lets them in
     * while they raise the rate and takes them back when they do not. One taken back finishes
     * its piece and waits again; once `grow` is done, those still waiting are not needed. */
    let grown_from = OPENED_AT_ONCE.max(workers.max(1));
    let grown = (workers.max(1) + helpers).saturating_sub(grown_from);
    let (gate, gate_seen) = tokio::sync::watch::channel(Gate::default());
    let grower = (grown > 0).then(|| {
        tokio::spawn(grow(
            Arc::clone(job),
            grown,
            gate,
            Arc::clone(&active),
            wave_over_seen.clone(),
        ))
    });
    /* The checks whole ranges hand on, asked beside the ranges on the places of the connections
     * done with them, so a range's connection takes its next work at once (gaps row S4). */
    job.handed.begin(workers.max(1) + helpers);
    running.spawn({
        let (client, job) = (client.clone(), Arc::clone(job));
        async move { ask_handed(&client, &job).await }
    });
    for ordinal in 0..workers.max(1) + helpers {
        let helper = ordinal >= workers.max(1);
        // Which of the connections `grow` lets in this one is.
        let grown_as = ordinal.checked_sub(grown_from);
        let mut gate_seen = gate_seen.clone();
        let on_second = second.is_some() && ordinal % 2 == 1;
        let client = match &second {
            Some(second) if on_second => second.clone(),
            _ => client.clone(),
        };
        let probe = Arc::clone(&probe);
        let (job, next) = (Arc::clone(job), Arc::clone(&next));
        let (orphans, active) = (Arc::clone(&orphans), Arc::clone(&active));
        let (first_wave, wave_over, mut wave_over_seen) = (
            Arc::clone(&first_wave),
            wave_over.clone(),
            wave_over_seen.clone(),
        );
        running.spawn(async move {
            // However it ends, the checks are told it is done with ranges.
            let mut leaving = Leaving {
                job: Arc::clone(&job),
                fetched: false,
            };
            /* Counted among the connections at work (`active`): one with a range of its own from
             * the start, a helper from when it takes one, and neither while it waits as one a
             * limiting server turned away past the most it serves at once (`limits::rejoin`). */
            let mut counted = !helper;
            // Whether this connection, left alone, has had its second go.
            let mut alone_again = false;
            if helper && grown_as.is_none() {
                tokio::select! {
                    () = tokio::time::sleep(HELP_AFTER) => {}
                    _ = wave_over_seen.wait_for(|over| *over) => return Ok(()),
                }
                job.control.check()?;
            }
            'connection: loop {
                if let Some(me) = grown_as {
                    let let_in = tokio::select! {
                        gate = gate_seen.wait_for(|g| g.closed || me < g.allowed) => {
                            gate.is_ok_and(|g| me < g.allowed)
                        }
                        _ = wave_over_seen.wait_for(|over| *over) => false,
                    };
                    if !let_in {
                        break 'connection;
                    }
                    job.control.check()?;
                }
                loop {
                    // Turned away past the most the server serves at once: back in when one at
                    // work is done.
                    if !counted && !helper {
                        rejoin(&job, &active).await?;
                        counted = true;
                    }
                    let index = if job.limits.limited() {
                        // A helper is not worth asking a server that limits connections for.
                        if helper {
                            break 'connection;
                        }
                        match limited_work(&job).await? {
                            Some(index) => index,
                            None => break 'connection,
                        }
                    } else {
                        let counter = next.fetch_add(1, Ordering::Relaxed);
                        let given = if counter < planned {
                            Some(in_order(counter, planned))
                        } else {
                            crate::lock(&orphans).pop()
                        };
                        if let Some(index) = given {
                            // An answer may have gone on into it already (`limits`).
                            if !take(&job, index) {
                                continue;
                            }
                            index
                        } else {
                            /* The watching ends early once the connections with
                             * ranges of their own are done: they end only when no
                             * range is left to halve (or a refusal, which ends a
                             * helper here anyway), and ranges only shrink, so
                             * there is nothing for a helper either. The last helper
                             * to finish its range otherwise watched out its whole
                             * window after the file was in, and the download waited
                             * for it: 80 ms of every small slow file (gaps row 165). */
                            // One `grow` let in has been judged worth it already.
                            if helper
                                && (job.limits.limited()
                                    || (grown_as.is_none()
                                        && !tokio::select! {
                                            worth = slow_enough_to_help(&job, started) => worth,
                                            _ = wave_over_seen.wait_for(|over| *over) => false,
                                        })
                                    // Looked at again after the watching: a refusal
                                    // while it went on is as good as one before.
                                    || job.limits.limited())
                            {
                                break 'connection;
                            }
                            /* A range worth halving, or a wait for one: a range can stop or slow
                             * down, and one just asked for is judged once its bytes come. Looked at
                             * again as soon as a range ends or is given up, or after `SPLIT_LOOK`,
                             * and given up only once no range under way is long enough to halve. */
                            let found = loop {
                                // Asked for before looking, so a range that ends meanwhile still
                                // wakes it.
                                let moved = job.moved.notified();
                                tokio::pin!(moved);
                                moved.as_mut().enable();
                                let now = tokio::time::Instant::now();
                                let judge_from = crate::lock(&job.last_ended)
                                    .map_or(now, |ended| ended + SPLIT_SETTLE);
                                match split_largest(&job, now >= judge_from).await {
                                    Halving::Range(index) => break Found::Range(index),
                                    Halving::Nothing => break Found::Done,
                                    Halving::Later => {}
                                }
                                let wake = if now < judge_from {
                                    judge_from
                                } else {
                                    now + SPLIT_LOOK
                                };
                                tokio::select! {
                                    () = &mut moved => {}
                                    () = tokio::time::sleep_until(wake) => {}
                                }
                                job.control.check()?;
                                // The server has turned a request away: looked at as one that
                                // limits connections (above).
                                if job.limits.limited() {
                                    break Found::Again;
                                }
                                let orphan = crate::lock(&orphans).pop();
                                if let Some(index) = orphan.filter(|&index| take(&job, index)) {
                                    break Found::Range(index);
                                }
                                // Taken back by `grow` while it waited: it waits to be let in again.
                                if grown_as.is_some_and(|me| me >= gate_seen.borrow().allowed) {
                                    if counted {
                                        counted = false;
                                        active.fetch_sub(1, Ordering::SeqCst);
                                    }
                                    continue 'connection;
                                }
                            };
                            match found {
                                Found::Range(index) => index,
                                Found::Again => continue,
                                Found::Done => break 'connection,
                            }
                        }
                    };
                    if !counted {
                        counted = true;
                        active.fetch_add(1, Ordering::SeqCst);
                    }
                    crate::second_path::fetched_by(&job.parts, index, on_second);
                    let first = if index == 0 {
                        crate::lock(&probe).take().map(Answer::Probe)
                    } else {
                        None
                    };
                    let turned_away = AtomicBool::new(false);
                    leaving.fetched = true;
                    let (index, fetched) = fetch_chain(
                        &client,
                        &job,
                        index,
                        resumable,
                        &active,
                        first,
                        &turned_away,
                        on_second,
                    )
                    .await;
                    // A connection waiting for a range worth halving looks again.
                    job.moved.notify_waiters();
                    match fetched {
                        Ok(()) => {}
                        /* The server would not take this connection (a 429 or
                         * 503 at once, a refused connect after its retries)
                         * while others are working: this one steps aside and
                         * leaves its range to them, as a server that limits
                         * connections asks, rather than failing a download the
                         * rest are fetching. */
                        Err(e)
                            if crate::retry::transient(&e) && active.load(Ordering::SeqCst) > 1 =>
                        {
                            give_back(&job, &orphans, index);
                            // Only a server's refusal makes it one that limits connections;
                            // a range that keeps failing otherwise is left to the others.
                            if helper || !turned_away.load(Ordering::SeqCst) {
                                break 'connection;
                            }
                            /* Past the most the server has served at once, it waits for a
                             * place among those at work; within it, for the server's pace,
                             * and looks for work again. Ending, it never came back: from the
                             * twelfth request on, a two-at-once server's file was fetched on
                             * one connection (gaps row 197). */
                            if active.load(Ordering::SeqCst) > job.together.most().max(1) {
                                counted = false;
                                active.fetch_sub(1, Ordering::SeqCst);
                                job.moved.notify_waiters();
                            }
                            continue;
                        }
                        /* The others stepped aside between the judgement and
                         * now, and this is the last connection: its range is
                         * nobody else's to fetch, so it is tried once more
                         * here, with the tries a connection alone gets. */
                        Err(e) if crate::retry::transient(&e) && !alone_again => {
                            alone_again = true;
                            // Given back, so it is this range that is tried
                            // again and not a fresh one taken in its place.
                            give_back(&job, &orphans, index);
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                    alone_again = false;
                    // The others take up its share for a moment before any is judged.
                    *crate::lock(&job.last_ended) = Some(tokio::time::Instant::now());
                    // Taken back by `grow`, with its piece done: it waits to be let in again.
                    if grown_as.is_some_and(|me| me >= gate_seen.borrow().allowed) {
                        if counted {
                            counted = false;
                            active.fetch_sub(1, Ordering::SeqCst);
                        }
                        continue 'connection;
                    }
                }
            }
            if counted {
                active.fetch_sub(1, Ordering::SeqCst);
                // One waiting for a place may have one now.
                job.moved.notify_waiters();
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
    crate::finish_trace::mark("connections ended");
    // Nothing is left for it to let in.
    if let Some(grower) = grower {
        grower.abort();
    }
    // A sync a stopped connection began goes on without it: waited for here, so nothing is
    // written into the parts once the download has returned (`files::Stopping`).
    job.stopping.settled().await;
    if let Some(e) = failed {
        return Err(e);
    }
    // A range given up on after the others had finished, or never taken: fetched now, on one
    // connection, its failure this download's.
    let left: Vec<usize> = {
        let count = job.segments.lock().await.len();
        (0..count)
            .filter(|&index| {
                let l = crate::lock(&job.live[index]);
                !l.taken && l.left() > 0
            })
            .collect()
    };
    for index in left {
        if !take(job, index) {
            continue;
        }
        let (_, fetched) = fetch_chain(
            client,
            job,
            index,
            resumable,
            &AtomicUsize::new(1),
            None,
            &AtomicBool::new(false),
            false,
        )
        .await;
        fetched?;
    }
    /* The checks handed on that no place was left for (those of a range fetched just above, or
     * handed on before the server was seen to limit the download), and the seams of a server
     * that limits requests, checked now that no range is being asked for; where it limits, one
     * at a time: a check sent beside a range's request took the request the server would have
     * taken. A refused check proves nothing, and once one is refused the rest are not asked
     * (gaps row 197). */
    let limited = job.limits.limited();
    if ask_left(client, job, limited).await {
        for index in job.limits.take_due() {
            if !check_seams(client, job, index).await {
                break;
            }
        }
    }
    Ok(())
}

/// A connection of `run`, which tells the checks handed on that it is done with ranges however
/// it ends, and whether its place at the server is free for one (`Handed::done`).
struct Leaving {
    job: Arc<Job>,
    /// Whether it has asked the server for a range.
    fetched: bool,
}

impl Drop for Leaving {
    fn drop(&mut self) {
        self.job.handed.done(self.fetched);
    }
}

/// Which planned range the `counter`th one taken is: the first is range 0, the probe's answer's;
/// after it, from the file's end backwards. A server that turns most of them away serves the
/// ones that came first, and the probe's answer, which runs on from the file's start, then goes
/// on through the ranges it turned away (`limits::continue_into`) as far as the first of those it
/// served: the farther that is, the fewer requests the rest of the file takes. In file order, the
/// served one was nearly always range 1, where the probe's answer had to stop.
fn in_order(counter: usize, planned: usize) -> usize {
    if counter == 0 {
        0
    } else {
        planned - counter
    }
}

/// Takes range `index` for this connection, unless another has it (an answer that went on
/// into it, `limits::continue_into`).
fn take(job: &Job, index: usize) -> bool {
    let mut l = crate::lock(&job.live[index]);
    !std::mem::replace(&mut l.taken, true)
}

/// Gives range `index` back for another connection to take.
fn give_back(job: &Job, orphans: &std::sync::Mutex<Vec<usize>>, index: usize) {
    crate::lock(&job.live[index]).taken = false;
    crate::lock(orphans).push(index);
    job.moved.notify_waiters();
}

/// What a connection waiting for a range to halve found.
enum Found {
    Range(usize),
    /// The server turned a request away meanwhile: looked at again as one that limits.
    Again,
    Done,
}

/// The next range for a connection at work on a download whose server limits it, once the
/// server's pace allows a request: what [`pick_limited`] finds, looked at again as ranges end or
/// every [`SPLIT_LOOK`]. None once nothing is left.
async fn limited_work(job: &Job) -> Result<Option<usize>, Error> {
    loop {
        let moved = job.moved.notified();
        tokio::pin!(moved);
        moved.as_mut().enable();
        job.limits.ready(&job.control).await?;
        let now = tokio::time::Instant::now();
        let judge_from = crate::lock(&job.last_ended).map_or(now, |ended| ended + SPLIT_SETTLE);
        match pick_limited(job, now >= judge_from).await {
            Halving::Range(index) => return Ok(Some(index)),
            Halving::Nothing => return Ok(None),
            Halving::Later => {}
        }
        let wake = if now < judge_from {
            judge_from
        } else {
            now + SPLIT_LOOK
        };
        tokio::select! {
            () = &mut moved => {}
            () = tokio::time::sleep_until(wake) => {}
        }
        job.control.check()?;
    }
}

/// Fetches range `index`, and the ranges after it its answer goes on into
/// (`limits::continue_into`): the last range fetched, and how that went.
#[allow(clippy::too_many_arguments)]
async fn fetch_chain(
    client: &reqwest::Client,
    job: &Job,
    mut index: usize,
    resumable: bool,
    active: &AtomicUsize,
    mut answer: Option<Answer>,
    turned_away: &AtomicBool,
    on_second: bool,
) -> (usize, Result<(), Error>) {
    loop {
        let fetched = fetch_segment(
            client,
            job,
            index,
            resumable,
            active,
            answer.take(),
            turned_away,
        )
        .await;
        match fetched {
            Ok(Some((next, carried))) => {
                job.moved.notify_waiters();
                crate::second_path::fetched_by(&job.parts, next, on_second);
                index = next;
                answer = Some(Answer::Carried(carried));
            }
            Ok(None) => return (index, Ok(())),
            Err(e) => return (index, Err(e)),
        }
    }
}

/// What a connection with no range of its own found to do.
pub(super) enum Halving {
    /// The new range made of a range's back half, to fetch.
    Range(usize),
    /// Ranges are still being fetched, none of them yet worth halving: looked at again soon.
    Later,
    /// Nothing will be worth halving again: the ranges under way are all too short.
    Nothing,
}

/// Halves the range its own connection would take longest to finish, under that connection,
/// where that is longer than a new request is worth (`Job::split_worth`), and returns the new
/// range made of its back half. Only says whether anything is left to halve when not `judging`.
///
/// The plan beside the parts is written again before the new range fetches
/// a byte, so a resume puts every part at the offset it was fetched for.
pub(super) async fn split_largest(job: &Job, judging: bool) -> Halving {
    /* Held across the plan's write below, on purpose (gaps row 184): from the
     * moment the range is shortened until the plan is on disk. A range that
     * reaches the end it had been given reads that end under this lock, and
     * a split whose plan could not be written gives the range its end back
     * before letting go: read in between, the range would stop at an end the
     * plan never recorded, and its last bytes would be fetched by no one.
     * This download's connections alone wait, for one small write, at most
     * once per halving. */
    let mut segments = job.segments.lock().await;
    let index = segments.len();
    if index >= job.live.len() || !job.resumable {
        return Halving::Nothing;
    }
    let (now, worth) = (tokio::time::Instant::now(), job.split_worth().as_secs_f64());
    let mut halvable = false;
    let mut slowest: Option<(usize, f64)> = None;
    for i in 0..index {
        let l = crate::lock(&job.live[i]);
        if !l.taken || l.left() < 2 * MIN_SPLIT {
            continue;
        }
        halvable = true;
        if !judging {
            break;
        }
        if let Some(time) = l.time_left(now).filter(|&time| time > worth) {
            if slowest.is_none_or(|(_, most)| time > most) {
                slowest = Some((i, time));
            }
        }
    }
    let Some((victim, _)) = slowest else {
        return if halvable {
            Halving::Later
        } else {
            Halving::Nothing
        };
    };
    let (start, end, old_end) = {
        let mut l = crate::lock(&job.live[victim]);
        let rest = l.left();
        if rest < 2 * MIN_SPLIT {
            return Halving::Later;
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
        return Halving::Nothing;
    }
    segments[victim].end = start - 1;
    segments.push(Segment {
        start,
        end,
        done: 0,
    });
    // A new range starts with nothing written down for it.
    let _ = fs::remove_file(ledger_path(&job.parts, index)).await;
    forget_synced(&job.parts, index).await;
    // Every byte of it will be this run's.
    *crate::lock(&job.live[index]) = Live {
        start,
        end,
        reserved: 0,
        taken: true,
        trusted: true,
        reach: 0,
        going: Going {
            since: Some(tokio::time::Instant::now()),
            ..Going::default()
        },
    };
    Halving::Range(index)
}

/// A range stopped by a pause (a person's, or dm ending): what it wrote is put on the disk itself
/// and written down as such (`mark_synced`), so the next run goes on from it without asking the
/// server for its last 64 KiB again and comparing them (`RESUME_CHECK`), a check that exists for a
/// ledger a power cut may have outlived its bytes by (gaps row 186). Where the sync fails nothing
/// is written down, and the next run checks as before. Paid once per range at a stop, within the
/// grace a stop gives the connections.
async fn stopped_synced(job: &Job, index: usize, file: &fs::File, written: u64) {
    if written == 0 {
        return;
    }
    sync_and_mark(&job.stopping, &job.parts, file, index, written).await;
}

/// Fetches one range into its place in the staged file, retrying what is
/// worth retrying. Where its answer goes on into the range after it
/// (`limits::continue_into`), that range and the rest of the answer, to be read
/// on. `turned_away` is set where the server's refusal made it step aside.
pub(super) async fn fetch_segment(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    resumable: bool,
    active: &AtomicUsize,
    answer: Option<Answer>,
    turned_away: &AtomicBool,
) -> Result<Option<(usize, Carried)>, Error> {
    // A part found whole on a resume is still checked at its end where the
    // server can be asked for it, since a power cut can tear a whole part
    // as easily as a half one.
    if !resumable && job.segments.lock().await[index].finished() {
        return Ok(None);
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
         * dm's tries on each would hold the download for minutes and then
         * fail it, where fewer connections finish it. Stepped aside while
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
        /* Once the server is known to limit this download, only while another range is being
         * fetched: one refused while none is, is asked again at the server's pace and counted
         * against the tries, so a server that refuses everything still fails the download
         * rather than being asked for ever. */
        let others = active.load(Ordering::SeqCst) > 1
            && (!job.limits.limited() || job.together.flowing() > 0);
        let stepping_aside = (refused || unanswered) && others;
        if stepping_aside {
            turned_away.store(true, Ordering::SeqCst);
            job.limits.found();
        }
        stepping_aside
    };
    // Refused by a server that limits how often it takes a request: asked again when its pace
    // allows, not on the backoff's growing waits (`limits::Limits::turned_away`).
    let paced = |e: &Error| {
        let wait = job.limits.wait_left();
        (job.limits.limited()
            && !wait.is_zero()
            && matches!(
                e,
                Error::Refused {
                    status: 429 | 503,
                    ..
                }
            ))
        .then_some(wait.max(PACE_LEAST))
    };
    // Only a capped stream is asked again slice by slice; without a cap the
    // ledger is not read here at all (it is a file, opened and read). Nor is
    // range 0 under the probe's answer where that was asked for the file's
    // first part only, or a range read on into by an answer that may end
    // inside it (that first part, read on past range 0's end where the server
    // limits this download): the rest of the range is asked for on, as a
    // slice's is.
    let first_part = matches!(
        &answer,
        Some(
            Answer::Probe(First {
                brings: Some(_),
                ..
            }) | Answer::Carried(_)
        )
    );
    let sliced = resumable && (job.chunk.is_some() || first_part);
    let mut answer = answer;
    let outcome = loop {
        let before = if sliced { on_disk() } else { 0 };
        let outcome = retry::run_making_progress_unless(
            &job.control,
            Some(&job.together),
            job.tries,
            on_disk,
            retrying,
            busy,
            paced,
            || {
                // Counted before the attempt, so the next one picks a different
                // mirror where there is one to pick.
                job.attempts[index].fetch_add(1, Ordering::Relaxed);
                at_start
                    .0
                    .store(job.downloaded.load(Ordering::Relaxed), Ordering::Relaxed);
                at_start.1.store(on_disk(), Ordering::Relaxed);
                attempt_segment(client, job, index, resumable, answer.take())
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
        Err(Error::Incomplete { .. }) => Ok(None),
        /* Its seams with the ranges either side that are whole, to be asked for again: handed on
         * to be asked beside the ranges (`hand_on`), so this connection takes its next work at
         * once; or, where the server limits this download, asked once no range is being asked
         * for (`run`), as a check sent beside a range's request took the request the server
         * would have taken. */
        Ok(onward) if resumable => {
            crate::finish_trace::mark("a range whole");
            if job.limits.limited() {
                job.limits.check_later(index);
            } else {
                hand_on(job, index).await;
                crate::finish_trace::mark("its seams handed on");
            }
            Ok(onward)
        }
        other => other,
    }
}

/// How many bytes before each range boundary are fetched as an overlap and
/// compared at assembly: reference A overlaps 64 bytes at each seam.
pub(super) const LEAD_BYTES: usize = 64;

/* ------------------------------------------------------------- streams */
