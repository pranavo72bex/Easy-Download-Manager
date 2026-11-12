// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A server that limits how many requests it serves at once, how often it takes a new one, or
//! both (nginx's `limit_conn` and `limit_req`, as Hetzner's mirrors answer): what it has shown of
//! that in this download, and how the connections keep to it.
//!
//! Before, each turned-away connection gave up for good, the next range's request at each range's
//! end was refused and waited out with backoff while nothing was fetched, the seams' checks went
//! out beside it and took the request the server would have taken, and a range halved for a
//! request the server then refused left its back half to no one. On a local copy of such a server
//! (two at once, a request a second) dm took 1.6 times as long as one connection, and on
//! Hetzner's mirror four times (gaps row 197). Once the server has turned a request away while it
//! served others:
//!
//! - no more connections are at work than the most it has served at once; the others wait for a
//!   place rather than end ([`rejoin`]);
//! - a request goes no sooner than the server's pace allows after a refusal ([`Limits::ready`]);
//! - an answer that brings a range whole goes on into the next range where nobody has that one,
//!   as far as the request asked ([`continue_into`]), and a request asks as far as the ranges
//!   nobody has run on after its own, so one request fetches what took one a range before;
//! - a connection looking for work takes the ranges nobody fetches, or the back half of the work
//!   of one under way, its own range and the ranges its answer will go on into together
//!   ([`pick_limited`]): the half refused is then still that answer's to fetch;
//! - the seams are checked once nothing else is being asked for, one at a time, and no more
//!   once one check is refused (`fetch_segment`, `run`).

use super::*;

/// How long after a refusal the next request waits where the server named no wait: half a
/// second at first, half again as long each time a request that waited was refused too, up to
/// [`PACE_MOST`]. A server that limits requests to one a second (nginx's usual) takes one after
/// the second wait; one that limits only connections never makes it grow, as dm keeps to them.
pub(in crate::download) const PACE_FIRST: Duration = Duration::from_millis(500);

/// The longest a refusal holds the next request back, where the server named no wait: a server
/// that keeps refusing is asked about four times a minute by the whole download, not by each
/// connection.
pub(in crate::download) const PACE_MOST: Duration = Duration::from_secs(4);

/// The least wait taken from a server's `Retry-After`: "0" is not asked again in a loop.
pub(in crate::download) const PACE_LEAST: Duration = Duration::from_millis(100);

/// The longest an answer that brought its range whole waits, at the range's end, on another
/// connection's request for the next range, before letting go: asked for a halving, that request
/// may yet be refused, and the answer then goes on into it (`continue_into`).
const BOUNDARY_WAIT: Duration = Duration::from_secs(1);

/// What a download has seen of its server's limits.
#[derive(Debug, Default)]
pub(in crate::download) struct Limits {
    /// Set once the server turned a request of this download away at once (429 or 503), or
    /// left it unanswered, while it was serving others.
    limited: std::sync::atomic::AtomicBool,
    pace: std::sync::Mutex<Pace>,
    /// Ranges whose seams are to be checked once no range is asked for (`run`).
    due: std::sync::Mutex<Vec<usize>>,
    /// The download's requests at the server now, from their sending to their end ([`Out`]).
    out: Arc<AtomicUsize>,
}

/// One request of the download at its server, counted in [`Limits`] until it is dropped. Its
/// answer read on into the next range carries it along (`Carried`): the request is still out.
pub(in crate::download) struct Out {
    out: Arc<AtomicUsize>,
    /// How many of the download's requests were at the server before this one went.
    pub(in crate::download) beside: usize,
}

impl Out {
    /// A request going out now.
    pub(in crate::download) fn sent(limits: &Limits) -> Out {
        let beside = limits.out.fetch_add(1, Ordering::SeqCst);
        Out {
            out: Arc::clone(&limits.out),
            beside,
        }
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        self.out.fetch_sub(1, Ordering::SeqCst);
    }
}

/// When the next request may go.
#[derive(Debug)]
struct Pace {
    /// No request before this.
    next: Option<tokio::time::Instant>,
    /// When the last refusal set `next`: a request sent after it waited, and one refused then
    /// lengthens the wait.
    set_at: Option<tokio::time::Instant>,
    /// The wait after a refusal that named none.
    interval: Duration,
}

