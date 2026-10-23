//! A ceiling on how fast bytes are allowed to arrive.
//!
//! The cap is shared across every connection of every download, because that
//! is the only kind anyone wants. A per-connection limit multiplied by eight
//! ranges and three files is not a limit; the reason to set one at all is to
//! leave the link usable for something else, and that is a statement about
//! the link, not about any one transfer.
//!
//! A torrent and a yt-dlp run are fetched by engines dm cannot meter chunk
//! by chunk; each can only be handed a rate. So while one runs it holds a
//! [`Share`]: with `n` of them running, the ceiling is split `n + 1` ways,
//! one part each and one for every download dm fetches itself, and the
//! total stays under the ceiling.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

/// How long a waiting caller sleeps before looking at the cap again.
///
/// The wait is sliced rather than capped. Capping it looks like the same
/// thing and is not: a truncated wait is a chunk let through early, so with
/// enough connections queued at once the ceiling stops being one. Slicing
/// keeps the total honest while still noticing a rate change within a
/// quarter of a second.
const SLICE: Duration = Duration::from_millis(250);

/// The most a counting `take` lets through at a time.
///
/// A connection reads whatever the socket holds, which on a fast link is a
/// quarter of a megabyte at once. Counted whole as it arrived, eight
/// connections each had a chunk counted before the ceiling had let any of
/// it through, and the shown speed ran at twice the limit for seconds.
const PIECE: u64 = 16 * 1024;

/// A shared rate ceiling. Cheap to clone; every clone shares one budget.
#[derive(Clone)]
pub struct Limiter {
    /// The rate again, outside the lock.
    ///
    /// `take` is called once per chunk by every connection of every download
    /// — thousands of times a second — and almost always there is no limit
    /// set at all. Taking a shared mutex to discover that has every
    /// connection queueing behind every other to read a zero. This is the
    /// same number, readable without waiting for anybody.
    rate: Arc<AtomicU64>,
    /// How many outside engines hold a share right now.
    outside: Arc<AtomicU64>,
    /// When one of dm's own downloads last took bytes, in milliseconds
    /// after `born`: while none has lately, the outside engines share the
    /// whole ceiling rather than leaving a part for nobody.
    own_seen: Arc<AtomicU64>,
    born: Instant,
    inner: Arc<Mutex<Bucket>>,
    /// A second ceiling held to as well: one download's own limit, chained
    /// to the one every download shares (`chained`).
    also: Option<Arc<Limiter>>,
}

/// How long after its last bytes one of dm's own downloads still counts as
/// running, for sharing the ceiling.
const OWN_QUIET: Duration = Duration::from_secs(2);

/// One outside engine's claim on the ceiling, for as long as it runs.
/// Dropping it gives the share back.
pub struct Share {
    outside: Arc<AtomicU64>,
}

