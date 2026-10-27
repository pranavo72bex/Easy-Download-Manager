//! When a connection beyond the planned ranges is worth opening: a helper
//! waits, watches how fast the others bring bytes in, and joins only if the
//! file is slow enough to be helped.

use super::*;

/// How long a helper connection waits before it asks whether the first ones
/// need it, and the window over which it then watches how fast bytes arrive.
pub(in crate::download) const HELP_AFTER: Duration = Duration::from_millis(100);

pub(in crate::download) const HELP_WATCH: Duration = Duration::from_millis(80);

/// How much work, at the rate seen, a helper is worth opening a connection
/// for: a connection costs a round trip before its first byte, so one opened
/// for what the others finish sooner only slows the end.
pub(in crate::download) const HELP_WORTH_SECS: f64 = 0.3;

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
