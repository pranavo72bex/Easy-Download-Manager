// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::sync::atomic::AtomicBool;

use super::*;

mod helping;
mod limits;
mod staged;

use helping::*;
pub(super) use limits::Limits;
use limits::*;
pub(super) use staged::*;

/// An answer a range's attempt reads where it asks for none of its own.
pub(super) enum Answer {
    /// The probe's, which runs from the file's first byte.
    Probe(reqwest::Response),
    /// The rest of one that brought the range before this one whole and goes on.
    Carried(Carried),
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
    first_body: Option<reqwest::Response>,
) -> Result<(), Error> {
    /* The probe's answer streams the file from byte 0: it is range 0's, for
     * whichever connection takes range 0. It was given to the first
     * connection started, which used it only if it also took range 0; the
     * connections start together, and when another took it first the
     * answer was held, unread, for as long as that connection ran: a ninth
     * connection beside eight (QA connections_1_8_32, 2026-11-06). */
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
    /* The seams of a server that limits requests, checked now that no range is being asked for,
     * one at a time: a check sent beside a range's request took the request the server would
     * have taken. A refused check proves nothing, and once one is refused the rest are not
     * asked (gaps row 197). */
    for index in job.limits.take_due() {
        if !check_seams(client, job, index, true).await {
            break;
        }
    }
    Ok(())
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
    // ledger is not read here at all (it is a file, opened and read).
    let sliced = job.chunk.is_some() && resumable;
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
        /* Its seams with the ranges either side that are whole, asked for again (`check_seams`):
         * at once, or, where the server limits this download, once no range is being asked for
         * (`run`), as a check sent beside a range's request took the request the server would
         * have taken. */
        Ok(onward) if resumable => {
            if job.limits.limited() {
                job.limits.check_later(index);
            } else {
                check_seams(client, job, index, false).await;
            }
            Ok(onward)
        }
        other => other,
    }
}

/// How many bytes before each range boundary are fetched as an overlap and
/// compared at assembly: reference A overlaps 64 bytes at each seam.
pub(super) const LEAD_BYTES: usize = 64;

