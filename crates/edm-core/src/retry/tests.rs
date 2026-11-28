// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
fn connections_failing_together_ask_apart_once_the_waits_are_long() {
    let cut = Error::Connection("refused".into());
    let mut asks: Vec<Duration> = (0..8)
        .map(|lane| (1..=6).map(|n| quick_nap(n, lane)).sum())
        .collect();
    asks.sort();
    assert!(
        asks.windows(2)
            .all(|w| w[1] - w[0] >= Duration::from_millis(200)),
        "{asks:?}"
    );
    let mut pace = Pace::default();
    assert!(pace.failed(&cut, 15, None).is_some() && pace.lane.is_some());
}

/// Eight connections that lose the server in the same instant ask a
/// tenth of a second apart, the first at a random point of its tenth, and
/// each again a round after its own turn.
#[test]
fn connections_that_lose_the_server_together_take_turns() {
    let together = Together::new(8);
    let now = tokio::time::Instant::now();
    let first: Vec<_> = (0..8).map(|_| together.turn_at(now, None)).collect();
    assert!(first[0] >= now && first[0] < now + TURN_LEAST, "{first:?}");
    assert!(first.windows(2).all(|w| w[1] - w[0] == TURN_LEAST));
    // Each fails a moment after its turn and is given the next.
    let again: Vec<_> = first
        .iter()
        .map(|&asked| together.turn_at(asked + Duration::from_millis(1), Some(asked)))
        .collect();
    for (asked, next) in first.iter().zip(&again) {
        assert_eq!(*next - *asked, ROUND_LEAST, "{first:?} then {again:?}");
    }
}

/// Every connection asking on its turn, failing a millisecond later and
/// taking the next: the asks of a download through an outage.
fn asks_through(together: &Together, connections: usize, lasting: Duration) -> Vec<Duration> {
    let start = tokio::time::Instant::now();
    // When each connection fails next, and when it last asked.
    let mut failing: Vec<(tokio::time::Instant, Option<tokio::time::Instant>)> =
        vec![(start, None); connections];
    let mut asks = Vec::new();
    loop {
        let (who, &(at, last)) = failing
            .iter()
            .enumerate()
            .min_by_key(|(_, (at, _))| *at)
            .expect("connections");
        if at - start > lasting {
            return asks;
        }
        let turn = together.turn_at(at, last);
        asks.push(turn - start);
        failing[who] = (turn + Duration::from_millis(1), Some(turn));
    }
}

/// A server back at any moment of a fifteen-second outage is asked within
/// a tenth of a second by a download of eight connections, at about a
/// hundred and fifty requests: not reference A's thousands.
#[test]
fn a_server_back_at_any_moment_is_asked_within_a_tenth_of_a_second() {
    let asks = asks_through(&Together::new(8), 8, Duration::from_secs(15));
    let gaps: Vec<Duration> = asks.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.iter().all(|g| *g <= TURN_LEAST), "{gaps:?}");
    assert!(asks[0] < TURN_LEAST, "{:?}", asks[0]);
    let within = asks
        .iter()
        .filter(|a| **a <= Duration::from_secs(15))
        .count();
    assert!((140..=160).contains(&within), "{within} asks in 15 s");
}

/// One connection failing alone asks once a round, not ten times a
/// second; and a long outage is asked about at the pace each connection
/// kept alone before there were turns: a download of eight, four times a
/// second.
#[test]
fn turns_are_held_to_a_round_each_and_lengthen_as_an_outage_goes_on() {
    let alone = asks_through(&Together::new(8), 1, Duration::from_secs(10));
    assert!(
        alone.windows(2).all(|w| w[1] - w[0] >= ROUND_LEAST),
        "{alone:?}"
    );
    let long = asks_through(&Together::new(8), 8, Duration::from_secs(300));
    let late: Vec<&Duration> = long
        .iter()
        .filter(|a| **a >= Duration::from_secs(60))
        .collect();
    let gaps: Vec<Duration> = late.windows(2).map(|w| *w[1] - *w[0]).collect();
    assert!(
        gaps.iter().all(|g| *g == ROUND_MOST / 8),
        "{:?}",
        &gaps[..gaps.len().min(20)]
    );
    // A download of two connections spreads them over the round evenly.
    let two = asks_through(&Together::new(2), 2, Duration::from_secs(10));
    assert!(
        two.windows(2).all(|w| w[1] - w[0] == ROUND_LEAST / 2),
        "{two:?}"
    );
}

