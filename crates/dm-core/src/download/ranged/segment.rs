// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What `fetch_segment` makes of its range's attempts: whether a refused or unanswered one
//! steps aside, how long one refused at a server's pace waits, and what the range's outcome
//! comes to.

use super::*;

/// Whether the range steps aside, where an attempt failed with `e`, for the connections the
/// server is serving: `at_start` is where the whole download and this range stood when the
/// attempt began, `on_disk` where the range stands now. Sets `turned_away` where it does.
pub(super) fn steps_aside(
    e: &Error,
    job: &Job,
    active: &AtomicUsize,
    turned_away: &AtomicBool,
    at_start: &(AtomicU64, AtomicU64),
    on_disk: &impl Fn() -> u64,
) -> bool {
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
    let others =
        active.load(Ordering::SeqCst) > 1 && (!job.limits.limited() || job.together.flowing() > 0);
    let stepping_aside = (refused || unanswered) && others;
    if stepping_aside {
        turned_away.store(true, Ordering::SeqCst);
        job.limits.found();
    }
    stepping_aside
}

/// The wait before the range is asked again where `e` refused it at a server's pace, rather
/// than the backoff's.
pub(super) fn paced_wait(e: &Error, job: &Job) -> Option<Duration> {
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
}

/// What the range's `outcome` comes to: a stop is passed on, a range that came short is left
/// to assembly, and a whole one's seams are asked for again.
pub(super) async fn settled(
    job: &Job,
    index: usize,
    resumable: bool,
    outcome: Result<Option<(usize, Carried)>, Error>,
) -> Result<Option<(usize, Carried)>, Error> {
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
