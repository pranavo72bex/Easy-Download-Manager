// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! When a connection beyond the planned ranges is worth opening: a helper
//! waits, watches how fast the others bring bytes in, and joins only if the
//! file is slow enough to be helped.

use super::*;

/// How long a helper connection waits before it asks whether the first ones
/// need it, and the window over which it then watches how fast bytes arrive.
pub(in crate::download) const HELP_AFTER: Duration = Duration::from_millis(100);

pub(in crate::download) const HELP_WATCH: Duration = Duration::from_millis(80);

/// How much of the file, at the rate seen, is worth a helper's looking for a
/// range to halve. Whether it then opens a connection is the range's to say
/// (`Job::split_worth`: a connection costs a round trip before its first byte,
/// so one opened for what the others finish sooner only slows the end), so
/// this is no longer that guard. At 0.3 s, a 2 MiB file fetched at 2 MiB/s a
/// connection was seen with 0.29 to 0.34 s left when its helpers looked, and was
/// helped or not by how its bytes fell into the 80 ms watched: 0.32 s with help,
/// 0.52 s without (`bench` batch, macOS, 2026-11-11).
pub(in crate::download) const HELP_WORTH_SECS: f64 = 0.2;

/// How long a helper waits for the first bytes to show a rate. A link that
/// has sent nothing for so long is slow enough to be helped.
pub(in crate::download) const HELP_GIVE_UP: Duration = Duration::from_millis(1500);

/// Whether a connection beyond the planned ranges is worth opening now: the
/// rate bytes are arriving at, watched over a short window (not from the
/// start, which counts the wait for the first byte as slowness), leaves more
/// than [`HELP_WORTH_SECS`] of the file still to come.
pub(in crate::download) async fn slow_enough_to_help(
    job: &Job,
    started: tokio::time::Instant,
) -> bool {
    // With a limit of dm's own set, the limit is the bottleneck and another
    // connection would only share it out again.
    if job.limiter.is_limited() {
        return false;
    }
    let Some(size) = job.size else {
        return false;
    };
    // Nothing left to come: not worth a window's watching to find out.
    if job.downloaded.load(Ordering::Relaxed) >= size {
        return false;
    }
    let mut from = (
        tokio::time::Instant::now(),
        job.downloaded.load(Ordering::Relaxed),
    );
    loop {
        tokio::time::sleep(HELP_WATCH).await;
        let now = (
            tokio::time::Instant::now(),
            job.downloaded.load(Ordering::Relaxed),
        );
        let left = size.saturating_sub(now.1) as f64;
        if left == 0.0 {
            return false;
        }
        if now.1 > from.1 {
            let rate = (now.1 - from.1) as f64 / now.0.duration_since(from.0).as_secs_f64();
            return left / rate > HELP_WORTH_SECS;
        }
        // Nothing came in this window: looked at again, until it is plainly
        // slow.
        if now.0.duration_since(started) >= HELP_GIVE_UP {
            return true;
        }
        from = now;
    }
}

/// What [`grow`] has decided for the connections past `OPENED_AT_ONCE`: how many of them may
/// work now, and whether it has done deciding (those still waiting are then not needed).
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::download) struct Gate {
    pub(in crate::download) allowed: usize,
    pub(in crate::download) closed: bool,
}

/// How long the first connections run before the rate they bring is measured.
const GROW_START: Duration = Duration::from_millis(250);

/// The window a rate is measured over.
const GROW_WINDOW: Duration = Duration::from_secs(1);

/// The longest wait for the added connections' first bytes before the rate is measured anyway:
/// a slow link takes some round trips to answer a new request.
const GROW_FIRST_BYTES: Duration = Duration::from_secs(3);

/// After the added connections' first bytes, before the rate is measured: a moment for them to
/// come up to speed.
const GROW_SETTLE: Duration = Duration::from_millis(250);

/// How much of what each connection brought on average an added connection must add to the
/// whole to be kept. A server that shares one rate among them all gives nearly nothing; one that
/// holds each connection down gives a whole connection's worth.
const GROW_SHARE: f64 = 0.5;