/// The first byte any connection gets sends every one waiting its turn at
/// once, including one whose own attempt began before that byte came.
#[tokio::test]
async fn a_byte_that_comes_sends_every_waiting_connection_at_once() {
    let together = std::sync::Arc::new(Together::new(8));
    let control = Control::new();
    let before = together.seen();
    let started = std::time::Instant::now();
    let waiting: Vec<_> = (0..7)
        .map(|_| {
            let (together, control) = (std::sync::Arc::clone(&together), control.clone());
            tokio::spawn(async move {
                let seen = together.seen();
                let far = tokio::time::Instant::now() + Duration::from_secs(5);
                together
                    .wait_turn(&control, seen, far)
                    .await
                    .map(|()| started.elapsed())
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let bringing = together.bringing();
    assert_eq!(together.flowing(), 1);
    for one in waiting {
        let waited = one.await.expect("joined").expect("not stopped");
        assert!(waited < Duration::from_secs(2), "{waited:?}");
    }
    // Seen before the byte came, the wait does not begin at all.
    let far = tokio::time::Instant::now() + Duration::from_secs(5);
    let at_once = std::time::Instant::now();
    together
        .wait_turn(&control, before, far)
        .await
        .expect("not stopped");
    assert!(at_once.elapsed() < Duration::from_secs(1));
    drop(bringing);
    assert_eq!(together.flowing(), 0);
}

/// The retry loop: a connection that keeps failing for want of a
/// connection asks again at once, then on its turn when no connection is
/// bringing bytes in, and on its own lane's time when one is.
#[tokio::test]
async fn a_connection_waits_its_turn_only_when_every_connection_has_lost_the_server() {
    async fn waits(together: &Together) -> Vec<Duration> {
        let told = std::sync::Mutex::new(Vec::new());
        let tries = std::sync::atomic::AtomicU32::new(0);
        let outcome: Result<(), Error> = run_making_progress_unless(
            &Control::new(),
            Some(together),
            15,
            || 0,
            |_, _, _, wait| told.lock().unwrap().push(wait),
            |_| false,
            |_| None,
            || async {
                if tries.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 2 {
                    Err(Error::Connection("reset".into()))
                } else {
                    Ok(())
                }
            },
        )
        .await;
        assert!(outcome.is_ok(), "{outcome:?}");
        told.into_inner().unwrap()
    }
    let lost = Together::new(8);
    let turned = waits(&lost).await;
    assert_eq!(turned[0], Duration::ZERO, "{turned:?}");
    assert!(turned[1] < TURN_LEAST, "{turned:?}");
    let fetching = Together::new(8);
    let _bringing = fetching.bringing();
    let own = waits(&fetching).await;
    assert_eq!(own[0], Duration::ZERO, "{own:?}");
    assert!(own[1] >= Duration::from_millis(250), "{own:?}");
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
fn successive_attempts_wait_different_times() {
    // Same attempt number on different segments still shares a delay.
    let delays: Vec<_> = (0..4).map(|a| backoff(a, None)).collect();
    assert!(
        delays.windows(2).all(|w| w[0] != w[1]),
        "delays repeat: {delays:?}"
    );
}

#[test]
fn a_server_that_keeps_asking_for_a_second_is_waited_out_longer_each_time() {
    let one = Some(Duration::from_secs(1));
    let waits: Vec<Duration> = (0..8).map(|a| backoff(a, one)).collect();
    assert!(waits.windows(2).all(|w| w[0] <= w[1]), "{waits:?}");
    assert!(
        waits.iter().any(|w| *w > Duration::from_secs(20)),
        "never quiet for long enough: {waits:?}"
    );
    assert!(waits.iter().all(|w| *w <= Duration::from_secs(60)));
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
