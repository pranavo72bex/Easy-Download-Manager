// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The seams checked: the bytes either side of each seam, and the file's end, asked for again while
//! the download goes on and compared with what was written.

use super::*;

/// How much of each side of a seam, and of the file's end, is fetched a second time and compared
/// with what the connections wrote. Another version of a file, or another place in it, differs
/// within any few hundred bytes of it, and the 64 bytes before each range (`LEAD_BYTES`) already
/// catch a server that labels a range with the wrong place. Each range's last 4 KiB asked apart
/// from 2 KiB either side of up to eight seams was 66 KB of a 64 MiB file fetched twice: all of
/// the 0.1% that tied reference A's floor in `bench` per_conn (gaps row 177). At a kibibyte a side
/// the checks were 15 KB, which with what the server had sent past the first range's end (up to
/// 18 KB) still came to 0.05% in one run of four.
pub(in crate::download) const SEAM_WINDOW: u64 = 512;

/// Once range `index` is whole: each seam between it and a neighbour that is whole too, and the
/// file's last bytes where it is the last range, claimed for asking again and compared with the
/// bytes on disk, as reference A looks at its seams a second time. A seam is claimed once, by
/// whichever of its two ranges is whole second, so the end of every range is looked at in the
/// same request as the start of the next, while the download goes on. Asked one after another
/// after the last byte, with every range's end asked apart as well, the finish would wait out
/// eight round trips and each range's end would be fetched twice over (gaps rows 171 and 177).
///
/// The spans are handed on to the download's checks (`ask_handed`), not asked on the range's own
/// connection: that connection takes its next work at once, a range to halve or to fetch, rather
/// than waiting out a round trip or two of its checks first (gaps row S4). The checks go out
/// on the places of the connections that are done with ranges, as many at once as there are such
/// places, so the server is never asked more at once than the download's connections asked of it,
/// and a site limited to so many connections sees no more (`Options::host_limits`). At the end,
/// the range whole last finds the others' places free and its two checks (a seam either side, or
/// a seam and the file's end) go out at once, as they would on its own connection.
///
/// A request that fails or is not answered as a range proves nothing and is let pass; bytes that
/// differ, or the whole file answered under the validator (another version of it), are a finding,
/// and the file is fetched again on one connection (`Job::disagreed`).
pub(in crate::download) async fn hand_on(job: &Job, index: usize) {
    let spans = claim(job, index).await;
    if spans.is_empty() {
        return;
    }
    crate::lock(&job.handed.due).extend(spans);
    job.handed.told.notify_one();
}

/// The seams of range `index` where the server limits the download (`ranged::limits`), asked once
/// no range is being asked for, one after the other, and no more once one is refused. False where
/// one was refused, so the caller asks none after it.
pub(in crate::download) async fn check_seams(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
) -> bool {
    let spans = claim(job, index).await;
    ask_all(client, job, spans, true).await
}

/// A span of the file to ask for again: the range whole second that claimed it, where it is, and
/// the ranges as they stood then.
pub(super) struct Span {
    index: usize,
    low: u64,
    high: u64,
    segments: Arc<Vec<Segment>>,
}

/// The checks the ranges have handed on, and the places they go out on (`hand_on`,
/// `ask_handed`).
#[derive(Default)]
pub(in crate::download) struct Handed {
    /// Claimed, not yet asked.
    due: std::sync::Mutex<std::collections::VecDeque<Span>>,
    /// The connections still looking for ranges, any of which may hand on more.
    fetching: AtomicUsize,
    /// The places of the connections done with ranges that fetched any: as many checks as this go
    /// out at once.
    freed: AtomicUsize,
    /// Told when a check is handed on or a connection is done with ranges.
    told: tokio::sync::Notify,
}

impl Handed {
    /// A download of `connections` connections, each looking for ranges.
    pub(in crate::download) fn begin(&self, connections: usize) {
        self.fetching.store(connections, Ordering::SeqCst);
    }

    /// A connection done with ranges, however it ended; its place is free for a check where it
    /// fetched any (one that never did has asked the server for nothing).
    pub(in crate::download) fn done(&self, fetched: bool) {
        if fetched {
            self.freed.fetch_add(1, Ordering::SeqCst);
        }
        let _ = self
            .fetching
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        self.told.notify_one();
    }
}

