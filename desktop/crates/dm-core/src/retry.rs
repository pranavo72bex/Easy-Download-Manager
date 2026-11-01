//! When to try again, and how long to wait first.
//!
//! Retrying is cheap here in a way it is not for most clients: a segment that
//! fails resumes from what it already has on disk, so a second attempt asks
//! only for the bytes the first one did not get. That makes the interesting
//! question not "can we afford to retry" but "is this worth retrying at all"
//! — a 404 answered instantly forever is not a download in progress.

use std::future::Future;
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
/// Exponential, capped, and jittered. The jitter matters more than it looks:
/// every connection of a segmented download fails at the same instant when a
/// server goes down, and without it they would all come back at the same
/// instant too, which is how a struggling server is kept struggling.
pub fn backoff(attempt: u32, retry_after: Option<Duration>) -> Duration {
    // A server that said how long to wait is believed, within reason — an
    // hour-long Retry-After is not something to sit in.
    if let Some(asked) = retry_after {
        return asked.min(Duration::from_secs(60));
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

    // Deterministic jitter from the attempt number: no RNG dependency, and
    // enough spread to break the lockstep.
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
    run_making_progress_unless(control, tries, progress, retrying, |_| false, op).await
}

/// As [`run_making_progress_telling`], giving up at once on an error
/// `give_up` says is not worth waiting out here, though it is transient: a
/// server turning away one more connection while others are being served.
pub async fn run_making_progress_unless<F, Fut, T>(
    control: &Control,
    tries: impl Into<Tries>,
    progress: impl Fn() -> u64,
    retrying: impl Fn(u32, u32, &Error, Duration),
    give_up: impl Fn(&Error) -> bool,
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
                // Said with how long it waits, so a row can tell a retry that
                // comes in a moment from one that comes in half a minute.
                retrying(pace.attempt.saturating_add(1), attempts, &error, nap);
                wait(control, nap).await?;
            }
        }
    }
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
        let retry_after = error.retry_after();
        if every.is_none() && retry_after.is_none() && dropped(error) {
            self.quick = self.quick.saturating_add(1);
            self.attempt = self.attempt.max(schedule_reached(now - since, limit));
            (self.attempt < limit).then(|| quick_nap(self.quick))
        } else {
            self.attempt = self.attempt.saturating_add(1);
            (self.attempt < limit).then(|| pause(self.attempt, retry_after, every))
        }
    }
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
/// quarter, a half and a whole second, then two seconds apart, spread a
/// little so that connections do not come back in step.
fn quick_nap(n: u32) -> Duration {
    const STEPS_MS: [u64; 5] = [0, 250, 500, 1000, 2000];
    let step = STEPS_MS[(n.saturating_sub(1) as usize).min(STEPS_MS.len() - 1)];
    let jitter = if n > 1 { u64::from(n) * 137 % 250 } else { 0 };
    Duration::from_millis(step + jitter)
}

/// As [`run`], with the number of attempts a setting rather than a
/// constant: a person on a link that drops for minutes at a time wants
/// more than five, and one who asked for "keep trying" (zero) wants no
/// end but a pause or a cancel. The backoff stops growing at half a
/// minute, so an endless retry asks about twice a minute.
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
mod tests {
    use super::*;

    fn refused(status: u16) -> Error {
        Error::Refused {
            status,
            url: String::new(),
            body: String::new(),
            retry_after: None,
        }
    }

    #[test]
    fn a_dropped_connection_is_asked_again_at_once_then_within_two_seconds() {
        let mut pace = Pace::default();
        let cut = Error::Connection("reset".into());
        assert_eq!(pace.failed(&cut, 15, None), Some(Duration::ZERO));
        let naps: Vec<Duration> = (0..20)
            .map(|_| pace.failed(&cut, 15, None).expect("tries left"))
            .collect();
        assert!(
            naps.iter().all(|n| *n <= Duration::from_millis(2250)),
            "{naps:?}"
        );
    }

    #[test]
    fn quick_attempts_spend_the_tries_no_faster_than_backoff_did() {
        // Fifteen tries outlast a couple of minutes without a network, as
        // they did when each one waited out `backoff`.
        let cut = Error::Connection("refused".into());
        let mut pace = Pace::default();
        let start = tokio::time::Instant::now();
        let mut now = start;
        let mut shown = Vec::new();
        while let Some(nap) = pace.failed_at(now, &cut, 15, None) {
            shown.push(pace.attempt);
            now += nap.max(Duration::from_millis(1));
        }
        let lasted = now - start;
        let paced: Duration = (1..15).map(|a| backoff(a, None)).sum();
        assert!(
            lasted + Duration::from_secs(3) >= paced,
            "{lasted:?} against {paced:?}"
        );
        assert!(lasted >= Duration::from_secs(120), "{lasted:?}");
        assert!(shown.windows(2).all(|w| w[0] <= w[1]), "{shown:?}");
        assert!(shown.iter().all(|a| *a < 15), "{shown:?}");
    }

    #[test]
    fn a_server_that_answers_no_is_still_asked_on_backoffs_pace() {
        let mut pace = Pace::default();
        assert_eq!(pace.failed(&refused(503), 15, None), Some(backoff(1, None)));
        assert_eq!(pace.failed(&refused(503), 15, None), Some(backoff(2, None)));
        let mut fixed = Pace::default();
        let every = Some(Duration::from_secs(20));
        let cut = Error::Connection("reset".into());
        assert_eq!(
            fixed.failed(&cut, 15, every),
            Some(Duration::from_secs(20)),
            "a person's fixed wait is kept"
        );
    }

