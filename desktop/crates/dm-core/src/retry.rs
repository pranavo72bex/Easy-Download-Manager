// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! When to try again, and how long to wait first.
//!
//! Retrying is cheap here in a way it is not for most clients: a segment that
//! fails resumes from what it already has on disk, so a second attempt asks
//! only for the bytes the first one did not get. That makes the interesting
//! question not "can we afford to retry" but "is this worth retrying at all"
//! — a 404 answered instantly forever is not a download in progress.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::control::Control;
use crate::Error;

/// After this many attempts a segment is failing for a reason waiting will
/// not fix.
pub const MAX_ATTEMPTS: u32 = 5;

/// Whether trying the same thing again could plausibly work.
///
/// Every 5xx except 501, plus the three 4xx that mean "later". A shorter hand-written list drops 425, 507,
/// 509 and Cloudflare's 52x, and a server having a bad day is not an answer.
pub fn transient(error: &Error) -> bool {
    match error {
        Error::Refused { status, .. } => {
            matches!(status, 408 | 425 | 429) || (*status >= 500 && *status != 501)
        }
        // A connection that dropped or timed out says nothing about whether
        // the file is there. A body cut off mid-stream arrives as a body or
        // decode error, not a request error, and was the commonest drop of
        // all: one reset on one range failed the whole download.
        Error::Network(e) => {
            !unreadable_response(e)
                && !untrusted_certificate(e)
                && (e.is_timeout()
                    || e.is_connect()
                    || e.is_request()
                    || e.is_body()
                    || e.is_decode())
        }
        // A connection that stopped short deserves another go.
        Error::Incomplete { .. } => true,
        Error::Connection(_) => true,
        // Stopping was asked for, not suffered.
        Error::Paused | Error::Cancelled => false,
        // Disk errors, a changed file, a dishonest server: all answers.
        _ => false,
    }
}

/// Whether the server's answer could not be read as HTTP at all (headers
/// beyond any limit, a malformed status line or header): asking again gets the
/// same bytes, so waiting between tries only held a failure back for minutes.
fn unreadable_response(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(e) = source {
        let said = e.to_string().to_ascii_lowercase();
        if said.contains("message head is too large")
            || said.contains("invalid http")
            || said.contains("invalid header")
            || said.contains("invalid status")
        {
            return true;
        }
        source = e.source();
    }
    false
}

/// Whether the connection was refused over the server's certificate (expired,
/// for another name, from an issuer nobody trusts): it arrives as a failed
/// connection, but asking again meets the same certificate, and two minutes
/// of retries only held back the reason (BUGLOG.md, BUG-002).
fn untrusted_certificate(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(e) = source {
        if e.to_string().to_ascii_lowercase().contains("certificate") {
            return true;
        }
        source = e.source();
    }
    false
}

/// How long to wait before attempt `attempt` (zero-based).
///
/// Exponential, capped, and jittered by the attempt number, so successive
/// waits differ. The jitter is not per connection: connections that fail
/// together at the same attempt wait the same time.
pub fn backoff(attempt: u32, retry_after: Option<Duration>) -> Duration {
    // A server that said how long to wait is believed, within reason — an
    // hour-long Retry-After is not something to sit in. Believed as the least
    // wait, not the whole of it: Wikimedia's limiter answered every request
    // "429, Retry-After: 1" for as long as they kept coming a second apart,
    // so dm, asking each second, sat at 0 B for minutes; the wait still grows.
    // Past the fifteen seconds a dropped connection is held to: that
    // limiter let a request through only after about twenty quiet seconds.
    if let Some(asked) = retry_after {
        let grown = Duration::from_millis(500 * 2u64.saturating_pow(attempt).min(128))
            + Duration::from_millis(u64::from(attempt) * 137 % 400);
        return asked.max(grown).min(Duration::from_secs(60));
    }

    let base = Duration::from_millis(500 * 2u64.saturating_pow(attempt).min(64));
    // Quick at first, for a dropped connection; then fifteen seconds apart,
    // so a server or a network that comes back is found within that (the
    // other macOS manager resumed 21 s after a server returned, dm took 32
    // at half a minute apart), while the fifteen attempts a person has by
    // default still outlast a router restarting or Wi-Fi coming back: five
    // quick ones were spent in twenty seconds and the download failed for
    // good.
    let capped = base.min(Duration::from_secs(if attempt < 5 { 8 } else { 15 }));

    // Deterministic jitter from the attempt number: no RNG dependency.
    let jitter = Duration::from_millis(u64::from(attempt) * 137 % 400);
    capped + jitter
}