/// One attempt at one range.
pub(super) async fn attempt_segment(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    resumable: bool,
    answer: Option<Answer>,
) -> Result<Option<(usize, Carried)>, Error> {
    // Read at each attempt: a split may have shortened the range since.
    let (start, end) = {
        let l = crate::lock(&job.live[index]);
        (l.start, l.end)
    };
    let span = (end - start).saturating_add(1);
    // The file this range's bytes are in, and where in it the range begins.
    let (held, origin) = job.held(index, start);
    // The ledger, read fresh: what it says is on disk is what has arrived.
    let already = job.ledger(index).min(span);
    let mut ledger = job.open_ledger(index).await?;
    let asked_at = tokio::time::Instant::now();
    // What a stop of an earlier run synced before writing it down needs no check (`read_synced`).
    let synced = already > 0 && read_synced(&job.parts, index).is_some_and(|s| s >= already);
    let trusted = {
        let mut l = crate::lock(&job.live[index]);
        l.trusted |= synced;
        l.reserved = l.reserved.max(already);
        // A new attempt: its pace is its own, from its first byte.
        l.going = Going {
            since: Some(asked_at),
            ..Going::default()
        };
        l.trusted
    };
    // A whole part needs nothing more, unless its tail is to be checked
    // against the server below: one this run wrote or checked has nothing to check.
    if already >= span && (!resumable || trusted) {
        job.segments.lock().await[index].done = span;
        return Ok(None);
    }

    /* A server that does not serve ranges is asked for the whole file, so
     * the whole file is what arrives — from byte 0. Seeking past what an
     * earlier attempt left wrote the body at that offset: the part only grew
     * and every retry after the first drop was corrupt. Start again. */
    let already = if !resumable && already > 0 {
        put(&mut ledger, 0).await?;
        forget_synced(&job.parts, index).await;
        job.downloaded.fetch_sub(already, Ordering::Relaxed);
        0
    } else {
        already
    };
    let (probe, carried) = match answer {
        Some(Answer::Probe(res)) => (Some(res), None),
        Some(Answer::Carried(carried)) => (None, Some(carried)),
        None => (None, None),
    };
    // An answer that went on from the range before is read on only into a range with nothing in it.
    let carried = carried.filter(|_| already == 0);
    let carried_on = carried.is_some();

    // A range after the first, starting fresh, asks for up to LEAD_BYTES more
    // at its front: the bytes of the range before it, checked at assembly. Not one
    // read on from the range before: those bytes came in the same answer.
    let lead = resumable && index > 0 && already == 0 && start > 0 && !carried_on;
    /* What a part already holds is asked for again at its end, and compared.
     * Its ledger says how much is there, and after a power cut the ledger can
     * be on disk while the last bytes are not — zeros, on NTFS, exFAT and
     * others — which a resume would have kept and built on, and called the
     * file finished. Where the two differ the part is cut back to the last
     * byte they agree on and fetched from there: repaired, not restarted.
     * Only for a ledger an earlier run wrote: a reconnect within this run
     * goes on from bytes this run put there (`Live::trusted`). */
    let check = if resumable && !trusted {
        already.min(RESUME_CHECK)
    } else {
        0
    };
    let from = if lead {
        start.saturating_sub(LEAD_BYTES as u64)
    } else {
        start + already - check
    };
    // How many lead bytes are actually before `start` (may be < LEAD_BYTES
    // when `start` itself is near the beginning of the file).
    let lead_n = if lead { (start - from) as usize } else { 0 };
    let probe = probe.filter(|_| from == 0 && already == 0);
    let is_probe_body = probe.is_some();

    /* Where this request stops. Without a cap, at the range's end, as ever, or
     * where the server limits this download, past it over the ranges after it
     * that nobody has run on (`limits::reach_from`): the answer goes on into
     * them as it comes to them. With a cap (`Job::chunk`, from the reading of a
     * video page), a slice of it: the connection asks for the next when this
     * one ends, and the range, the ledger and what a split sees are those of
     * the whole. */
    let asked_end = match job.chunk.filter(|_| resumable) {
        Some(cap) => end.min(from.saturating_add(cap.max(MIN_CHUNK)) - 1),
        None if probe.is_none() && !carried_on => reach_from(job, index, end).await,
        None => end,
    };
    // The last byte the answer brings: what was asked; the probe's runs to the
    // file's end; one read on, as far as its request asked.
    let reach = match &carried {
        Some(carried) => carried.reach,
        None if is_probe_body => job.size.map_or(u64::MAX, |size| size.saturating_sub(1)),
        None => asked_end,
    };
    crate::lock(&job.live[index]).reach = reach;
    let source = job.source(index);
    /* Read on from the range before: asked for, and checked, there. `carry` is the part of a
     * chunk still to be written: past the bytes a resume compares, or what came past the range
     * before's end. */
    let (mut stream, stopped, stated, mut carry, decoded, out) = if let Some(carried) = carried {
        let stopped: crate::decode::Stopped = Default::default();
        (
            carried.stream,
            stopped,
            None,
            carried.rest,
            false,
            carried.out,
        )
    } else {
        let (res, out) = if let Some(res) = probe {
            (res, Out::sent(&job.limits))
        } else {
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
                request =
                    request.header(reqwest::header::RANGE, format!("bytes={from}-{asked_end}"));
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
                if let Some(validator) = source.validator.as_ref().filter(|v| !v.starts_with("W/"))
                {
                    request = request.header(reqwest::header::IF_RANGE, validator);
                }
            }
            // Where the server limits this download, no sooner than its pace allows.
            if job.limits.limited() {
                job.limits.ready(&job.control).await?;
            }
            let sent = tokio::time::Instant::now();
            let out = Out::sent(&job.limits);
            let answered = Error::refused_or(
                or_stop(
                    &job.control,
                    crate::auth::send(client, request, &source.url),
                )
                .await?,
            )
            .await;
            // Turned away: the download's next request may wait for the server's pace.
            if let Err(Error::Refused {
                status: 429 | 503,
                retry_after,
                ..
            }) = &answered
            {
                let most = job.together.most();
                job.limits.turned_away(sent, out.beside, most, *retry_after);
            }
            (answered?, out)
        };
        let status = res.status();
        // The length an encoded body states is the encoded one, and what is written is decoded: held
        // to it, a whole file was taken as short and fetched again for as long as retries last
        // (engine review, 2026-11-08). A cut-off encoded body is still caught: the decoder says so.
        let stated = if crate::decode::coding_of(res.headers()).is_some() {
            None
        } else {
            res.content_length()
        };
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
            // A server that names no version, then or now, gives no sign of a change: its 200 is
            // ranges refused, fetched again on one connection, not "the file changed on the server"
            // (engine review, 2026-11-08).
            let unversioned = source.validator.is_none() && answered.is_none();
            if unversioned && !all_of_it {
                return Err(Error::RangesIgnored);
            }
            if !unversioned && !(same && all_of_it) {
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
            // The total it names is the file's length now: another than the one the ranges were
            // planned on is another file, whatever its validator says (a server that ignores
            // If-Range, or a weak ETag dm does not send), caught here rather than by the sampled
            // checks of the bytes (engine review, 2026-11-08).
            let said_total = res
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit_once('/'))
                .and_then(|(_, total)| total.trim().parse::<u64>().ok());
            if let (Some(total), Some(size)) = (said_total, job.size) {
                if total != size {
                    forget_plan(job).await;
                    return Err(Error::Changed);
                }
            }
        }
        // Undone here where a server compressed what it was not asked to: with a
        // count of what came over the wire, so that a body which unpacks to far
        // more is stopped (`decode.rs`).
        let stopped: crate::decode::Stopped = Default::default();
        let coding = crate::decode::coding_of(res.headers());
        let decoded = coding.is_some();
        let stream: BodyStream = match coding {
            Some(coding) => {
                crate::decode::decoded(coding, Box::pin(res.bytes_stream()), stopped.clone())
            }
            None => Box::pin(res.bytes_stream()),
        };
        (stream, stopped, stated, None, decoded, out)
    };

    /* Created if absent, never truncated.
     *
     * clippy suggests `.truncate(true)` here and it would be exactly wrong:
     * the staged file holds every range, so emptying it on any attempt
     * would throw away the bytes of all of them, and a range's own file
     * the bytes an earlier attempt left. Stated explicitly so the
     * next person to read the lint does not take its advice. */
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&held)
        .await?;

    let already = if check > 0 {
        let mut tail = vec![0u8; check as usize];
        let mut on_disk = tokio::fs::File::open(&held).await?;
        on_disk
            .seek(std::io::SeekFrom::Start(origin + already - check))
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
                // In the file the range is in, at the range's place there.
                let zeros_from =
                    zeros_ending_at(&held, origin + base, origin + floor).await? - origin;
                let cut = zeros_from - zeros_from % DISK_BLOCK;
                if cut < base {
                    put(&mut ledger, cut).await?;
                    forget_synced(&job.parts, index).await;
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
                    return Box::pin(attempt_segment(client, job, index, resumable, None)).await;
                }
            }
        }
        carry = rest;
        let keep = already - check + agree;
        if keep < already {
            put(&mut ledger, keep).await?;
            forget_synced(&job.parts, index).await;
            job.downloaded.fetch_sub(already - keep, Ordering::Relaxed);
            crate::lock(&job.live[index]).reserved = keep;
        }
        keep
    } else {
        already
    };
    // What the ledger vouches for is checked, or this run's own, from here on.
    crate::lock(&job.live[index]).trusted = true;

    /* Buffered, because chunks arrive far smaller than a useful write.
     *
     * Safe because the ledger is written down only after the buffer is
     * flushed: a retry starts from what the ledger says reached the disk, so
     * a buffer lost to a crash costs the bytes it held and nothing else. */
    /* Small: tokio's file keeps a copy of every write it hands to the
     * disk, so each connection holds this twice. 256 KiB cost 20–30 MB more
     * over ten downloads of eight connections than 16 KiB, and fetched 2 GB
     * over loopback no faster nor with less CPU (qa/after.md). But not under
     * the 16 to 64 KiB a held-down server sends at a time: at 16 KiB nearly
     * every piece went past the buffer as a write of its own, each a trip to
     * a blocking thread and a system call, which were most of the engine's
     * time over eight connections held to 1 MiB/s (gaps row 176). Tokio's
     * copy already grows to the 64 KiB such a server sends at most. */
    let mut file = tokio::io::BufWriter::with_capacity(WRITE_BUFFER, file);
    // At this range's own place in the staged file, or in its own file.
    file.seek(std::io::SeekFrom::Start(origin + already))
        .await?;

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
    let mut lead_buf: Vec<u8> = Vec::with_capacity(lead_n);
    let mut reader = Reader::new();
    let mut window = Window::open(written, job);
    // Counted among the requests bringing bytes in from its first byte (`grow`); that byte also
    // sends at once every connection waiting its turn through an outage (`retry::Together`). The
    // probe's from the start: the server answered it before any range was asked for, and the
    // ranges it turns away meanwhile are turned away while it serves another (`fetch_segment`).
    let mut flowing = is_probe_body.then(|| job.together.bringing());
    // A pause seen, and how much has been taken since of what had already come (`DRAIN_MOST`).
    let mut draining: Option<Error> = None;
    let mut drained = 0u64;
    // What came past the range's end, where the answer goes on.
    let mut onward: Option<bytes::Bytes> = None;

    loop {
        /* Paused: what has already come, in the connection's buffers and the
         * system's, is taken and written before the range lets go, without
         * waiting for more. Dropped, it was fetched again at the resume: on a
         * fast link most of what a stop cost twice (gaps row 186). */
        if carry.is_none() {
            if let Some(stopped) = draining.take() {
                let ready = (drained < DRAIN_MOST)
                    .then(|| futures_util::FutureExt::now_or_never(stream.next()))
                    .flatten()
                    .flatten();
                match ready {
                    Some(Ok(more)) => {
                        draining = Some(stopped);
                        carry = Some(more);
                    }
                    _ => {
                        file.flush().await?;
                        put(&mut ledger, written).await?;
                        // Cancelled meanwhile, it is a cancel.
                        let stopped = job.control.check().err().unwrap_or(stopped);
                        if matches!(stopped, Error::Paused) {
                            stopped_synced(job, index, file.get_ref(), written).await;
                            job.segments.lock().await[index].done = written.min(span_now);
                        }
                        return Err(stopped);
                    }
                }
            }
        }
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
                // While what came is not yet written down, no longer than the
                // ledger's quarter second: a server that goes quiet does not
                // keep what it sent in the write buffer, off the disk a preview
                // reads and past the ledger a crash goes back to.
                let until = if written > recorded {
                    window.ends().min(tokio::time::Instant::from_std(
                        recorded_at + LEDGER_EVERY_TIME,
                    ))
                } else {
                    window.ends()
                };
                match tokio::time::timeout_at(until, reader.next(&mut stream, &job.control)).await {
                    // Quiet past the quarter: what came is written down.
                    Err(_) if written > recorded && recorded_at.elapsed() >= LEDGER_EVERY_TIME => {
                        file.flush().await?;
                        put(&mut ledger, written).await?;
                        recorded = written;
                        recorded_at = std::time::Instant::now();
                        continue;
                    }
                    // The window ran out with nothing arriving: judged at the top.
                    Err(_) => continue,
                    Ok(Ok(Some(chunk))) => chunk,
                    Ok(Ok(None)) => break,
                    // Paused while waiting: what has come is taken first (above).
                    Ok(Err(Error::Paused)) => {
                        draining = Some(Error::Paused);
                        continue;
                    }
                    // Stopped while waiting: what is in the write buffer is on
                    // disk before the range lets go, or a resume fetches it again.
                    Ok(Err(stopped)) => {
                        file.flush().await?;
                        put(&mut ledger, written).await?;
                        if matches!(stopped, Error::Paused) {
                            stopped_synced(job, index, file.get_ref(), written).await;
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
        // The lead bytes are kept aside, not written into this range.
        if want_lead && !chunk.is_empty() {
            let need = lead_n.saturating_sub(lead_buf.len());
            let take = need.min(chunk.len());
            lead_buf.extend_from_slice(&chunk[..take]);
            chunk = chunk.slice(take..);
            if lead_buf.len() >= lead_n {
                if let Some(slot) = job.overlaps.get(index) {
                    if let Ok(mut guard) = slot.lock() {
                        *guard = Some(lead_buf.clone());
                    }
                }
                want_lead = false;
            }
        }

        /* Checked between chunks rather than mid-write, so what is flushed
         * and what the ledger records stay obviously in step. A pause takes
         * this chunk, and what else has come, first (`draining`). */
        if let (None, Err(Error::Paused)) = (&draining, job.control.check()) {
            draining = Some(Error::Paused);
            carry = Some(chunk);
            continue;
        }
        if let (None, Err(stopped)) = (&draining, job.control.check()) {
            // Flushed first either way: the bytes already written are what a
            // resume picks up from, and a pause also records where it got to.
            file.flush().await?;
            put(&mut ledger, written).await?;
            if matches!(stopped, Error::Paused) {
                stopped_synced(job, index, file.get_ref(), written).await;
                job.segments.lock().await[index].done = written.min(span_now);
            }
            return Err(stopped);
        }

        if !is_probe_body && start + written + chunk.len() as u64 > reach.max(end) + 1 {
            /* More bytes than the request asked for: past the range's end, or
             * past the ranges after it it asked on over (`reach`).
             *
             * A server holding a compressed representation ranges over the
             * *encoded* bytes and hands back the decoded ones, and nothing in
             * the response says the two do not correspond. Trimming to length
             * here is what makes that silent: every part ends up exactly as
             * long as planned, assembled in order, and wrong all the way
             * through. A file of the right size and the wrong contents is the
             * one thing a download manager must never produce.
             *
             * Skipped for a probe body: the probe asked for bytes=0- (the
             * whole file) and the server is legitimately sending all of it;
             * the clipping below stops at this segment's boundary. */
            file.flush().await?;
            put(&mut ledger, written).await?;
            return Err(Error::DishonestRanges);
        }

        // Claimed before it is written, against where the range ends now:
        // a split under this connection stops it at the new end.
        let arrived = tokio::time::Instant::now();
        let (fits, first) = {
            let mut l = crate::lock(&job.live[index]);
            span_now = (l.end - l.start).saturating_add(1);
            let fits = (chunk.len() as u64).min(span_now.saturating_sub(written));
            l.reserved = written + fits;
            let held = l.reserved;
            let first = l.going.came(arrived, held);
            (fits, first)
        };
        // Read on from the range before, its first bytes waited for nothing.
        if first && !carried_on {
            job.note_first_byte(arrived.saturating_duration_since(asked_at));
        }
        let cut = fits < chunk.len() as u64;
        if cut {
            // What is past the range's end: the next range's, where the answer goes on into it.
            onward = Some(chunk.slice(fits as usize..));
            chunk = chunk.slice(..fits as usize);
        }
        as_slow_as_the_disk(chunk.len() as u64).await;
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        if draining.is_some() {
            drained += chunk.len() as u64;
        }
        flowing.get_or_insert_with(|| job.together.bringing());
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
        // Written down every half megabyte or quarter second: a crash then
        // costs no more than that of this range, fetched again.
        if written > recorded
            && (written - recorded >= LEDGER_EVERY || recorded_at.elapsed() >= LEDGER_EVERY_TIME)
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
        let stopping = if draining.is_some() {
            // Taken while stopping: no ceiling to wait for, only counted.
            through = chunk.len() as u64;
            job.downloaded.fetch_add(through, Ordering::Relaxed);
            Ok(())
        } else {
            or_stop(&job.control, async {
                job.limiter
                    .take_counting(chunk.len() as u64, &job.downloaded, &mut through)
                    .await;
                Ok(())
            })
            .await
        };
        if let Err(stopped) = stopping {
            // It is on disk: the count is a ledger of what is, and a retry
            // of this range starts from what its ledger records.
            job.downloaded
                .fetch_add(chunk.len() as u64 - through, Ordering::Relaxed);
            file.flush().await?;
            put(&mut ledger, written).await?;
            if matches!(stopped, Error::Paused) {
                stopped_synced(job, index, file.get_ref(), written).await;
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
        return Ok(None);
    }

    // A slice that came whole is not a short range: the next is asked for.
    if asked_end < end && written < span_now && start + written > asked_end {
        return Ok(None);
    }

    // A stream that ended early without erroring is not a finished range.
    if written < span_now {
        return Err(Error::Incomplete {
            got: written,
            expected: span_now,
        });
    }

    /* Whole, with the answer going on past it: where the server limits this download, read on
     * into the range after it where nobody has that one, rather than asked for again. Only from
     * the range's very end (a range halved under a connection already past the new end stops
     * there), and never through a decoder or under a cap. */
    if let Some(rest) = onward {
        let at = start + written;
        let may = job.limits.limited()
            && resumable
            && job.chunk.is_none()
            && job.size.is_some()
            && !decoded
            && written == span_now;
        if may {
            if let Some(next) = continue_into(job, index, at, reach).await {
                return Ok(Some((
                    next,
                    Carried {
                        stream,
                        rest: Some(rest).filter(|rest| !rest.is_empty()),
                        reach,
                        out,
                    },
                )));
            }
        }
    }
    Ok(None)
}

/* ------------------------------------------------------------- streams */
