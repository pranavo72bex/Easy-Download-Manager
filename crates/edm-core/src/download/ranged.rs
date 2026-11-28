// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::sync::atomic::AtomicBool;

use super::*;

mod attempt;
mod connection;
mod halving;
mod helping;
mod limits;
mod segment;
mod staged;

use attempt::attempt_segment;
use connection::Connection;
use halving::{split_largest, Halving};
use helping::*;
pub(super) use limits::Limits;
use limits::*;
use segment::*;
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

/// Part files an earlier EDM left go into the staged file first. A ledger with no staged file
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
    // The networks chosen beside the system's route (a second, a third; gaps rows 47, K3):
    // the connections are spread over them in turn (`second_path::network_for`). The ranges
    // are taken as connections free up, so a faster network fetches more of them.
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
        let first = client.clone();
        let (network, client) = crate::second_path::network_for(client, ordinal);
        // Its reads take from the second network's own limit where it goes out over it (gaps
        // row K5).
        running.spawn(
            crate::second_path::ON_SECOND_TASK.scope(
                std::cell::Cell::new(network),
                Connection {
                    client,
                    first,
                    job: Arc::clone(job),
                    probe: Arc::clone(&probe),
                    next: Arc::clone(&next),
                    orphans: Arc::clone(&orphans),
                    active: Arc::clone(&active),
                    first_wave: Arc::clone(&first_wave),
                    wave_over: wave_over.clone(),
                    wave_over_seen: wave_over_seen.clone(),
                    gate_seen: gate_seen.clone(),
                    planned,
                    resumable,
                    started,
                    helper,
                    grown_as,
                    network,
                    counted: !helper,
                    alone_again: false,
                }
                .work(),
            ),
        );
    }

    /* The first failure is the download's. The other connections are then
     * stopped and waited out before it is returned: dropping their handles
     * leaves them running, still writing parts and the plan beside them —
     * under a download that has already failed, or under the fresh attempt
     * a changed file starts, which would find the old version's parts
     * adopted again. A pause or a cancel stops every connection by itself,
     * each flushing what it has, so those get a moment to; one stalled
     * where the stop does not reach it is stopped after that. */
    let failed = ended(&mut running).await;
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
    fetch_left(client, job, resumable).await?;
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

/// Waits for `run`'s connections as each ends, in whatever order: the first failure is the
/// download's, seen when it happens, not after the connections spawned before it are done.
async fn ended(running: &mut tokio::task::JoinSet<Result<(), Error>>) -> Option<Error> {
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
    failed
}

/// Fetches each range given up on after the others had finished, or never taken, on one
/// connection, its failure this download's.
async fn fetch_left(client: &reqwest::Client, job: &Job, resumable: bool) -> Result<(), Error> {
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
            0,
        )
        .await;
        fetched?;
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
    network: u8,
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
                crate::second_path::fetched_by(&job.parts, next, network);
                index = next;
                answer = Some(Answer::Carried(carried));
            }
            Ok(None) => return (index, Ok(())),
            Err(e) => return (index, Err(e)),
        }
    }
}

/// A range stopped by a pause (a person's, or EDM ending): what it wrote is put on the disk itself
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
    let busy = |e: &Error| steps_aside(e, job, active, turned_away, &at_start, &on_disk);
    // Refused by a server that limits how often it takes a request: asked again when its pace
    // allows, not on the backoff's growing waits (`limits::Limits::turned_away`).
    let paced = |e: &Error| paced_wait(e, job);
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

    settled(job, index, resumable, outcome).await
}

/// How many bytes before each range boundary are fetched as an overlap and
/// compared at assembly: reference A overlaps 64 bytes at each seam.
pub(super) const LEAD_BYTES: usize = 64;

/* ------------------------------------------------------------- streams */