/// Runs `op` until it succeeds, gives an answer, or runs out of attempts.
///
/// One place, because the probe needs this as much as the ranges do — a
/// server that is briefly busy would otherwise fail the download before it
/// started, which is the hole the first version of this shipped with.
///
/// The wait between attempts belongs to `control` as much as the attempts
/// do: a cancelled download that sat out a sixty-second `Retry-After` and
/// then made one more request before noticing was not cancelled, and a
/// paused one would resume from a second writer.
pub async fn run<F, Fut, T>(control: &Control, op: F) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    run_up_to(control, MAX_ATTEMPTS, op).await
}

/// As [`run_up_to`], with the count started again by any attempt that made
/// progress, as `progress` measures it (the bytes on disk). The budget
/// is for failing to get anywhere: a link that drops every few megabytes,
/// each drop resumed from where it got to, is a download going well, and
/// five such drops used to fail a large file part-way.
pub async fn run_making_progress<F, Fut, T>(
    control: &Control,
    attempts: impl Into<Tries>,
    progress: impl Fn() -> u64,
    op: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    run_making_progress_telling(control, attempts, progress, |_, _, _, _| {}, op).await
}

/// As [`run_making_progress`], with `retrying` told of each attempt after a
/// failed one, before the wait for it: the attempt's number (the first try
/// is 1) and how many there may be (0 for "keep trying"), so a row can say
/// it is retrying rather than drop back and climb without a word.
pub async fn run_making_progress_telling<F, Fut, T>(
    control: &Control,
    tries: impl Into<Tries>,
    progress: impl Fn() -> u64,
    retrying: impl Fn(u32, u32, &Error, Duration),
    op: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    run_making_progress_unless(
        control,
        None,
        tries,
        progress,
        retrying,
        |_| false,
        |_| None,
        op,
    )
    .await
}