impl Default for Pace {
    fn default() -> Self {
        Pace {
            next: None,
            set_at: None,
            interval: PACE_FIRST,
        }
    }
}

impl Limits {
    /// Whether the server has turned a request away while serving others.
    pub(in crate::download) fn limited(&self) -> bool {
        self.limited.load(Ordering::SeqCst)
    }

    /// The server turned a request away while it served others.
    pub(in crate::download) fn found(&self) {
        self.limited.store(true, Ordering::SeqCst);
    }

    /// A request sent at `sent`, beside `beside` others of the download at the server, was
    /// refused, with the wait the server named if any. Where the server has served as many at
    /// once as were out beside it, it is turned away for their number, which `rejoin` keeps to,
    /// and nothing else need wait: a server limited to two at once took the next request the
    /// moment one ended, where a pace held every connection back 0.75 s. Otherwise it limits how
    /// often it takes a request, or named a wait: the next request waits for that, or the pace.
    pub(in crate::download) fn turned_away(
        &self,
        sent: tokio::time::Instant,
        beside: usize,
        most: usize,
        named: Option<Duration>,
    ) {
        if named.is_none() && beside >= most.max(1) {
            return;
        }
        let now = tokio::time::Instant::now();
        let mut pace = crate::lock(&self.pace);
        if pace.set_at.is_some_and(|set| sent >= set) {
            pace.interval = pace.interval.mul_f64(1.5).min(PACE_MOST);
        }
        pace.set_at = Some(now);
        let wait = named.map_or(pace.interval, |named| {
            named.clamp(PACE_LEAST, Duration::from_secs(60))
        });
        pace.next = Some(pace.next.map_or(now + wait, |next| next.max(now + wait)));
    }

    /// How long until a request may go.
    pub(in crate::download) fn wait_left(&self) -> Duration {
        crate::lock(&self.pace).next.map_or(Duration::ZERO, |next| {
            next.saturating_duration_since(tokio::time::Instant::now())
        })
    }

    /// Waits until a request may go, unless asked to stop first.
    pub(in crate::download) async fn ready(&self, control: &Control) -> Result<(), Error> {
        loop {
            control.check()?;
            let left = self.wait_left();
            if left.is_zero() {
                return Ok(());
            }
            tokio::time::sleep(left.min(Duration::from_millis(100))).await;
        }
    }

    /// Range `index` is whole: its seams are to be checked once nothing else is asked for.
    pub(in crate::download) fn check_later(&self, index: usize) {
        crate::lock(&self.due).push(index);
    }

    /// The ranges whose seams are still to be checked.
    pub(in crate::download) fn take_due(&self) -> Vec<usize> {
        std::mem::take(&mut *crate::lock(&self.due))
    }
}

