// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! One attempt at one range: what it asks for, the answer opened and checked, the part's
//! tail compared where a resume found it, the body read into place, and the range's end.

use std::ops::ControlFlow;

use super::*;

mod reading;

/// What an attempt asks for, worked out before it asks.
struct Plan {
    /// The range's first byte, as a split last left it.
    start: u64,
    /// Its last byte.
    end: u64,
    /// Its length.
    span: u64,
    /// The file its bytes are in.
    held: PathBuf,
    /// Where in that file the range begins.
    origin: u64,
    /// How much of it is on disk: the ledger's, cut back where its tail differed.
    already: u64,
    /// When the attempt began.
    asked_at: tokio::time::Instant,
    /// Whether the bytes before the range are asked for, to be checked at assembly.
    lead: bool,
    /// How many of them.
    lead_n: usize,
    /// How many bytes at the end of what is on disk are asked for again and compared.
    check: u64,
    /// The first byte asked for.
    from: u64,
    /// The last byte asked for.
    asked_end: u64,
    /// The last byte the answer brings.
    reach: u64,
    /// Whether the answer read is the probe's.
    is_probe_body: bool,
    /// Whether the answer read is one read on from the range before.
    carried_on: bool,
}

/// A [`Plan`] with the ledger it was read from and the answer it may read rather than ask.
struct Planned {
    plan: Plan,
    ledger: fs::File,
    probe: Option<First>,
    carried: Option<Carried>,
}

/// The answer an attempt reads, opened and checked.
struct Answered {
    stream: BodyStream,
    /// Where a decoder says why it stopped undoing the body.
    stopped: crate::decode::Stopped,
    /// The body's length, as the server stated it.
    stated: Option<u64>,
    /// Bytes already in hand to be written first.
    carry: Option<bytes::Bytes>,
    decoded: bool,
    out: Out,
}

/// What a resume's check of the part's tail leaves to go on with.
struct Kept {
    file: fs::File,
    stream: BodyStream,
    carry: Option<bytes::Bytes>,
}

/// Where the read loop goes after a step that does not hand it a chunk.
enum Flow {
    /// Round again: what the step did is done.
    Again,
    /// Out of the loop: the answer has ended, or the range is whole.
    Ended,
}

/// An attempt's body being read into its place. Its fields are dropped in the order its
/// locals were: the last made first.
struct Reading<'a> {
    job: &'a Job,
    index: usize,
    /// What came past the range's end, where the answer goes on.
    onward: Option<bytes::Bytes>,
    /// How much the attempt has taken off its answer, with what it was handed to write first
    /// (`carry`): what it does not write by its end is counted (`unwritten`, gaps row 186).
    received: u64,
    /// How much has been taken since a pause of what had already come (`DRAIN_MOST`).
    drained: u64,
    /// A pause seen.
    draining: Option<Error>,
    /// Counted among the requests bringing bytes in from its first byte.
    flowing: Option<retry::Bringing<'a>>,
    window: Window,
    /// When it last brought bytes, or began (`helping::holds_the_end`).
    quiet_since: tokio::time::Instant,
    reader: Reader,
    lead_buf: Vec<u8>,
    want_lead: bool,
    /// Where this range ends now: sooner than asked, once a split has taken its back half.
    span_now: u64,
    /// When the ledger last said what it says.
    recorded_at: std::time::Instant,
    /// What the ledger says.
    recorded: u64,
    /// Where the free space was last looked at (`DISK_LOOK_EVERY`).
    disk_looked: u64,
    written: u64,
    file: tokio::io::BufWriter<fs::File>,
    out: Out,
    decoded: bool,
    carry: Option<bytes::Bytes>,
    stated: Option<u64>,
    stopped: crate::decode::Stopped,
    stream: BodyStream,
    ledger: fs::File,
}