/// Asks the checks the ranges hand on (`hand_on`) while the download goes on, as many at once as
/// the connections done with ranges have left places (`Handed::freed`). Ends once no connection
/// is looking for ranges and nothing is being asked, or once the server limits the download,
/// whose checks wait until no range is being asked for (`ranged::run`); what is left then is
/// asked by `ask_left`.
pub(in crate::download) async fn ask_handed(
    client: &reqwest::Client,
    job: &Job,
) -> Result<(), Error> {
    let handed = &job.handed;
    let mut asking = futures_util::stream::FuturesUnordered::new();
    loop {
        let told = handed.told.notified();
        if !job.limits.limited() {
            while asking.len() < handed.freed.load(Ordering::SeqCst) {
                let Some(span) = crate::lock(&handed.due).pop_front() else {
                    break;
                };
                asking.push(async move {
                    ask(client, job, &span).await;
                });
            }
        }
        if asking.is_empty()
            && (job.limits.limited() || handed.fetching.load(Ordering::SeqCst) == 0)
        {
            return Ok(());
        }
        tokio::select! {
            _ = asking.next(), if !asking.is_empty() => {}
            () = told => {}
        }
        job.control.check()?;
    }
}

/// The checks handed on that `ask_handed` left, asked once no range is being asked for: at once,
/// or `one_at_a_time` where the server limits the download, no more once one is refused. False
/// where one was refused.
pub(in crate::download) async fn ask_left(
    client: &reqwest::Client,
    job: &Job,
    one_at_a_time: bool,
) -> bool {
    let spans: Vec<Span> = crate::lock(&job.handed.due).drain(..).collect();
    ask_all(client, job, spans, one_at_a_time).await
}

/// The spans range `index`, now whole, is to have asked again: each seam with a neighbour whole
/// too that no other range has claimed, and the file's end where it is the last range.
async fn claim(job: &Job, index: usize) -> Vec<Span> {
    use std::sync::atomic::Ordering::SeqCst;
    let segments = job.segments.lock().await;
    let Some(me) = segments.get(index).filter(|s| s.finished()) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    // The range just before this one in the file, and the one just after it.
    let before = segments
        .iter()
        .position(|s| s.end.checked_add(1) == Some(me.start));
    let after = segments
        .iter()
        .position(|s| me.end.checked_add(1) == Some(s.start));
    for (lower, higher) in [(before, Some(index)), (Some(index), after)] {
        let (Some(lower), Some(higher)) = (lower, higher) else {
            continue;
        };
        let (low_side, high_side) = (&segments[lower], &segments[higher]);
        // A seam is known by the range that starts at it, whose start never moves.
        let claimed = || {
            job.seams_checked
                .get(higher)
                .is_some_and(|checked| !checked.swap(true, SeqCst))
        };
        if low_side.finished() && high_side.finished() && claimed() {
            let seam = high_side.start;
            spans.push((
                seam - low_side.span().min(SEAM_WINDOW),
                seam + high_side.span().min(SEAM_WINDOW) - 1,
            ));
        }
    }
    let last = job
        .size
        .is_some_and(|size| me.end.checked_add(1) == Some(size));
    if last && !job.end_checked.swap(true, SeqCst) {
        spans.push((me.end + 1 - me.span().min(SEAM_WINDOW), me.end));
    }
    if spans.is_empty() {
        return Vec::new();
    }
    let segments = Arc::new(segments.clone());
    spans
        .into_iter()
        .map(|(low, high)| Span {
            index,
            low,
            high,
            segments: Arc::clone(&segments),
        })
        .collect()
}

/// Asks `span` again; bytes that differ mark the download (`Job::disagreed`).
async fn ask(client: &reqwest::Client, job: &Job, span: &Span) -> Look {
    let (low, high) = (span.low, span.high);
    let look = span_agrees(client, job, span.index, &span.segments, low, high).await;
    if matches!(look, Look::Differs) {
        job.disagreed.store(true, Ordering::SeqCst);
    }
    look
}

/// Asks `spans` at once, or `one_at_a_time`, no more once one is refused. False where one was
/// refused.
async fn ask_all(
    client: &reqwest::Client,
    job: &Job,
    spans: Vec<Span>,
    one_at_a_time: bool,
) -> bool {
    let refused = |look: &Look| matches!(look, Look::Unproven { refused: true });
    if one_at_a_time {
        for span in &spans {
            if refused(&ask(client, job, span).await) {
                return false;
            }
        }
        true
    } else {
        let asked = spans.iter().map(|span| ask(client, job, span));
        let looks = futures_util::future::join_all(asked).await;
        !looks.iter().any(refused)
    }
}

/// What a span asked for again showed.
enum Look {
    /// The bytes on disk.
    Same,
    /// Other bytes, or another version of the file.
    Differs,
    /// Nothing either way: the request failed, or was answered otherwise than as the range
    /// (`refused` where the server turned it away, 429 or 503).
    Unproven { refused: bool },
}

/// How long the bytes of a seam fetched afresh to compare ([`span_agrees`]) are given to arrive
/// before the seam is left unproven.
const SPAN_WITHIN: std::time::Duration = std::time::Duration::from_secs(15);