/// Counts a connection a limiting server turned away back in among those at work, once fewer are
/// at work than the most the server has served at once: one at work ends (nothing left for it),
/// or more is found to be served. Woken as ranges end or connections let go.
pub(in crate::download) async fn rejoin(job: &Job, active: &AtomicUsize) -> Result<(), Error> {
    loop {
        job.control.check()?;
        let moved = job.moved.notified();
        tokio::pin!(moved);
        moved.as_mut().enable();
        let ceiling = job.together.most().max(1);
        let now = active.load(Ordering::SeqCst);
        if now < ceiling
            && active
                .compare_exchange(now, now + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            return Ok(());
        }
        tokio::select! {
            () = &mut moved => {}
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }
}

/// Where a request for range `index`, from `end`, its end, may ask to: past it, over the ranges
/// right after it that nobody has taken and nothing is on disk for, where the server limits
/// requests. The answer goes on into them as it comes to them (`continue_into`), so one request
/// fetches what took one a range; a connection that takes one of them meanwhile only stops it
/// there.
pub(in crate::download) async fn reach_from(job: &Job, index: usize, end: u64) -> u64 {
    if !job.limits.limited() || job.size.is_none() || !job.resumable || job.chunk.is_some() {
        return end;
    }
    let count = job.segments.lock().await.len();
    let mut reach = end;
    loop {
        let after = (0..count).filter(|&i| i != index).find(|&i| {
            let l = crate::lock(&job.live[i]);
            l.start == reach.saturating_add(1)
        });
        let Some(after) = after else {
            return reach;
        };
        let l = crate::lock(&job.live[after]);
        if l.taken || l.reserved > 0 {
            return reach;
        }
        reach = l.end;
    }
}

/// The range that begins at `at`, right after range `index`, taken by the answer that has just
/// brought range `index` whole and goes on to `reach`: where nobody has it and nothing is on disk
/// for it, the answer is read on into it, and nobody asks the server for it. Where another
/// connection has asked for it and has had no answer yet (a halving), it waits for that answer,
/// up to [`BOUNDARY_WAIT`]: refused, the range is given back and taken here, so a refused halving
/// leaves no hole; answered, this answer ends. None where it ends here.
pub(in crate::download) async fn continue_into(
    job: &Job,
    index: usize,
    at: u64,
    reach: u64,
) -> Option<usize> {
    if reach < at {
        return None;
    }
    let count = job.segments.lock().await.len();
    let next = (0..count)
        .filter(|&i| i != index)
        .find(|&i| crate::lock(&job.live[i]).start == at)?;
    let until = tokio::time::Instant::now() + BOUNDARY_WAIT;
    loop {
        let moved = job.moved.notified();
        tokio::pin!(moved);
        moved.as_mut().enable();
        {
            let mut l = crate::lock(&job.live[next]);
            if !l.taken && l.reserved == 0 && l.end >= l.start {
                l.taken = true;
                l.trusted = true;
                l.reach = reach;
                l.going = Going {
                    since: Some(tokio::time::Instant::now()),
                    ..Going::default()
                };
                // One answer across the seam: nothing to check there.
                if let Some(checked) = job.seams_checked.get(next) {
                    checked.store(true, Ordering::SeqCst);
                }
                return Some(next);
            }
            let waiting = l.taken && l.reserved == 0 && l.going.latest.is_none();
            if !waiting {
                return None;
            }
        }
        let now = tokio::time::Instant::now();
        if now >= until || job.control.check().is_err() {
            return None;
        }
        tokio::select! {
            () = &mut moved => {}
            () = tokio::time::sleep_until(until.min(now + Duration::from_millis(20))) => {}
        }
    }
}

/// One range as the picker sees it.
#[derive(Clone, Copy)]
struct Seen {
    index: usize,
    live: Live,
}

impl Seen {
    fn free(&self) -> bool {
        !self.live.taken && self.live.left() > 0
    }
}

/// What a connection at work on a download from a limiting server does next, once the server's
/// pace allows a request: the first range of the longest run of ranges nobody fetches, nor will,
/// with the request asked as far as the run goes; else the back half of the work of the range
/// that would take longest, its own bytes and the ranges its answer will go on into together.
/// The half is cut where it falls: in that range itself (halved, as `split_largest` does), or in
/// one of the ranges after it (that one taken, or cut where the half falls, so the answer stops
/// there). Only says whether anything is left to halve when not `judging`.
pub(in crate::download) async fn pick_limited(job: &Job, judging: bool) -> Halving {
    let mut segments = job.segments.lock().await;
    let count = segments.len();
    let mut seen: Vec<Seen> = (0..count)
        .map(|index| Seen {
            index,
            live: *crate::lock(&job.live[index]),
        })
        .collect();
    seen.sort_by_key(|s| s.live.start);

    /* Runs of free ranges, each with the range before it if that one's answer goes on into the
     * run: a free range something is on disk for is a run of its own, asked from where it got
     * to, as no answer goes on into it. */
    struct Run {
        from: usize,
        to: usize,
        feeder: Option<usize>,
        bytes: u64,
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut at = 0;
    while at < seen.len() {
        if !seen[at].free() {
            at += 1;
            continue;
        }
        let from = at;
        let mut bytes = seen[at].live.left();
        while at + 1 < seen.len()
            && seen[at].live.reserved == 0
            && seen[at + 1].free()
            && seen[at + 1].live.reserved == 0
            && seen[at].live.end.checked_add(1) == Some(seen[at + 1].live.start)
        {
            at += 1;
            bytes += seen[at].live.left();
        }
        let feeder = from.checked_sub(1).filter(|&before| {
            let f = &seen[before].live;
            f.taken
                && f.left() > 0
                && seen[from].live.reserved == 0
                && f.end.checked_add(1) == Some(seen[from].live.start)
                && f.reach >= seen[from].live.start
        });
        runs.push(Run {
            from,
            to: at,
            feeder,
            bytes,
        });
        at += 1;
    }

    // Nobody's: the longest such run, from its front.
    if let Some(run) = runs
        .iter()
        .filter(|run| run.feeder.is_none())
        .max_by_key(|run| run.bytes)
    {
        let index = seen[run.from].index;
        let mut l = crate::lock(&job.live[index]);
        if !l.taken {
            l.taken = true;
            return Halving::Range(index);
        }
        // Taken meanwhile (an answer went on into it): looked at again.
        return Halving::Later;
    }

    // The range whose work, with the run its answer goes on into, would take longest.
    let now = tokio::time::Instant::now();
    let worth = job.split_worth().as_secs_f64();
    let mut halvable = false;
    let mut slowest: Option<(usize, u64, f64)> = None;
    for (at, s) in seen.iter().enumerate() {
        let l = &s.live;
        if !l.taken || l.left() == 0 {
            continue;
        }
        let run = runs.iter().find(|run| run.feeder == Some(at));
        let work = l.left() + run.map_or(0, |run| run.bytes);
        if work < 2 * MIN_SPLIT {
            continue;
        }
        halvable = true;
        if !judging {
            break;
        }
        let time = match l.time_left(now) {
            Some(time) if time.is_infinite() => time,
            Some(_) => l
                .going
                .pace
                .map_or(f64::INFINITY, |pace| work as f64 / pace),
            None => continue,
        };
        if time > worth && slowest.is_none_or(|(_, _, most)| time > most) {
            slowest = Some((at, work, time));
        }
    }
    let Some((victim_at, work, _)) = slowest else {
        return if halvable {
            Halving::Later
        } else {
            Halving::Nothing
        };
    };
    let victim = seen[victim_at];
    let (cursor, left) = (victim.live.start + victim.live.reserved, victim.live.left());
    /* Halved so both end together: the new request brings nothing until its answer comes, while
     * the range under way goes on, so that one keeps what it fetches meanwhile on top of its half.
     * Cut at the middle, the new half ended a round trip after the other (0.1 s of 4.3 on the
     * test's server, which answers 100 ms late). */
    let waited = Duration::from_micros(job.first_byte_us.load(Ordering::Relaxed)).as_secs_f64();
    let meanwhile = victim.live.going.pace.map_or(0.0, |pace| pace * waited) as u64;
    let half = cursor + (work + meanwhile.min(work)) / 2;
    // In the range itself: halved there, as `split_largest` does.
    if half <= victim.live.end && left >= 2 * MIN_SPLIT {
        let at = half.clamp(cursor + MIN_SPLIT, victim.live.end + 1 - MIN_SPLIT);
        return match cut(job, &mut segments, victim.index, at).await {
            Some(index) => Halving::Range(index),
            None => Halving::Later,
        };
    }
    // In the run after it: the range the half falls in, taken whole or cut where it falls.
    let half = half.max(victim.live.end + 1);
    let Some(run) = runs.iter().find(|run| run.feeder == Some(victim_at)) else {
        return Halving::Later;
    };
    let within = &seen[run.from..=run.to];
    let Some(at) = within.iter().position(|s| s.live.end >= half) else {
        return Halving::Later;
    };
    let r = within[at];
    let whole = if half < r.live.start + MIN_SPLIT {
        Some(r)
    } else if r.live.end + 1 < half + MIN_SPLIT {
        // The half falls at its end: the next one, if the run goes on.
        Some(within.get(at + 1).copied().unwrap_or(r))
    } else {
        None
    };
    if let Some(whole) = whole {
        let mut l = crate::lock(&job.live[whole.index]);
        if !l.taken && l.reserved == 0 {
            l.taken = true;
            return Halving::Range(whole.index);
        }
        return Halving::Later;
    }
    match cut(job, &mut segments, r.index, half).await {
        Some(index) => Halving::Range(index),
        None => Halving::Later,
    }
}

/// Cuts range `victim` at byte `at`: from now on it ends at `at - 1`, and a new range from `at` to
/// its old end is taken by the connection asking, which it returns. The victim may be under way
/// (its connection stops at the new end) or nobody's yet (whoever fetches it does). The plan
/// beside the parts is written first, under the segments lock, so a resume puts every part where
/// it was fetched for; where it cannot be written, nothing changes. None also where the victim
/// has meanwhile got to within [`MIN_SPLIT`] of `at`, or there is no room for another range.
pub(in crate::download) async fn cut(
    job: &Job,
    segments: &mut Vec<Segment>,
    victim: usize,
    at: u64,
) -> Option<usize> {
    let index = segments.len();
    if index >= job.live.len() || !job.resumable {
        return None;
    }
    let old_end = {
        let mut l = crate::lock(&job.live[victim]);
        if l.start + l.reserved + MIN_SPLIT > at || at + MIN_SPLIT > l.end + 1 {
            return None;
        }
        let old_end = l.end;
        l.end = at - 1;
        old_end
    };
    let mut planned = segments.clone();
    planned[victim].end = at - 1;
    planned.push(Segment {
        start: at,
        end: old_end,
        done: 0,
    });
    let (dir, size, resumable, validator) = (
        job.parts.clone(),
        job.size,
        job.resumable,
        job.validator.clone(),
    );
    let recorded = tokio::task::spawn_blocking(move || {
        plan::record(&dir, size, resumable, validator.as_deref(), &planned)
    })
    .await;
    if !matches!(recorded, Ok(Ok(()))) {
        crate::lock(&job.live[victim]).end = old_end;
        return None;
    }
    segments[victim].end = at - 1;
    segments.push(Segment {
        start: at,
        end: old_end,
        done: 0,
    });
    let _ = fs::remove_file(ledger_path(&job.parts, index)).await;
    forget_synced(&job.parts, index).await;
    *crate::lock(&job.live[index]) = Live {
        start: at,
        end: old_end,
        reserved: 0,
        taken: true,
        trusted: true,
        reach: 0,
        going: Going {
            since: Some(tokio::time::Instant::now()),
            ..Going::default()
        },
    };
    Some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `left` is `wait`, give or take the moments the test took.
    fn about(left: Duration, wait: Duration) -> bool {
        left <= wait && left + Duration::from_millis(50) >= wait
    }

    /// A refusal for want of a place among the requests the server serves at once holds no
    /// request back: the next goes as soon as one of them ends. One with fewer out than that
    /// (a limit on how often it takes one) holds the next back half a second, longer each time
    /// a request that waited is refused too, never past `PACE_MOST`; a wait the server names is
    /// kept to, within reason.
    #[tokio::test]
    async fn only_a_limit_on_how_often_holds_the_next_request_back() {
        let limits = Limits::default();
        let now = tokio::time::Instant::now();
        // Two served at once, and a third turned away: a matter of places.
        limits.turned_away(now, 2, 2, None);
        assert!(limits.wait_left().is_zero());
        // One out beside it where two have been served: a matter of pace.
        limits.turned_away(now, 1, 2, None);
        assert!(
            about(limits.wait_left(), PACE_FIRST),
            "{:?}",
            limits.wait_left()
        );
        // Asked again once the wait was set, and refused again: longer.
        limits.turned_away(tokio::time::Instant::now(), 1, 2, None);
        let longer = PACE_FIRST.mul_f64(1.5);
        assert!(
            about(limits.wait_left(), longer),
            "{:?}",
            limits.wait_left()
        );
        for _ in 0..20 {
            limits.turned_away(tokio::time::Instant::now(), 1, 2, None);
        }
        assert!(
            about(limits.wait_left(), PACE_MOST),
            "{:?}",
            limits.wait_left()
        );
        // A wait the server names, at least `PACE_LEAST` and at most a minute.
        let named = Limits::default();
        named.turned_away(now, 2, 2, Some(Duration::ZERO));
        assert!(
            about(named.wait_left(), PACE_LEAST),
            "{:?}",
            named.wait_left()
        );
        named.turned_away(now, 2, 2, Some(Duration::from_secs(3600)));
        let minute = Duration::from_secs(60);
        assert!(about(named.wait_left(), minute), "{:?}", named.wait_left());
    }

    /// Refusals of requests that went out together (the first ones of a download, before
    /// anything was known) lengthen nothing: only a request sent after a refusal set the wait,
    /// and refused in its turn, does.
    #[tokio::test]
    async fn refusals_of_requests_sent_together_do_not_lengthen_the_wait() {
        let limits = Limits::default();
        let sent = tokio::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(5)).await;
        for _ in 0..6 {
            limits.turned_away(sent, 1, 3, None);
        }
        assert!(
            about(limits.wait_left(), PACE_FIRST),
            "{:?}",
            limits.wait_left()
        );
    }
}