/// One attempt at one range.
pub(in crate::download) async fn attempt_segment(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    resumable: bool,
    answer: Option<Answer>,
) -> Result<Option<(usize, Carried)>, Error> {
    let Some(Planned {
        mut plan,
        mut ledger,
        probe,
        carried,
    }) = plan_attempt(job, index, resumable, answer).await?
    else {
        return Ok(None);
    };
    let Answered {
        stream,
        stopped,
        stated,
        carry,
        decoded,
        out,
    } = open_answer(client, job, index, resumable, &plan, probe, carried).await?;
    let Some(Kept {
        file,
        stream,
        carry,
    }) = tail_checked(job, index, &mut plan, &mut ledger, stream, carry).await?
    else {
        // Cut back to where a power cut's zeros begin: straight on to the window there.
        return Box::pin(attempt_segment(client, job, index, resumable, None)).await;
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
    file.seek(std::io::SeekFrom::Start(plan.origin + plan.already))
        .await?;

    let written = plan.already;
    // Where the free space was last looked at (`DISK_LOOK_EVERY`).
    let disk_looked = plan.already;
    // What the ledger says and when it said it, so it is written down
    // again every few megabytes or every second rather than every chunk.
    let recorded = plan.already;
    let recorded_at = std::time::Instant::now();
    // Where this range ends now: sooner than asked, once a split has taken
    // its back half.
    let span_now = plan.span;
    let want_lead = plan.lead;
    let lead_buf: Vec<u8> = Vec::with_capacity(plan.lead_n);
    let reader = Reader::new();
    let window = Window::open(written, job);
    // Counted among the requests bringing bytes in from its first byte (`grow`); that byte also
    // sends at once every connection waiting its turn through an outage (`retry::Together`). The
    // probe's from the start: the server answered it before any range was asked for, and the
    // ranges it turns away meanwhile are turned away while it serves another (`fetch_segment`).
    let flowing = plan.is_probe_body.then(|| job.together.bringing());
    // A pause seen, and how much has been taken since of what had already come (`DRAIN_MOST`).
    let draining: Option<Error> = None;
    let drained = 0u64;
    // What it was handed to write first: received, and to be written.
    let received = carry.as_ref().map_or(0, |carry| carry.len() as u64);
    // What came past the range's end, where the answer goes on.
    let onward: Option<bytes::Bytes> = None;
    let mut reading = Reading {
        job,
        index,
        onward,
        received,
        drained,
        draining,
        flowing,
        window,
        quiet_since: tokio::time::Instant::now(),
        reader,
        lead_buf,
        want_lead,
        span_now,
        recorded_at,
        recorded,
        disk_looked,
        written,
        file,
        out,
        decoded,
        carry,
        stated,
        stopped,
        stream,
        ledger,
    };
    if let Err(stopped) = reading.read(&plan).await {
        reading.count_unwritten(&plan, 0);
        return Err(stopped);
    }
    reading.finish(&plan, resumable).await
}

/// What an attempt asks for: read fresh from the range and its ledger, the answer it may
/// read rather than ask kept where it fits. `None` where the part is whole and needs nothing.
async fn plan_attempt(
    job: &Job,
    index: usize,
    resumable: bool,
    answer: Option<Answer>,
) -> Result<Option<Planned>, Error> {
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
        Some(Answer::Probe(first)) => (Some(first), None),
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
    // Where the probe's answer stops short of the file's end: asked for its first part only.
    let probe_brings = probe.as_ref().and_then(|first| first.brings);
    // Where an answer read on from the range before ends: inside this range where it is the
    // probe's first part, read on past range 0's end.
    let carried_reach = carried.as_ref().map(|carried| carried.reach);

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
        // The probe's first part ends where it ends, and so does an answer read on from the
        // range before: the range is asked for on from there.
        None => probe_brings
            .or(carried_reach)
            .map_or(end, |brings| brings.min(end)),
    };
    // The last byte the answer brings: what was asked; the probe's to where it
    // said it stops, or else to the file's end; one read on, as far as its
    // request asked.
    let reach = match &carried {
        Some(carried) => carried.reach,
        None if is_probe_body => {
            probe_brings.unwrap_or_else(|| job.size.map_or(u64::MAX, |size| size.saturating_sub(1)))
        }
        None => asked_end,
    };
    crate::lock(&job.live[index]).reach = reach;
    Ok(Some(Planned {
        plan: Plan {
            start,
            end,
            span,
            held,
            origin,
            already,
            asked_at,
            lead,
            lead_n,
            check,
            from,
            asked_end,
            reach,
            is_probe_body,
            carried_on,
        },
        ledger,
        probe,
        carried,
    }))
}