/// Whether `low..=high` of the file, fetched afresh from the source of range `index` exactly as
/// a range is asked for (with the headers a stream's reader says its server wants, and under the
/// validator), is the bytes on disk.
async fn span_agrees(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    segments: &[Segment],
    low: u64,
    high: u64,
) -> Look {
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
    request = request.header(reqwest::header::RANGE, format!("bytes={low}-{high}"));
    let conditional = source.validator.as_ref().filter(|v| !v.starts_with("W/"));
    if let Some(validator) = conditional {
        request = request.header(reqwest::header::IF_RANGE, validator);
    }
    let Ok(res) = or_stop(
        &job.control,
        crate::auth::send(client, request, &source.url),
    )
    .await
    else {
        return Look::Unproven { refused: false };
    };
    /* Asked under the validator, the whole file instead of the range is the server saying the
     * file is no longer that version: the bytes on disk are not all of one file (a file
     * republished during the download, `zoo.rs` changes-midway). */
    if conditional.is_some() && res.status() == reqwest::StatusCode::OK {
        crate::finish_trace::mark(&format!(
            "seam {low}-{high} of range {index}: another version"
        ));
        return Look::Differs;
    }
    let unproven = Look::Unproven { refused: false };
    if res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        let refused = matches!(res.status().as_u16(), 429 | 503);
        return Look::Unproven { refused };
    }
    let wanted = high - low + 1;
    if res.content_length().is_some_and(|n| n != wanted) {
        return unproven;
    }
    let Ok(Ok(there)) = tokio::time::timeout(SPAN_WITHIN, res.bytes()).await else {
        return unproven;
    };
    if there.len() as u64 != wanted {
        return unproven;
    }
    let Some(here) = bytes_at(job, segments, low, wanted).await else {
        return unproven;
    };
    if there[..] == here[..] {
        Look::Same
    } else {
        let at = there.iter().zip(here.iter()).position(|(a, b)| a != b);
        crate::finish_trace::mark(&format!(
            "seam {low}-{high} of range {index}: differs from byte {at:?} of the span"
        ));
        Look::Differs
    }
}

/// `length` bytes of the file from `at`, read from where its ranges are: the staged file, or each
/// range's own file for the part of the span it holds. `None` where any of it is not on disk.
async fn bytes_at(job: &Job, segments: &[Segment], at: u64, length: u64) -> Option<Vec<u8>> {
    if !job.by_range {
        return span_on_disk(&job.staged(), at, length).await;
    }
    let end = at.checked_add(length)?;
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|&i| segments[i].start);
    let mut bytes = Vec::with_capacity(usize::try_from(length).ok()?);
    for index in order {
        let segment = &segments[index];
        let (from, to) = (at.max(segment.start), end.min(segment.start + segment.done));
        if from >= to {
            continue;
        }
        // The ranges meet end to end; a gap before this one is bytes no range holds.
        if from != at + bytes.len() as u64 {
            return None;
        }
        let path = range_path(&job.parts, index);
        bytes.extend(span_on_disk(&path, from - segment.start, to - from).await?);
    }
    (bytes.len() as u64 == length).then_some(bytes)
}

/// The bytes on disk under each overlap slice, given as where it begins and the slice: `None` for
/// one not all there. Out of the staged file in one blocking task through one open, rather than an
/// open, a seek and a read for each slice, each of those a hop to the blocking pool while the
/// finished download waited: assembly before the file is placed went from 0.8 to 2.6 ms to 0.4 to
/// 1.0 on eight ranges (traced, 2026-11-11).
pub(super) async fn leads_on_disk(
    job: &Job,
    segments: &[Segment],
    leads: &[(u64, Vec<u8>)],
) -> Vec<Option<Vec<u8>>> {
    if job.by_range {
        let mut found = Vec::with_capacity(leads.len());
        for (at, lead) in leads {
            found.push(bytes_at(job, segments, *at, lead.len() as u64).await);
        }
        return found;
    }
    if leads.is_empty() {
        return Vec::new();
    }
    let spans: Vec<(u64, usize)> = leads.iter().map(|(at, lead)| (*at, lead.len())).collect();
    let staged = job.staged();
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek};
        let mut file = std::fs::File::open(staged).ok();
        spans
            .into_iter()
            .map(|(at, length)| {
                let file = file.as_mut()?;
                file.seek(std::io::SeekFrom::Start(at)).ok()?;
                let mut bytes = vec![0u8; length];
                file.read_exact(&mut bytes).ok()?;
                Some(bytes)
            })
            .collect()
    })
    .await
    .unwrap_or_else(|_| vec![None; leads.len()])
}

/// `length` bytes of the file at `path` from `at`.
async fn span_on_disk(path: &Path, at: u64, length: u64) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).await.ok()?;
    file.seek(std::io::SeekFrom::Start(at)).await.ok()?;
    let mut bytes = vec![0u8; usize::try_from(length).ok()?];
    tokio::io::AsyncReadExt::read_exact(&mut file, &mut bytes)
        .await
        .ok()?;
    Some(bytes)
}