/// As [`run_making_progress_telling`], giving up at once on an error
/// `give_up` says is not worth waiting out here, though it is transient: a
/// server turning away one more connection while others are being served.
/// `together`, where the attempt is one connection of several fetching one
/// file, has them take turns asking a server they have all lost, and sends
/// them all again the moment one of them gets a byte ([`Together`]).
/// `paced` names the wait after an error where the caller knows the server's
/// own pace better than the backoff does (a server that limits how often it
/// takes a request); the attempt counts against the tries all the same.
#[allow(clippy::too_many_arguments)]
pub async fn run_making_progress_unless<F, Fut, T>(
    control: &Control,
    together: Option<&Together>,
    tries: impl Into<Tries>,
    progress: impl Fn() -> u64,
    retrying: impl Fn(u32, u32, &Error, Duration),
    give_up: impl Fn(&Error) -> bool,
    paced: impl Fn(&Error) -> Option<Duration>,
    mut op: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    let Tries {
        attempts,
        wait: every,
    } = tries.into();
    let limit = if attempts == 0 { u32::MAX } else { attempts };
    let mut pace = Pace::default();
    loop {
        control.check()?;
        let before = progress();
        // Read before asking: a first byte that comes to another connection
        // while this one asks, or waits after, is news this one acts on.
        let seen = together.map(Together::seen);
        match op().await {
            Ok(value) => return Ok(value),
            Err(error) => {
                if progress() > before {
                    pace = Pace::default();
                }
                if !transient(&error) || give_up(&error) {
                    return Err(error);
                }
                let Some(nap) = pace.failed(&error, limit, every) else {
                    return Err(error);
                };
                let nap = paced(&error).unwrap_or(nap);
                /* Asked again at once and failed again for want of a
                 * connection, with no connection of the file bringing bytes
                 * in: the server is gone for all of them, so this one waits
                 * its turn among them rather than its own lane's time. One
                 * failing beside others that are fetching keeps its own pace:
                 * the server may be turning that one away. */
                let turn = match (together, seen) {
                    (Some(together), Some(seen))
                        if pace.quick >= 2 && quickly(&error, every) && together.flowing() == 0 =>
                    {
                        let now = tokio::time::Instant::now();
                        Some((together, seen, together.turn_at(now, pace.turn)))
                    }
                    _ => None,
                };
                let nap = turn.map_or(nap, |(_, _, at)| {
                    at.saturating_duration_since(tokio::time::Instant::now())
                });
                // Said with how long it waits, so a row can tell a retry that
                // comes in a moment from one that comes in half a minute.
                retrying(pace.attempt.saturating_add(1), attempts, &error, nap);
                match turn {
                    Some((together, seen, at)) => {
                        together.wait_turn(control, seen, at).await?;
                        pace.turn = Some(tokio::time::Instant::now());
                    }
                    None => wait(control, nap).await?,
                }
            }
        }
    }
}

/// While every connection of a download fails for want of a connection, each
/// asks again once a round. The round is this at first...
const ROUND_LEAST: Duration = Duration::from_millis(800);

/// ...and grows to this, a twenty-fifth of how long the failing has gone on
/// (from twenty seconds in to fifty), so a long outage is asked about as often
/// as each connection asked on its own before there were turns (`quick_nap`:
/// two seconds and up to a quarter more).
const ROUND_MOST: Duration = Duration::from_secs(2);

/// Never two turns of one download closer than this: ten a second at most,
/// however many connections it has.
const TURN_LEAST: Duration = Duration::from_millis(100);

/// What the connections fetching one file share as they try again: how many
/// are bringing bytes in, and, while none is, whose turn it is to ask.
///
/// Eight connections that lost the server together each asked again on its own
/// lane's time, every two seconds a quarter-second apart: the first byte after
/// a fifteen-second outage came 0.2 s after the server returned, and the other
/// seven connections over the next 1.75 s (`bench` outage, macOS, 2026-11-11),
/// where reference A, asking about six hundred times a second, had a request
/// waiting. Taking turns, a download asks evenly, ten times a second while it
/// has eight connections, and the first byte any of them gets sends every other
/// one at once. Hundreds a second are still not copied.
#[derive(Debug)]
pub struct Together {
    /// How many connections the download spreads its turns over.
    connections: u32,
    /// Requests bringing bytes in now, each from its first byte to its end.
    flowing: AtomicUsize,
    /// The most that have brought bytes in at once: what a server that turns
    /// requests away has shown it serves (`download::Limits`).
    most: AtomicUsize,
    /// How many first bytes have come: one that comes while a connection waits
    /// its turn sends it at once.
    came: tokio::sync::watch::Sender<u64>,
    /// The turns, while every connection fails.
    turns: std::sync::Mutex<Turns>,
}

/// Where the turns of one outage stand.
#[derive(Debug, Default)]
struct Turns {
    /// When the connections began failing together.
    since: Option<tokio::time::Instant>,
    /// The soonest the next turn may be.
    next: Option<tokio::time::Instant>,
}

impl Together {
    /// For a download of `connections` connections (at least one).
    pub fn new(connections: usize) -> Together {
        Together {
            connections: u32::try_from(connections.max(1)).unwrap_or(u32::MAX),
            flowing: AtomicUsize::new(0),
            most: AtomicUsize::new(0),
            came: tokio::sync::watch::Sender::new(0),
            turns: std::sync::Mutex::new(Turns::default()),
        }
    }