/// How much of the file must be left, at the rate seen, for another connection to be worth
/// opening and judging.
const GROW_WORTH_SECS: f64 = 2.0;

/// How long after connections were taken back more are tried again, doubling each time: a
/// server's or a link's limit can change, and one window can misjudge.
const GROW_AGAIN: Duration = Duration::from_secs(10);

/// Lets in the `grown` connections past `OPENED_AT_ONCE` only while they raise the rate, as
/// reference A treats its number of connections as a ceiling (gaps row 168): one, then twice as
/// many each time the last ones added each raised the total by [`GROW_SHARE`] of what a
/// connection brought; the last ones are taken back when they did not, and finish the pieces they
/// hold. Done once the connections opened at once are (`wave_over`), or too little is left.
pub(in crate::download) async fn grow(
    job: Arc<Job>,
    grown: usize,
    gate: tokio::sync::watch::Sender<Gate>,
    active: Arc<AtomicUsize>,
    mut wave_over: tokio::sync::watch::Receiver<bool>,
) {
    tokio::select! {
        () = growing(&job, grown, &gate, &active) => {}
        _ = wave_over.wait_for(|over| *over) => {}
    }
    gate.send_modify(|g| g.closed = true);
}

async fn growing(
    job: &Job,
    grown: usize,
    gate: &tokio::sync::watch::Sender<Gate>,
    active: &AtomicUsize,
) {
    let Some(size) = job.size else { return };
    tokio::time::sleep(GROW_START).await;
    let mut before = rate_over(job).await;
    let (mut allowed, mut batch, mut again) = (0usize, 1usize, GROW_AGAIN);
    loop {
        let left = size.saturating_sub(job.downloaded.load(Ordering::Relaxed)) as f64;
        if left == 0.0 || allowed >= grown {
            return;
        }
        // Nothing arrived over the window (a link gone quiet): looked at again.
        if before <= 0.0 {
            before = rate_over(job).await;
            continue;
        }
        if left / before < GROW_WORTH_SECS {
            return;
        }
        let at_work = active.load(Ordering::SeqCst).max(1);
        let added = batch.min(grown - allowed);
        let flowing = job.together.flowing();
        allowed += added;
        gate.send_modify(|g| g.allowed = allowed);
        let asked = tokio::time::Instant::now();
        while job.together.flowing() < flowing + added && asked.elapsed() < GROW_FIRST_BYTES {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(GROW_SETTLE).await;
        let after = rate_over(job).await;
        if raised(before, after, at_work, added) {
            before = after;
            batch *= 2;
            continue;
        }
        allowed -= added;
        gate.send_modify(|g| g.allowed = allowed);
        batch = 1;
        tokio::time::sleep(again).await;
        again *= 2;
        before = rate_over(job).await;
    }
}

/// Whether `added` connections beside `at_work` raised the rate from `before` to `after` by
/// [`GROW_SHARE`] of what each connection brought before, each.
fn raised(before: f64, after: f64, at_work: usize, added: usize) -> bool {
    let each = before / at_work.max(1) as f64;
    after - before >= GROW_SHARE * each * added as f64
}

/// The rate bytes arrive at over the next [`GROW_WINDOW`], in bytes a second.
async fn rate_over(job: &Job) -> f64 {
    let (from, had) = (
        tokio::time::Instant::now(),
        job.downloaded.load(Ordering::Relaxed),
    );
    tokio::time::sleep(GROW_WINDOW).await;
    let got = job.downloaded.load(Ordering::Relaxed).saturating_sub(had);
    got as f64 / from.elapsed().as_secs_f64().max(f64::EPSILON)
}

/// How long a connection is watched before it is judged against the rest.
const STALL_WINDOW: Duration = Duration::from_secs(5);

/// One look at a connection: what it and the whole download had brought in
/// when the window opened.
pub(in crate::download) struct Window {
    opened: tokio::time::Instant,
    own: u64,
    all: u64,
}

impl Window {
    pub(in crate::download) fn open(own: u64, job: &Job) -> Window {
        Window {
            opened: tokio::time::Instant::now(),
            own,
            all: job.downloaded.load(Ordering::Relaxed),
        }
    }

    pub(in crate::download) fn ends(&self) -> tokio::time::Instant {
        self.opened + STALL_WINDOW
    }

    pub(in crate::download) fn over(&self) -> bool {
        tokio::time::Instant::now() >= self.ends()
    }

    /// Whether this connection fell far behind the others over the window
    /// ([`behind`]). Never under a limit of dm's own, which holds every
    /// connection back alike.
    pub(in crate::download) fn stalled(&self, own: u64, job: &Job) -> bool {
        if job.limiter.is_limited() {
            return false;
        }
        let all = job
            .downloaded
            .load(Ordering::Relaxed)
            .saturating_sub(self.all);
        let mine = own.saturating_sub(self.own);
        behind(all, mine, fetching(job))
    }
}

/// How many ranges have a connection on them with bytes still to come.
fn fetching(job: &Job) -> usize {
    job.live
        .iter()
        .filter(|slot| {
            let l = crate::lock(slot);
            l.taken && (l.end - l.start + 1) > l.reserved
        })
        .count()
}

/// The share of the other connections' average rate under which a connection
/// is dropped and asked again (reference A's watchdog: under 0.15 of its
/// peers').
const BEHIND_SHARE: f64 = 0.15;

/// Whether a connection that brought in `mine` of the `all` bytes the
/// download got over the window, beside `connections` fetching in all, is far
/// enough behind to be dropped: while the whole was a real flow (over 1 MiB),
/// under [`BEHIND_SHARE`] of the others' average, or under a sixty-fourth of
/// the whole. Never alone: a download that is all slow is the link's, and the
/// read timeout is its judge. Measured against the others' average rather
/// than the whole, a slow one beside one or two fast ones is caught: a
/// sixty-fourth of the whole let one at a twentieth of its one peer's rate
/// hold the end of a two-connection download.
fn behind(all: u64, mine: u64, connections: usize) -> bool {
    if all <= 1024 * 1024 {
        return false;
    }
    if mine < all / 64 {
        return true;
    }
    let others = all.saturating_sub(mine);
    let peers = connections.saturating_sub(1);
    peers > 0 && (mine as f64) < BEHIND_SHARE * (others as f64 / peers as f64)
}

#[cfg(test)]
mod tests {
    use super::{behind, raised};

    const MB: u64 = 1024 * 1024;

    #[test]
    fn a_connection_is_kept_only_where_it_raised_the_whole() {
        // Each held to 1 MiB/s: eight bring 8, nine 9.
        assert!(raised(8.0, 9.0, 8, 1));
        // Then two more, four more.
        assert!(raised(9.0, 11.0, 9, 2));
        assert!(raised(11.0, 15.0, 11, 4));
        // One rate shared among them all: the ninth takes from the others.
        assert!(!raised(8.0, 8.1, 8, 1));
        assert!(!raised(8.0, 7.6, 8, 1));
        // Two added that brought one connection's worth between them.
        assert!(!raised(8.0, 8.9, 8, 2));
    }

    #[test]
    fn a_connection_far_behind_its_peers_is_dropped_however_many_there_are() {
        // Two connections, one at a twentieth of the other: a sixty-fourth
        // of the whole did not catch it.
        assert!(behind(21 * MB, MB, 2));
        // Eight, one at a tenth of the others' average.
        assert!(behind(71 * MB, MB, 8));
        // Nothing at all from it.
        assert!(behind(10 * MB, 0, 4));
    }

    #[test]
    fn a_connection_near_its_peers_or_alone_or_in_a_trickle_is_kept() {
        // A fifth of its one peer: slower, not stalled.
        assert!(!behind(6 * MB, MB, 2));
        // Alone, however slow: the whole is its own, the link's to judge.
        assert!(!behind(2 * MB, 2 * MB, 1));
        // Its peers just finished, so it is the only one fetching now.
        assert!(!behind(4 * MB, MB, 1));
        // Under a mebibyte over the whole window: too little to judge by.
        assert!(!behind(MB, 0, 8));
        // Every connection alike.
        assert!(!behind(8 * MB, MB, 8));
    }
}