impl Drop for Share {
    fn drop(&mut self) {
        self.outside.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One part of `rate` split among `outside` engines and dm's own downloads.
/// Zero stays zero: no ceiling is no ceiling for anyone.
pub fn split(rate: u64, outside: u64) -> u64 {
    if rate == 0 {
        return 0;
    }
    (rate / outside.saturating_add(1)).max(1)
}

struct Bucket {
    /// Bytes per second across everything. Zero means no ceiling at all.
    rate: u64,
    /// The moment the next byte handed out is allowed to have arrived.
    next: Instant,
    /// Bumped whenever the rate changes, so a caller part-way through a wait
    /// computed under the old rate can tell that it no longer applies.
    generation: u64,
}

impl Limiter {
    /// A limiter that lets everything through.
    pub fn unlimited() -> Self {
        Limiter::new(0)
    }

    /// `rate` is bytes per second; zero means unlimited.
    pub fn new(rate: u64) -> Self {
        Limiter {
            rate: Arc::new(AtomicU64::new(rate)),
            outside: Arc::new(AtomicU64::new(0)),
            own_seen: Arc::new(AtomicU64::new(u64::MAX)),
            born: Instant::now(),
            inner: Arc::new(Mutex::new(Bucket {
                rate,
                next: Instant::now(),
                generation: 0,
            })),
            also: None,
        }
    }

    /// This ceiling and `own` together: bytes wait for both. For one
    /// download's limit of its own beside the one every download shares;
    /// changing either's rate reaches the download at once.
    pub fn chained(&self, own: &Limiter) -> Limiter {
        let mut both = self.clone();
        both.also = Some(Arc::new(Limiter {
            also: None,
            ..own.clone()
        }));
        both
    }

    pub async fn rate(&self) -> u64 {
        self.inner.lock().await.rate
    }

    /// Claims a share of the ceiling for an outside engine, until dropped.
    pub fn share(&self) -> Share {
        self.outside.fetch_add(1, Ordering::AcqRel);
        Share {
            outside: self.outside.clone(),
        }
    }

    /// The rate one outside engine may use now: its part of the ceiling.
    /// With none of dm's own downloads running, the outside engines share
    /// all of it: a torrent alone was held to half the limit a person set.
    pub async fn part(&self) -> u64 {
        let rate = self.rate().await;
        let outside = self.outside.load(Ordering::Acquire);
        let part = if self.own_running() {
            split(rate, outside)
        } else if rate == 0 {
            0
        } else {
            (rate / outside.max(1)).max(1)
        };
        // Never more than the download's own limit, where it has one.
        match self.also.as_ref().map(|a| a.rate.load(Ordering::Acquire)) {
            Some(own) if own > 0 && (part == 0 || own < part) => own,
            _ => part,
        }
    }

    /// Whether one of dm's own downloads took bytes lately.
    fn own_running(&self) -> bool {
        let seen = self.own_seen.load(Ordering::Acquire);
        seen != u64::MAX
            && self.born.elapsed().as_millis() as u64
                <= seen.saturating_add(OWN_QUIET.as_millis() as u64)
    }

    /// Changes the ceiling for everything already running.
    ///
    /// A speed cap that only applied to downloads started afterwards would
    /// be one people set in the middle of the download they wanted to slow
    /// down, and watch do nothing.
    pub async fn set_rate(&self, rate: u64) {
        let mut bucket = self.inner.lock().await;
        bucket.rate = rate;
        // Published under the lock, so a reader outside it never sees a rate
        // the bucket has not been told about.
        self.rate.store(rate, Ordering::Release);
        // Whatever was reserved under the old rate is now the wrong length,
        // including reservations already being waited out.
        bucket.next = Instant::now();
        bucket.generation += 1;
    }

    /// Waits until this many bytes are within the ceiling.
    ///
    /// Called after a chunk has arrived rather than before asking for one:
    /// the delay stops the stream being read, and a stream that is not read
    /// stops being sent. Reserving a slot under the lock and sleeping outside
    /// it is what keeps the total right — every caller sleeping for its own
    /// share independently would let n connections run at n times the cap.
    pub async fn take(&self, bytes: u64) {
        if let Some(own) = &self.also {
            own.take_here(bytes).await;
        }
        self.take_here(bytes).await;
    }

    /// `take` for this ceiling alone, not the one chained to it.
    async fn take_here(&self, bytes: u64) {
        // The overwhelmingly common case, and the reason this is not a lock.
        if self.rate.load(Ordering::Acquire) == 0 {
            return;
        }

        self.own_seen
            .store(self.born.elapsed().as_millis() as u64, Ordering::Release);
        let (deadline, generation) = {
            let mut bucket = self.inner.lock().await;
            if bucket.rate == 0 {
                return;
            }
            let now = Instant::now();
            if bucket.next < now {
                bucket.next = now;
            }
            let mine = bucket.next;
            // dm's own downloads get one part while outside engines hold theirs.
            let rate = split(bucket.rate, self.outside.load(Ordering::Acquire));
            let cost = Duration::from_secs_f64(bytes as f64 / rate as f64);
            bucket.next += cost;
            (mine, bucket.generation)
        };

        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            tokio::time::sleep(left.min(SLICE)).await;

            // The cap moved while this chunk was waiting for the old one.
            // Holding it back under a rule that no longer exists is how a
            // download appears to hang after the limit was lifted.
            if self.inner.lock().await.generation != generation {
                return;
            }
        }
    }
}

impl Limiter {
    /// Waits until `bytes` are within the ceiling, adding them to `counted`
    /// a piece at a time as the ceiling lets each through.
    /// For a byte count that is shown as progress: the count then rises at
    /// the rate the ceiling allows, which is the rate people are told, not
    /// at the rate the socket happened to fill. `through` says how
    /// much of `bytes` was counted, for a caller whose wait was cut short by
    /// a pause or a cancel and who must count the rest itself.
    pub async fn take_counting(&self, bytes: u64, counted: &AtomicU64, through: &mut u64) {
        let own = self
            .also
            .as_ref()
            .map_or(0, |a| a.rate.load(Ordering::Acquire));
        if self.rate.load(Ordering::Acquire) == 0 && own == 0 {
            counted.fetch_add(bytes, Ordering::Relaxed);
            *through = bytes;
            return;
        }
        while *through < bytes {
            let piece = (bytes - *through).min(PIECE);
            self.take(piece).await;
            counted.fetch_add(piece, Ordering::Relaxed);
            *through += piece;
        }
    }
}

impl std::fmt::Debug for Limiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Limiter")
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Limiter::unlimited()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_downloads_own_limit_holds_beside_the_shared_one() {
        let shared = Limiter::unlimited();
        let own = Limiter::new(100_000);
        let both = shared.chained(&own);
        // The own limit holds where nothing is shared.
        let started = Instant::now();
        for _ in 0..4 {
            both.take(25_000).await;
        }
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(700), "{took:?}");
        assert_eq!(both.part().await, 100_000);
        // The shared one, lower, wins; lifted, the own one is the part.
        shared.set_rate(40_000).await;
        assert_eq!(both.part().await, 40_000);
        shared.set_rate(0).await;
        own.set_rate(0).await;
        let started = Instant::now();
        both.take(10_000_000).await;
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(both.part().await, 0);
    }

    #[tokio::test]
    async fn an_unlimited_limiter_never_waits() {
        let limiter = Limiter::unlimited();
        let started = Instant::now();
        for _ in 0..1000 {
            limiter.take(1024 * 1024).await;
        }
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    #[tokio::test]
    async fn an_unlimited_limiter_does_not_serialise_its_callers() {
        /* Every connection of every download calls this once a chunk. If
         * discovering that there is no limit needs a shared lock, they all
         * queue behind one another to read a zero — for nothing, on the
         * hottest path in the engine. */
        let limiter = Limiter::unlimited();
        let started = Instant::now();

        let callers: Vec<_> = (0..8)
            .map(|_| {
                let limiter = limiter.clone();
                tokio::spawn(async move {
                    for _ in 0..200_000 {
                        limiter.take(64 * 1024).await;
                    }
                })
            })
            .collect();
        for caller in callers {
            caller.await.unwrap();
        }

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "1.6M unlimited takes across 8 tasks took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn bytes_beyond_the_ceiling_are_made_to_wait() {
        // A second's worth goes straight through; the next second's worth is
        // what the cap is for.
        let limiter = Limiter::new(1000);
        let started = Instant::now();
        limiter.take(1000).await;
        limiter.take(1000).await;
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "2000 bytes at 1000/s went through in {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn the_ceiling_is_shared_rather_than_per_caller() {
        /* The whole point. Eight connections each allowed the full rate is
         * eight times the rate, which is not a limit anyone asked for. */
        let limiter = Limiter::new(1000);
        let started = Instant::now();

        let callers: Vec<_> = (0..4)
            .map(|_| {
                let limiter = limiter.clone();
                tokio::spawn(async move { limiter.take(500).await })
            })
            .collect();
        for caller in callers {
            caller.await.unwrap();
        }

        // 2000 bytes at 1000/s is a second, however many callers split it.
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "four callers took {:?} for what should cost a second",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn outside_engines_and_dms_own_downloads_split_the_ceiling() {
        let limiter = Limiter::new(900);
        assert_eq!(limiter.part().await, 900);
        let torrent = limiter.share();
        // A torrent alone has the whole ceiling: nothing of dm's own runs.
        assert_eq!(limiter.part().await, 900, "a lone torrent was held to half");
        // One of dm's own downloads taking bytes: halves.
        limiter.take(1).await;
        assert_eq!(limiter.part().await, 450, "a torrent and dm's own, halves");
        let ytdlp = limiter.share();
        assert_eq!(
            limiter.part().await,
            300,
            "two engines and dm's own, thirds"
        );
        drop(torrent);
        assert_eq!(limiter.part().await, 450);
        drop(ytdlp);
        assert_eq!(limiter.part().await, 900, "every share given back");

        let unlimited = Limiter::unlimited();
        let _held = unlimited.share();
        assert_eq!(unlimited.part().await, 0, "no ceiling stays no ceiling");
        assert_eq!(split(1, 5), 1, "never rounded down to unlimited");
    }

    #[tokio::test]
    async fn dms_own_downloads_slow_to_their_part_while_an_engine_runs() {
        /* With a torrent holding half of 1000/s, dm's own get the other
         * half: the 500 bytes after the free first chunk take a second,
         * not half of one, so the two together stay under the ceiling. */
        let limiter = Limiter::new(1000);
        let _torrent = limiter.share();
        let started = Instant::now();
        limiter.take(500).await;
        limiter.take(500).await;
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "dm's own ran at the whole ceiling beside a torrent: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn lifting_the_cap_releases_what_it_was_holding() {
        let limiter = Limiter::new(10);
        limiter.take(1000).await; // reserves a long way into the future
        limiter.set_rate(0).await;

        let started = Instant::now();
        limiter.take(1_000_000).await;
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "a lifted cap still held bytes back"
        );
    }

    #[tokio::test]
    async fn a_wait_already_under_way_is_released_when_the_cap_is_lifted() {
        /* The wait is sliced rather than capped, so a chunk reserved under a
         * slow rate would otherwise sit there for the whole of it — the
         * download appearing to hang precisely because someone lifted the
         * limit on it. */
        let limiter = Limiter::new(1);
        limiter.take(1).await; // free; the next one is what waits

        let waiting = {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.take(3600).await })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        limiter.set_rate(0).await;

        tokio::time::timeout(Duration::from_secs(3), waiting)
            .await
            .expect("lifting the cap left a chunk waiting out the old rate")
            .unwrap();
    }

    #[tokio::test]
    async fn the_total_is_enforced_however_many_chunks_are_queued_at_once() {
        /* The reason the wait is sliced and not capped. Twenty connections
         * each having their wait truncated is twenty chunks let through
         * early, and a ceiling that is not one. */
        let limiter = Limiter::new(1000);
        let started = Instant::now();

        let callers: Vec<_> = (0..20)
            .map(|_| {
                let limiter = limiter.clone();
                tokio::spawn(async move { limiter.take(250).await })
            })
            .collect();
        for caller in callers {
            caller.await.unwrap();
        }

        // 5000 bytes at 1000/s is five seconds, split however it likes.
        assert!(
            started.elapsed() >= Duration::from_millis(4500),
            "20 chunks totalling 5 KB passed a 1 KB/s cap in {:?}",
            started.elapsed()
        );
    }
}