    /// How many requests are bringing bytes in now.
    pub fn flowing(&self) -> usize {
        self.flowing.load(Ordering::SeqCst)
    }

    /// The most requests that have brought bytes in at once.
    pub fn most(&self) -> usize {
        self.most.load(Ordering::SeqCst)
    }

    /// A request's first byte: it is counted among those bringing bytes in for
    /// as long as the guard lives, and every connection waiting its turn asks
    /// at once, the server being back.
    pub fn bringing(&self) -> Bringing<'_> {
        let now = self.flowing.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        // The next outage's turns begin afresh, at a point of their own.
        *crate::lock(&self.turns) = Turns::default();
        self.came.send_modify(|came| *came = came.wrapping_add(1));
        Bringing(self)
    }

    /// How many first bytes have come so far.
    fn seen(&self) -> u64 {
        *self.came.borrow()
    }

    /// When a connection that failed at `now`, and last asked at `last` in
    /// this outage, asks next: no sooner than a round after `last`, and no
    /// sooner than a share of the round after the turn before it, so the
    /// download's connections take the round evenly. The first turn of an
    /// outage falls at a random point of its share ([`phase`]).
    fn turn_at(
        &self,
        now: tokio::time::Instant,
        last: Option<tokio::time::Instant>,
    ) -> tokio::time::Instant {
        let mut turns = crate::lock(&self.turns);
        let since = *turns.since.get_or_insert(now);
        let round = ((now - since) / 25).clamp(ROUND_LEAST, ROUND_MOST);
        let apart = (round / self.connections).max(TURN_LEAST);
        let own = last.map_or(now, |last| (last + round).max(now));
        let turn = match turns.next {
            Some(next) => next.max(own),
            None => own + phase(apart),
        };
        turns.next = Some(turn + apart);
        turn
    }

    /// Waits until `at`, or only until a first byte has come to any connection
    /// since `seen`: the server is back, and this connection asks now.
    async fn wait_turn(
        &self,
        control: &Control,
        seen: u64,
        at: tokio::time::Instant,
    ) -> Result<(), Error> {
        let mut came = self.came.subscribe();
        loop {
            control.check()?;
            let now = tokio::time::Instant::now();
            if *came.borrow_and_update() != seen || now >= at {
                return Ok(());
            }
            // Looked at every tenth of a second for a pause or a cancel, as
            // `wait` does.
            tokio::select! {
                () = tokio::time::sleep_until(at.min(now + Duration::from_millis(100))) => {}
                // An error only once the sender is gone, which it is not while
                // `self` is here to wait on.
                _ = came.changed() => {}
            }
        }
    }
}

/// A request counted among those bringing bytes in ([`Together::bringing`])
/// while it lives.
#[derive(Debug)]
pub struct Bringing<'a>(&'a Together);

