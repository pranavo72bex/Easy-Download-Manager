// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
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
        // What the server named it, for the trace (gaps row 206).
        let named = |name: reqwest::header::HeaderName| {
            res.headers()
                .get(name)
                .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
                .unwrap_or("none")
                .to_owned()
        };
        crate::finish_trace::mark(&format!(
            "seam {low}-{high} of range {index}: another version (asked under {}, answered ETag {}, Last-Modified {})",
            conditional.map_or("none", String::as_str),
            named(reqwest::header::ETAG),
            named(reqwest::header::LAST_MODIFIED),
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
    // No more than was asked for, whatever the server says of its length (gaps row H14).
    let Ok(most) = usize::try_from(wanted) else {
        return unproven;
    };
    let Ok(Ok(there)) = tokio::time::timeout(SPAN_WITHIN, edm_net::body::at_most(res, most)).await
    else {
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
        /* What was compared, for the trace alone (gaps row 206: a refetch of the whole file
         * seen once in 31 `fast` runs, with no trace of it): the ranges under the span as they
         * stood when it was claimed, how the bytes differ, and the disk read again a moment
         * later, which tells bytes not yet written when it was read (the server's by then) from
         * bytes written wrong (still other). The finding is the same either way. */
        if crate::finish_trace::on() {
            tokio::time::sleep(REREAD_AFTER).await;
            let again = match bytes_at(job, segments, low, wanted).await {
                Some(later) if later[..] == there[..] => "the server's bytes",
                Some(later) if later[..] == here[..] => "as first read",
                Some(_) => "other bytes again",
                None => "not readable",
            };
            crate::finish_trace::mark(&format!(
                "seam {low}-{high}: {}; ranges {}; on disk {} ms later: {again}",
                difference(&there, &here),
                ranges_under(segments, low, high),
                REREAD_AFTER.as_millis()
            ));
        }
        Look::Differs
    }
}

/// How long after a span is found to differ the disk is read again, for the trace.
const REREAD_AFTER: std::time::Duration = std::time::Duration::from_millis(200);

/// The farthest a shift of the bytes is looked for (`difference`).
const SHIFT_MOST: usize = 256;

/// How `here` (on disk) differs from `there` (the server's, for the same span), for the trace: how
/// many bytes differ, the first and the last; whether the disk is all zeros from the first (bytes
/// not written there); whether it holds the server's bytes from a little further on or back
/// (`shifted by k`: the disk's byte `i` is the server's `i + k`, bytes written at the wrong
/// place); and up to 16 bytes of each from the first difference.
pub(super) fn difference(there: &[u8], here: &[u8]) -> String {
    let length = there.len().min(here.len());
    let differing: Vec<usize> = (0..length).filter(|&i| there[i] != here[i]).collect();
    let (Some(&first), Some(&last)) = (differing.first(), differing.last()) else {
        return if there.len() == here.len() {
            "the same".into()
        } else {
            format!("{} bytes against {}", there.len(), here.len())
        };
    };
    let mut said = format!("{} bytes differ, {first} to {last}", differing.len());
    if here[first..length].iter().all(|&b| b == 0) {
        said.push_str("; the disk is zeros from the first");
    } else if there[first..length].iter().all(|&b| b == 0) {
        said.push_str("; the server's are zeros from the first");
    }
    if let Some(shift) = shifted(there, here, first) {
        said.push_str(&format!("; the disk is the server's shifted by {shift:+}"));
    }
    let hex =
        |bytes: &[u8]| -> String { bytes.iter().take(16).map(|b| format!("{b:02x}")).collect() };
    said.push_str(&format!(
        "; server {} disk {}",
        hex(&there[first..length]),
        hex(&here[first..length])
    ));
    said
}

/// The shift `k`, within [`SHIFT_MOST`] either way, for which the disk's bytes from `first` on are
/// the server's from `first + k` on, over at least 16 of them.
fn shifted(there: &[u8], here: &[u8], first: usize) -> Option<i64> {
    let length = there.len().min(here.len());
    let most = SHIFT_MOST.min(length) as i64;
    (1..=most).flat_map(|k| [k, -k]).find(|&k| {
        let pairs: Vec<(usize, usize)> = (first..length)
            .filter_map(|i| {
                let j = i as i64 + k;
                (j >= 0 && (j as usize) < there.len()).then_some((i, j as usize))
            })
            .collect();
        pairs.len() >= 16 && pairs.iter().all(|&(i, j)| here[i] == there[j])
    })
}

/// The ranges under `low..=high` as `segments` has them, for the trace: each as its index, its
/// first and last byte, and how much of it is on disk.
pub(super) fn ranges_under(segments: &[Segment], low: u64, high: u64) -> String {
    let under: Vec<String> = segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.start <= high && s.end >= low)
        .map(|(i, s)| format!("{i} {}-{} done {}", s.start, s.end, s.done))
        .collect();
    if under.is_empty() {
        "none".into()
    } else {
        under.join(", ")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The trace of a span that differs says how (gaps row 206): zeros on disk where nothing was
    /// written yet, the server's bytes at another place, or other bytes.
    #[test]
    fn a_span_that_differs_is_described_for_the_trace() {
        let server: Vec<u8> = (0..1024u32).map(|i| (i * 7 + i / 251) as u8 | 1).collect();

        let same = difference(&server, &server);
        assert_eq!(same, "the same");

        // Nothing on disk past byte 600: zeros, never written.
        let mut unwritten = server.clone();
        unwritten[600..].fill(0);
        let said = difference(&server, &unwritten);
        assert!(said.starts_with("424 bytes differ, 600 to 1023"), "{said}");
        assert!(said.contains("the disk is zeros from the first"), "{said}");
        assert!(!said.contains("shifted"), "{said}");

        // From byte 512 on, the disk holds the server's bytes from 64 further on.
        let mut ahead = server.clone();
        ahead[512..960].copy_from_slice(&server[576..1024]);
        let said = difference(&server, &ahead);
        assert!(said.contains("shifted by +64"), "{said}");
        assert!(!said.contains("zeros"), "{said}");
        // And from 64 back.
        let mut behind = server.clone();
        behind[512..].copy_from_slice(&server[448..960]);
        let said = difference(&server, &behind);
        assert!(said.contains("shifted by -64"), "{said}");

        // One byte other: where, and both sides of it.
        let mut one = server.clone();
        one[100] ^= 0xff;
        let said = difference(&server, &one);
        assert!(said.starts_with("1 bytes differ, 100 to 100"), "{said}");
        let hex = |b: u8| format!("{b:02x}");
        assert!(
            said.contains(&format!("server {}", hex(server[100]))),
            "{said}"
        );
        assert!(said.contains(&format!("disk {}", hex(one[100]))), "{said}");
    }

    /// The ranges under a span, as the trace names them.
    #[test]
    fn the_ranges_under_a_span_are_named() {
        let segments = vec![
            Segment {
                start: 0,
                end: 999,
                done: 1000,
            },
            Segment {
                start: 2000,
                end: 2999,
                done: 10,
            },
            Segment {
                start: 1000,
                end: 1999,
                done: 1000,
            },
        ];
        assert_eq!(
            ranges_under(&segments, 488, 1511),
            "0 0-999 done 1000, 2 1000-1999 done 1000"
        );
        assert_eq!(ranges_under(&segments, 5000, 5100), "none");
    }
}