/// The answer the attempt reads: one read on from the range before, the probe's, or a
/// request of its own, checked against what was asked and undone where it was compressed.
async fn open_answer(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    resumable: bool,
    plan: &Plan,
    probe: Option<First>,
    carried: Option<Carried>,
) -> Result<Answered, Error> {
    let source = job.source(index);
    /* Read on from the range before: asked for, and checked, there. `carry` is the part of a
     * chunk still to be written: past the bytes a resume compares, or what came past the range
     * before's end. */
    if let Some(carried) = carried {
        let stopped: crate::decode::Stopped = Default::default();
        return Ok(Answered {
            stream: carried.stream,
            stopped,
            stated: None,
            carry: carried.rest,
            decoded: false,
            out: carried.out,
        });
    }
    let (res, out) = if let Some(first) = probe {
        (first.response, Out::sent(&job.limits))
    } else {
        send_range(client, job, source, resumable, plan.from, plan.asked_end).await?
    };
    // The length an encoded body states is the encoded one, and what is written is decoded: held
    // to it, a whole file would be taken as short and fetched again for as long as retries
    // last (engine review). A cut-off encoded body is still caught: the decoder says so.
    let stated = if crate::decode::coding_of(res.headers()).is_some() {
        None
    } else {
        res.content_length()
    };
    check_answer(
        job,
        source,
        res.status(),
        res.headers(),
        resumable,
        plan,
        stated,
    )
    .await?;
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
    Ok(Answered {
        stream,
        stopped,
        stated,
        carry: None,
        decoded,
        out,
    })
}

/// The range's request, sent once the server's pace allows: its answer, and the request
/// counted among those at the server.
async fn send_range(
    client: &reqwest::Client,
    job: &Job,
    source: &Source,
    resumable: bool,
    from: u64,
    asked_end: u64,
) -> Result<(reqwest::Response, Out), Error> {
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
    // How long the network this connection goes over took to answer (gaps row K4).
    if answered.is_ok() {
        crate::second_path::answered_in(crate::second_path::task_network(), sent.elapsed());
    }
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
    Ok((answered?, out))
}

/// Refuses an answer that is not the range asked for: the whole file where a part was asked
/// (ranges ignored, or another version of the file), another range, or another length.
async fn check_answer(
    job: &Job,
    source: &Source,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    resumable: bool,
    plan: &Plan,
    stated: Option<u64>,
) -> Result<(), Error> {
    let (from, end) = (plan.from, plan.end);
    if resumable && status != reqwest::StatusCode::PARTIAL_CONTENT {
        // The server answered the whole file to a request for part of it.
        // Under the validator the ranges were planned under, it is the same
        // file served whole; under another, everything staged belongs to a
        // version that is gone.
        let header = |name| {
            headers
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
        // (engine review).
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
        let said_start = headers
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
        // checks of the bytes (engine review).
        let said_total = headers
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
    Ok(())
}

/// Opens the part's file, and where a resume asked again for its tail, compares it: kept to
/// where the two agree, the whole download refused where the file changed. `None` where the
/// part was cut back to where a power cut's zeros begin, for a new attempt from there.
async fn tail_checked(
    job: &Job,
    index: usize,
    plan: &mut Plan,
    ledger: &mut fs::File,
    mut stream: BodyStream,
    mut carry: Option<bytes::Bytes>,
) -> Result<Option<Kept>, Error> {
    let (origin, already, check) = (plan.origin, plan.already, plan.check);
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
        .open(&plan.held)
        .await?;

    plan.already = if check > 0 {
        let mut tail = vec![0u8; check as usize];
        let mut on_disk = tokio::fs::File::open(&plan.held).await?;
        on_disk
            .seek(std::io::SeekFrom::Start(origin + already - check))
            .await?;
        tokio::io::AsyncReadExt::read_exact(&mut on_disk, &mut tail).await?;
        drop(on_disk);
        let (agree, rest) = compare_tail(&mut stream, &tail, &job.control).await?;
        // The bytes that agree were on the disk already: received twice (gaps row 186).
        crate::download::unwritten::count(index, agree);
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
                    zeros_ending_at(&plan.held, origin + base, origin + floor).await? - origin;
                let cut = zeros_from - zeros_from % DISK_BLOCK;
                if cut < base {
                    // What came past the bytes compared goes with the connection, fetched again.
                    let dropped = rest.as_ref().map_or(0, |rest| rest.len() as u64);
                    crate::download::unwritten::count(index, dropped);
                    put(ledger, cut).await?;
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
                    return Ok(None);
                }
            }
        }
        carry = rest;
        let keep = already - check + agree;
        if keep < already {
            put(ledger, keep).await?;
            forget_synced(&job.parts, index).await;
            job.downloaded.fetch_sub(already - keep, Ordering::Relaxed);
            crate::lock(&job.live[index]).reserved = keep;
        }
        keep
    } else {
        already
    };
    Ok(Some(Kept {
        file,
        stream,
        carry,
    }))
}