impl Drop for Bringing<'_> {
    fn drop(&mut self) {
        self.0.flowing.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A random share of `within`: where the first turn of an outage falls. Two
/// downloads, or two computers, that lost a server in the same instant then
/// ask it apart rather than in the same instant every round, and do not all
/// come back to it at once. It also keeps the wait after an outage from
/// hanging on how long the outage lasted: turns counted from its first
/// instant would find a server gone a whole number of tenths of a second back
/// at once, and one gone a few milliseconds longer a tenth later.
fn phase(within: Duration) -> Duration {
    let mut bytes = [0u8; 4];
    let share = match getrandom::fill(&mut bytes) {
        Ok(()) => f64::from(u32::from_le_bytes(bytes)) / (f64::from(u32::MAX) + 1.0),
        // Without the system's randomness, half-way: as good as any one point.
        Err(_) => 0.5,
    };
    within.mul_f64(share)
}

/// Where a run of failed attempts stands.
///
/// A server that answered (a 503, a 429) is asked again on `backoff`'s
/// pace. A connection that failed or dropped is asked again at once, then
/// at most two seconds apart: reference A was back on every connection
/// within 0.04 s of a cut and within 0.01 s of a forty-second outage
/// ending, where `backoff` left dm 1.2 s and 22 s behind
/// (`docs/reference-behavior.md`, measured 2026-10-31). Its own pace there,
/// about 205 attempts a second, is not copied. The quick attempts are not
/// what is counted against the person's number of tries: `attempt` is
/// how far into `backoff`'s schedule the failing has run, so "attempt 5 of
/// 15" means what it did and a download outlasts an outage as long as it did.
#[derive(Debug, Default)]
struct Pace {
    /// The attempt a row shows, and the one the limit is held against.
    attempt: u32,
    /// Quick attempts made since the failing began.
    quick: u32,
    /// When the failing began.
    since: Option<tokio::time::Instant>,
    /// Where in the two-second round this run of failing asks, so the
    /// connections of a download that failed together do not all ask in the
    /// same instant: eight that asked in step found a server back up to two
    /// seconds after it came (`bench` outage, 1.97 s, against reference A's
    /// request already waiting), eight a quarter-second apart within 0.25 s.
    /// Where the download's connections take turns ([`Together`]), only one
    /// failing while others fetch still asks on its lane.
    lane: Option<u32>,
    /// When this connection last asked on its turn among the download's.
    turn: Option<tokio::time::Instant>,
}

impl Pace {
    /// Counts a failed attempt: the wait before the next one, or `None`
    /// when the attempts are spent.
    fn failed(&mut self, error: &Error, limit: u32, every: Option<Duration>) -> Option<Duration> {
        self.failed_at(tokio::time::Instant::now(), error, limit, every)
    }

    fn failed_at(
        &mut self,
        now: tokio::time::Instant,
        error: &Error,
        limit: u32,
        every: Option<Duration>,
    ) -> Option<Duration> {
        let since = *self.since.get_or_insert(now);
        if quickly(error, every) {
            self.quick = self.quick.saturating_add(1);
            self.attempt = self.attempt.max(schedule_reached(now - since, limit));
            let lane = *self.lane.get_or_insert_with(next_lane);
            (self.attempt < limit).then(|| quick_nap(self.quick, lane))
        } else {
            self.attempt = self.attempt.saturating_add(1);
            (self.attempt < limit).then(|| pause(self.attempt, error.retry_after(), every))
        }
    }
}

/// Whether a failed attempt is asked again on the quick pace: it failed for
/// want of a connection, and neither the server nor the person named a wait.
fn quickly(error: &Error, every: Option<Duration>) -> bool {
    every.is_none() && error.retry_after().is_none() && dropped(error)
}

/// Whether the attempt failed for want of a connection, not on a server's
/// answer: refused, reset, timed out or cut off mid-body.
fn dropped(error: &Error) -> bool {
    matches!(
        error,
        Error::Network(_) | Error::Connection(_) | Error::Incomplete { .. }
    )
}

/// The attempt `backoff`'s schedule would be on after `failing` of failed
/// attempts: 1 at first, one more each time its waits add up to `failing`.
fn schedule_reached(failing: Duration, limit: u32) -> u32 {
    let mut attempt = 1;
    let mut spent = Duration::ZERO;
    while attempt < limit {
        spent += backoff(attempt, None);
        if failing < spent {
            break;
        }
        attempt += 1;
    }
    attempt
}

/// The wait before quick attempt `n` (the first is 1): none, then a
/// quarter, a half and a whole second, then two seconds apart, each a
/// little longer by the attempt number. The first two-second wait is cut
/// short by `lane` quarter-seconds, so lanes keep apart from then on. A
/// connection whose download's every connection is failing waits its turn
/// instead, after the first ([`Together::turn_at`]).
fn quick_nap(n: u32, lane: u32) -> Duration {
    const STEPS_MS: [u64; 5] = [0, 250, 500, 1000, 2000];
    let mut step = STEPS_MS[(n.saturating_sub(1) as usize).min(STEPS_MS.len() - 1)];
    if n as usize == STEPS_MS.len() {
        step -= u64::from(lane % 8) * 250;
    }
    let jitter = if n > 1 { u64::from(n) * 137 % 250 } else { 0 };
    Duration::from_millis(step + jitter)
}

/// A lane for a run of failing that has just begun: each the next of eight.
fn next_lane() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// As [`run`], with the number of attempts a setting rather than a
/// constant: a person on a link that drops for minutes at a time wants
/// more than five, and one who asked for "keep trying" (zero) wants no
/// end but a pause or a cancel. The backoff stops growing at fifteen
/// seconds (`backoff`), so an endless retry asks about four times a minute.
pub async fn run_up_to<F, Fut, T>(
    control: &Control,
    attempts: impl Into<Tries>,
    op: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    run_up_to_telling(control, attempts, |_, _, _, _| {}, op).await
}

/// As [`run_up_to`], with `retrying` told of each attempt after a failed
/// one, as [`run_making_progress_telling`] tells it.
pub async fn run_up_to_telling<F, Fut, T>(
    control: &Control,
    tries: impl Into<Tries>,
    retrying: impl Fn(u32, u32, &Error, Duration),
    mut op: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    let Tries {
        attempts,
        wait: every,
    } = tries.into();
    let limit = if attempts == 0 { u32::MAX } else { attempts };
    let mut pace = Pace::default();
    loop {
        control.check()?;
        match op().await {
            Ok(value) => return Ok(value),
            Err(error) => {
                if !transient(&error) {
                    return Err(error);
                }
                let Some(nap) = pace.failed(&error, limit, every) else {
                    return Err(error);
                };
                // Said with how long it waits, so a row can tell a retry that
                // comes in a moment from one that comes in half a minute.
                retrying(pace.attempt.saturating_add(1), attempts, &error, nap);
                wait(control, nap).await?;
            }
        }
    }
}

/// How many attempts, and how long between them: `None` for dm's own
/// pace (`backoff`), or a person's fixed wait from Options › Advanced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tries {
    /// How many tries before giving up.
    pub attempts: u32,
    /// The wait between tries; `None` is dm's own pace.
    pub wait: Option<Duration>,
}

