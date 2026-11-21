// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
    let video = limiter.share();
    assert_eq!(
        limiter.part().await,
        300,
        "two engines and dm's own, thirds"
    );
    drop(torrent);
    assert_eq!(limiter.part().await, 450);
    drop(video);
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