    #[test]
    fn a_server_having_a_bad_day_is_worth_waiting_for() {
        for status in [408, 425, 429, 500, 502, 503, 504, 507, 509, 520, 522] {
            assert!(transient(&refused(status)), "{status} should be retried");
        }
    }

    #[test]
    fn an_answer_is_not_retried() {
        // 501 is the server saying it will never do this, unlike the rest of 5xx.
        for status in [400, 401, 403, 404, 410, 501] {
            assert!(
                !transient(&refused(status)),
                "{status} should not be retried"
            );
        }
    }

    #[test]
    fn a_dishonest_or_changed_server_is_not_waited_out() {
        // Both mean the bytes are wrong, which more attempts cannot fix.
        assert!(!transient(&Error::DishonestRanges));
        assert!(!transient(&Error::Changed));
        assert!(!transient(&Error::ShortFile {
            got: 1,
            expected: 2
        }));
        // But a connection that merely stopped short is worth another try.
        assert!(transient(&Error::Incomplete {
            got: 1,
            expected: 2
        }));
    }

    #[test]
    fn a_fixed_wait_is_kept_unless_the_server_asked_for_one() {
        let every = Some(Duration::from_secs(20));
        assert_eq!(pause(1, None, every), Duration::from_secs(20));
        assert_eq!(pause(9, None, every), Duration::from_secs(20));
        assert_eq!(
            pause(1, Some(Duration::from_secs(5)), every),
            Duration::from_secs(5),
            "the server's Retry-After"
        );
        assert_eq!(pause(3, None, None), backoff(3, None));
    }

    #[test]
    fn waiting_grows_with_each_attempt_and_stops_growing() {
        let first = backoff(0, None);
        let later = backoff(3, None);
        assert!(later > first, "backoff should grow");
        // The fifteen attempts a person has by default outlast a couple of
        // minutes without a network.
        let waited: Duration = (1..15).map(|a| backoff(a, None)).sum();
        assert!(waited >= Duration::from_secs(120), "{waited:?}");
        assert!(backoff(4, None) <= Duration::from_secs(9), "quick at first");
        assert!(
            backoff(20, None) <= Duration::from_secs(16),
            "and then stop, near enough to find the server back soon"
        );
    }

    #[test]
    fn connections_do_not_all_come_back_at_once() {
        // Same attempt number on different segments still shares a delay, but
        // across attempts the spread is what breaks the lockstep.
        let delays: Vec<_> = (0..4).map(|a| backoff(a, None)).collect();
        assert!(
            delays.windows(2).all(|w| w[0] != w[1]),
            "delays repeat: {delays:?}"
        );
    }

    #[test]
    fn a_server_that_names_a_delay_is_believed_but_not_indefinitely() {
        assert_eq!(
            backoff(0, Some(Duration::from_secs(3))),
            Duration::from_secs(3)
        );
        assert_eq!(
            backoff(0, Some(Duration::from_secs(9999))),
            Duration::from_secs(60)
        );
    }

    #[tokio::test]
    async fn a_cancel_is_not_waited_out() {
        // A refusal with an hour-long Retry-After is believed for a minute;
        // cancelling in the middle of that minute must end it now, not then.
        let control = Control::new();
        let cancelled = control.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancelled.cancel();
        });
        let started = std::time::Instant::now();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "3600".parse().unwrap());
        let outcome: Result<(), Error> = run(&control, || async {
            Err(Error::Refused {
                status: 503,
                url: String::new(),
                body: String::new(),
                retry_after: retry_after(&headers),
            })
        })
        .await;
        assert!(matches!(outcome, Err(Error::Cancelled)), "got {outcome:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the cancel waited out the backoff"
        );
    }

    /// Each attempt after a failed one is told, numbered from the
    /// first try, with the number there may be.
    #[tokio::test]
    async fn each_retry_is_told_with_its_number() {
        let control = Control::new();
        let heard = std::sync::Mutex::new(Vec::new());
        let tries = std::sync::atomic::AtomicU32::new(0);
        let outcome: Result<(), Error> = run_making_progress_telling(
            &control,
            3,
            || 0,
            |attempt, of, _, _| heard.lock().unwrap().push((attempt, of)),
            || async {
                tries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Err(Error::Incomplete {
                    got: 0,
                    expected: 1,
                })
            },
        )
        .await;
        assert!(matches!(outcome, Err(Error::Incomplete { .. })));
        // A connection that stops short is asked again quickly, so there
        // are more requests than tries; a row still hears attempt 2 of 3,
        // then 3 of 3, and no number past the person's.
        assert!(tries.load(std::sync::atomic::Ordering::Relaxed) >= 3);
        let mut told = heard.lock().unwrap().clone();
        told.dedup();
        assert_eq!(told, [(2, 3), (3, 3)]);
    }

    #[test]
    fn retry_after_reads_only_what_it_can_trust() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "7".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(7)));

        // An HTTP-date with no `Date` to measure it from is left alone:
        // this machine's clock is not trusted to.
        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&headers), None);

        // Measured against the server's own clock, it is a delay.
        headers.insert(
            reqwest::header::DATE,
            "Wed, 21 Oct 2026 07:26:30 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(90)));

        // A date already past means now, not a negative wait.
        headers.insert(
            reqwest::header::DATE,
            "Wed, 21 Oct 2026 08:00:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&headers), Some(Duration::ZERO));

        // Garbage is no hint.
        headers.insert(reqwest::header::RETRY_AFTER, "soon".parse().unwrap());
        assert_eq!(retry_after(&headers), None);
    }
}