impl From<u32> for Tries {
    fn from(attempts: u32) -> Self {
        Tries {
            attempts,
            wait: None,
        }
    }
}

/// The wait before attempt `attempt`: what the server asked for (within
/// reason), else the person's fixed wait, else dm's own backoff.
fn pause(attempt: u32, retry_after: Option<Duration>, every: Option<Duration>) -> Duration {
    match (retry_after, every) {
        (None, Some(every)) => every,
        _ => backoff(attempt, retry_after),
    }
}

/// Sleeps for `total`, unless asked to stop first.
async fn wait(control: &Control, total: Duration) -> Result<(), Error> {
    let until = tokio::time::Instant::now() + total;
    loop {
        control.check()?;
        let now = tokio::time::Instant::now();
        if now >= until {
            return Ok(());
        }
        tokio::time::sleep((until - now).min(Duration::from_millis(100))).await;
    }
}

/// The `Retry-After` a refusal carried, if it carried a usable one.
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    /* The HTTP-date form, measured against the response's own `Date` rather
     * than this machine's clock: both are the server's, so a local clock
     * that is wrong cannot turn "in two minutes" into "a day ago" or "next
     * week". Without a `Date` there is nothing safe to measure from, and the
     * hint is left alone. */
    let at = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let now = headers.get(reqwest::header::DATE)?.to_str().ok()?;
    let now = chrono::DateTime::parse_from_rfc2822(now.trim()).ok()?;
    let wait = (at - now).num_seconds().max(0);
    Some(Duration::from_secs(wait as u64))
}

#[cfg(test)]
mod tests;
