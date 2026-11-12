// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The settings a person reaches for when a site misbehaves: fewer
//! connections for that one site, more (or endless) attempts on a link that
//! drops, and a shorter wait on a connection that has gone quiet.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, download, HostLimit, Options};
use futures_util::StreamExt as _;

use common::{options, serve, tempdir};

fn body(n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(17).wrapping_add(3))
        .collect()
}

fn ranged(data: &[u8], headers: &HeaderMap) -> Response {
    let total = data.len();
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .map(|v| {
            let (a, b) = v.split_once('-').unwrap();
            let a: usize = a.parse().unwrap();
            let b = b.parse::<usize>().unwrap_or(total - 1).min(total - 1);
            (a, b)
        });
    match range {
        Some((a, b)) => Response::builder()
            .status(206)
            .header("content-range", format!("bytes {a}-{b}/{total}"))
            .header("etag", "\"v1\"")
            .body(Body::from(data[a..=b].to_vec()))
            .unwrap(),
        None => Response::builder()
            .status(200)
            .header("etag", "\"v1\"")
            .header("content-length", total)
            .body(Body::from(data.to_vec()))
            .unwrap(),
    }
}

/// Counts how many requests are being served at once, and the most ever.
struct Busy {
    data: Vec<u8>,
    now: AtomicUsize,
    most: AtomicUsize,
}

async fn counted(State(b): State<Arc<Busy>>, headers: HeaderMap) -> Response {
    let now = b.now.fetch_add(1, Ordering::SeqCst) + 1;
    b.most.fetch_max(now, Ordering::SeqCst);
    // Long enough that connections opened together are seen together.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let res = ranged(&b.data, &headers);
    b.now.fetch_sub(1, Ordering::SeqCst);
    res
}

#[tokio::test]
async fn a_site_with_a_limit_of_its_own_is_fetched_on_that_many_connections() {
    let busy = Arc::new(Busy {
        data: body(8 * 1024 * 1024),
        now: AtomicUsize::new(0),
        most: AtomicUsize::new(0),
    });
    let app = axum::Router::new()
        .route("/big.bin", get(counted))
        .with_state(Arc::clone(&busy));
    let addr = serve(app).await;
    let dir = tempdir();
    // Eight connections everywhere, two for this one site — named by a
    // suffix, as a person would write it, and in another case.
    let limited = Options {
        connections: 8,
        host_limits: vec![
            HostLimit {
                host: "0.0.1".into(),
                connections: 2,
            },
            HostLimit {
                host: "elsewhere.example".into(),
                connections: 1,
            },
        ],
        ..options(dir.path())
    };
    let url = format!("http://127.0.0.1:{}/big.bin", addr.port());
    assert_eq!(limited.connections_for(&url), 2);
    assert_eq!(limited.connections_for("http://elsewhere.example/x"), 1);
    assert_eq!(limited.connections_for("http://other.example/x"), 8);
    // A site's own limit never raises the setting.
    assert_eq!(
        Options {
            connections: 1,
            ..limited.clone()
        }
        .connections_for(&url),
        1
    );

    let done = download(&client(), &url, &limited).await.expect("download");
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        busy.data,
        "bytes differ"
    );
    assert_eq!(done.connections, 2, "the row says how many it used");
    assert!(
        busy.most.load(Ordering::SeqCst) <= 2,
        "{} connections at once to a site limited to 2",
        busy.most.load(Ordering::SeqCst)
    );
}

/// Refuses the first `fail` ranged requests with 503, then serves the file.
/// A plain request (the engine's one second try of a failed first request) is
/// refused and not counted, so `seen` counts attempts.
struct Flaky {
    data: Vec<u8>,
    seen: AtomicUsize,
    fail: usize,
}

async fn flaky(State(f): State<Arc<Flaky>>, headers: HeaderMap) -> Response {
    let plain = !headers.contains_key("range");
    if plain || f.seen.fetch_add(1, Ordering::SeqCst) < f.fail {
        return Response::builder()
            .status(503)
            .header("retry-after", "1")
            .body(Body::from("busy"))
            .unwrap();
    }
    ranged(&f.data, &headers)
}

#[tokio::test]
async fn attempts_are_as_many_as_asked_and_zero_keeps_trying() {
    let stand_up = |fail| async move {
        let f = Arc::new(Flaky {
            data: body(64 * 1024),
            seen: AtomicUsize::new(0),
            fail,
        });
        let app = axum::Router::new()
            .route("/f.bin", get(flaky))
            .with_state(Arc::clone(&f));
        (f, serve(app).await)
    };

    // Two attempts against a server that refuses three: told after the
    // second, not the fifth.
    let (f, addr) = stand_up(3).await;
    let dir = tempdir();
    let two = Options {
        attempts: 2,
        ..options(dir.path())
    };
    let outcome = download(&client(), &format!("http://{addr}/f.bin"), &two).await;
    assert!(
        matches!(outcome, Err(dm_core::Error::Refused { status: 503, .. })),
        "{outcome:?}"
    );
    assert_eq!(f.seen.load(Ordering::SeqCst), 2, "asked more than twice");

    // Zero: kept asking until it answered.
    let (f, addr) = stand_up(3).await;
    let dir = tempdir();
    let endless = Options {
        attempts: 0,
        ..options(dir.path())
    };
    let done = download(&client(), &format!("http://{addr}/f.bin"), &endless)
        .await
        .expect("a download that was to keep trying gave up");
    assert_eq!(std::fs::read(&done.path).unwrap(), f.data);
    assert!(
        f.seen.load(Ordering::SeqCst) >= 4,
        "{}",
        f.seen.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn a_connection_that_sends_nothing_is_given_up_on_within_the_timeout_set() {
    // Headers, then silence: with the setting at a second and one attempt,
    // the download is over in seconds, not after the default half minute.
    async fn silent() -> Response {
        let late = futures_util::stream::once(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"late"))
        });
        Response::builder()
            .status(200)
            .header("content-length", "4")
            .body(Body::from_stream(late))
            .unwrap()
    }
    let addr = serve(axum::Router::new().route("/s.bin", get(silent))).await;
    let dir = tempdir();
    let once = Options {
        attempts: 1,
        ..options(dir.path())
    };
    let quick = dm_core::client_with_timeout(&Default::default(), Duration::from_secs(1)).unwrap();
    let started = Instant::now();
    let outcome = download(&quick, &format!("http://{addr}/s.bin"), &once).await;
    assert!(outcome.is_err(), "a silent server was called a download");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_link_that_drops_every_half_megabyte_still_delivers_the_file() {
    // Each response stops 512 KiB in, every time. Each drop resumes from
    // where it got to, so the download is making progress, and five such
    // drops must not fail it.
    let data = Arc::new(body(4 * 1024 * 1024));
    let served = Arc::clone(&data);
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |headers: HeaderMap| {
            let data = Arc::clone(&served);
            async move {
                let total = data.len();
                let (a, b) = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .map(|v| {
                        let (a, b) = v.split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b = b.parse::<usize>().unwrap_or(total - 1).min(total - 1);
                        (a, b)
                    })
                    .unwrap_or((0, total - 1));
                let cut = (b + 1).min(a + 512 * 1024);
                let sent = data[a..cut].to_vec();
                // Promises the whole range, sends half a megabyte of it.
                let stream = futures_util::stream::iter(vec![
                    Ok::<_, std::io::Error>(bytes::Bytes::from(sent)),
                    Err(std::io::Error::other("dropped")),
                ]);
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{total}"))
                    .header("content-length", b - a + 1)
                    .header("etag", "\"v1\"")
                    .body(Body::from_stream(stream))
                    .unwrap()
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let one = Options {
        connections: 1,
        attempts: 5,
        ..options(dir.path())
    };
    let done = download(&client(), &format!("http://{addr}/f.bin"), &one)
        .await
        .expect("five resumed drops failed the download");
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
}

/// Serves two requests at a time and turns the rest away with 429, as
/// Hetzner's mirror does: with no limit set for the site.
async fn two_at_a_time(State(b): State<Arc<Busy>>, headers: HeaderMap) -> Response {
    let now = b.now.fetch_add(1, Ordering::SeqCst) + 1;
    if now > 2 && headers.contains_key("range") {
        b.now.fetch_sub(1, Ordering::SeqCst);
        b.most.fetch_add(1, Ordering::SeqCst);
        return Response::builder()
            .status(429)
            .body(Body::from("too many connections"))
            .unwrap();
    }
    // Each served at about 2 MB/s, as a real server's connection is: the
    // two it serves are busy for a while, as on a large file.
    let res = ranged(&b.data, &headers);
    let status = res.status();
    let range = res.headers().get("content-range").cloned();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    // Its place given back however the body ends, as a server frees one on a
    // disconnect: sent whole, or let go of part-way.
    struct Place(Arc<Busy>);
    impl Drop for Place {
        fn drop(&mut self) {
            self.0.now.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let place = Arc::new(Place(Arc::clone(&b)));
    let chunks: Vec<Vec<u8>> = bytes.chunks(64 * 1024).map(<[u8]>::to_vec).collect();
    let paced = futures_util::stream::iter(chunks).then(move |c| {
        let _held = Arc::clone(&place);
        async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            Ok::<_, std::io::Error>(c)
        }
    });
    let mut built = Response::builder().status(status);
    if let Some(range) = range {
        built = built.header("content-range", range);
    }
    built.body(Body::from_stream(paced)).unwrap()
}

/// A server that takes only two connections at once, with no limit of its
/// own set for it: the connections it turns away step aside at once and
/// the two it serves fetch the file. They used to wait each refusal out
/// with backoff, ten times over, which made a 100 MB file take minutes at
/// a seventh of curl's speed. The next file from it opens two connections,
/// and none is turned away.
#[tokio::test]
async fn connections_a_server_turns_away_step_aside_at_once() {
    let busy = Arc::new(Busy {
        data: body(4 * 1024 * 1024),
        now: AtomicUsize::new(0),
        // Here: how many requests were turned away.
        most: AtomicUsize::new(0),
    });
    let app = axum::Router::new()
        .route("/big.bin", get(two_at_a_time))
        .with_state(Arc::clone(&busy));
    let addr = serve(app).await;
    let dir = tempdir();
    let url = format!("http://127.0.0.1:{}/big.bin", addr.port());
    let started = Instant::now();
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("download");
    let took = started.elapsed();
    println!(
        "{took:?}, {} requests turned away",
        busy.most.load(Ordering::SeqCst)
    );
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        busy.data,
        "bytes differ"
    );
    assert!(
        busy.most.load(Ordering::SeqCst) > 0,
        "the server turned none away"
    );
    /* 4 MiB at 2.2 MB/s a connection (64 KiB every 30 ms) on two: about one
     * second. The refusals waited out took sixteen; the connections turned
     * away ending for good, and a wait after each refusal for the next
     * request though the server took one the moment another ended, 1.5 to
     * 4.3 (gaps row 197). */
    assert!(
        took < Duration::from_millis(1500),
        "took {took:?}: the refusals were waited out"
    );

    // The same server again: two connections from the start, none turned away.
    busy.most.store(0, Ordering::SeqCst);
    let again = tempdir();
    let done = download(&client(), &url, &options(again.path()))
        .await
        .expect("download");
    assert_eq!(std::fs::read(&done.path).unwrap(), busy.data);
    assert_eq!(done.connections, 2, "the server's limit was not kept");
    assert_eq!(
        busy.most.load(Ordering::SeqCst),
        0,
        "the second file had requests turned away"
    );
}

/// How many requests [`Mirror`] serves at once.
const MIRROR_AT_ONCE: usize = 2;
/// How many new requests [`Mirror`] takes a second, beyond its burst.
const MIRROR_EACH_SECOND: f64 = 1.0;
/// How many requests over its rate [`Mirror`] takes at once (nginx's `burst`).
const MIRROR_BURST: f64 = 2.0;
/// How late [`Mirror`] answers every request, served or refused.
const MIRROR_LATE: Duration = Duration::from_millis(100);
/// How fast [`Mirror`] sends on each connection, in bytes a second.
const MIRROR_RATE: f64 = 4.0 * 1024.0 * 1024.0;

/// A mirror that limits both how many requests it serves at once and how often it takes a new
/// one, as nginx's `limit_conn` and `limit_req` (with `nodelay`) do, and as Hetzner's mirrors were
/// seen to (2026-11-12): two at once, a request a second with a burst of two, every answer 100 ms
/// late, each connection held to [`MIRROR_RATE`]. Refused requests are answered 429 and cost a
/// place in the bucket only when they are taken.
struct Mirror {
    data: Vec<u8>,
    started: Instant,
    serving: AtomicUsize,
    /// `limit_req`'s bucket: the requests over the rate it holds, and when it was last drained.
    bucket: std::sync::Mutex<(f64, Instant)>,
    log: std::sync::Mutex<Vec<Asked>>,
}

/// One request to [`Mirror`]: when it came, what it asked for, and when its body went out
/// (from its first byte to its last, or to the client letting go), or none if it was refused.
#[derive(Debug, Clone)]
struct Asked {
    came: Duration,
    range: Option<String>,
    sent: Option<(Duration, Duration)>,
}

/// Holds a place among the requests [`Mirror`] serves for as long as a body goes out, and
/// notes when it stopped, however it stopped.
struct Sending {
    mirror: Arc<Mirror>,
    index: usize,
}

impl Drop for Sending {
    fn drop(&mut self) {
        self.mirror.serving.fetch_sub(1, Ordering::SeqCst);
        let at = self.mirror.started.elapsed();
        if let Some(sent) = self.mirror.log.lock().unwrap()[self.index].sent.as_mut() {
            sent.1 = at;
        }
    }
}

async fn mirrored(State(m): State<Arc<Mirror>>, headers: HeaderMap) -> Response {
    let came = m.started.elapsed();
    let asked = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    // Over the rate as it comes: the bucket drains at the rate, and a request that would
    // overflow it is refused and left out of it.
    let over_rate = {
        let mut bucket = m.bucket.lock().unwrap();
        let now = Instant::now();
        let held =
            (bucket.0 - MIRROR_EACH_SECOND * now.duration_since(bucket.1).as_secs_f64()).max(0.0);
        bucket.1 = now;
        let over = held + 1.0 > MIRROR_BURST + 1.0;
        bucket.0 = if over { held } else { held + 1.0 };
        over
    };
    let index = {
        let mut log = m.log.lock().unwrap();
        log.push(Asked {
            came,
            range: asked.clone(),
            sent: None,
        });
        log.len() - 1
    };
    tokio::time::sleep(MIRROR_LATE).await;
    let refused = || {
        Response::builder()
            .status(429)
            .body(Body::from("Too Many Requests"))
            .unwrap()
    };
    if over_rate {
        return refused();
    }
    if m.serving.fetch_add(1, Ordering::SeqCst) >= MIRROR_AT_ONCE {
        m.serving.fetch_sub(1, Ordering::SeqCst);
        return refused();
    }
    let total = m.data.len();
    let (a, b) = asked
        .as_deref()
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| {
            let a: usize = a.parse().unwrap();
            (a, b.parse::<usize>().unwrap_or(total - 1).min(total - 1))
        })
        .unwrap_or((0, total - 1));
    let sending = Arc::new(Sending {
        mirror: Arc::clone(&m),
        index,
    });
    const CHUNK: usize = 64 * 1024;
    let begun = tokio::time::Instant::now();
    let pieces: Vec<(usize, usize)> = (a..=b)
        .step_by(CHUNK)
        .map(|s| (s, (s + CHUNK - 1).min(b)))
        .collect();
    let paced = futures_util::stream::iter(pieces).then(move |(s, e)| {
        let sending = Arc::clone(&sending);
        async move {
            // Each piece once the connection's rate has let its last byte through.
            let due = begun + Duration::from_secs_f64((e + 1 - a) as f64 / MIRROR_RATE);
            tokio::time::sleep_until(due).await;
            let mirror = &sending.mirror;
            let at = mirror.started.elapsed();
            let mut log = mirror.log.lock().unwrap();
            let sent = log[sending.index].sent.get_or_insert((at, at));
            sent.1 = at;
            drop(log);
            Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&mirror.data[s..=e]))
        }
    });
    Response::builder()
        .status(if asked.is_some() { 206 } else { 200 })
        .header("content-range", format!("bytes {a}-{b}/{total}"))
        .header("content-length", b - a + 1)
        .header("etag", "\"mirror\"")
        .body(Body::from_stream(paced))
        .unwrap()
}

/// A server that serves two connections at once and takes a new request about once a second,
/// as Hetzner's mirrors do: dm is to fetch on both, faster than one connection could, with no
/// stretch of the download where nothing is fetched. Before, dm took about 1.6 times as long as
/// one connection: its next range's request at each range's end was refused and waited out with
/// backoff while nothing was fetched, the seams' checks went out beside it and took the request
/// the server would have taken, the connections it turned away never came back, and a range
/// halved for a request the server refused left its back half to no one for seconds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_mirror_that_limits_connections_and_requests_is_fetched_faster_than_one_connection() {
    let mirror = Arc::new(Mirror {
        data: body(32 * 1024 * 1024),
        started: Instant::now(),
        serving: AtomicUsize::new(0),
        bucket: std::sync::Mutex::new((0.0, Instant::now())),
        log: std::sync::Mutex::new(Vec::new()),
    });
    let app = axum::Router::new()
        .route("/mirror.bin", get(mirrored))
        .with_state(Arc::clone(&mirror));
    let addr = serve(app).await;
    let dir = tempdir();
    let url = format!("http://127.0.0.1:{}/mirror.bin", addr.port());
    let started = Instant::now();
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("download");
    let took = started.elapsed();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        mirror.data,
        "bytes differ"
    );
    let log = mirror.log.lock().unwrap().clone();
    let refused = log.iter().filter(|asked| asked.sent.is_none()).count();
    // Where nothing was being sent, from the first byte to the last.
    let mut sent: Vec<(Duration, Duration)> = log.iter().filter_map(|asked| asked.sent).collect();
    sent.sort();
    let mut longest_still = Duration::ZERO;
    let mut covered = sent.first().map_or(Duration::ZERO, |s| s.1);
    for &(from, to) in &sent {
        longest_still = longest_still.max(from.saturating_sub(covered));
        covered = covered.max(to);
    }
    let one_connection = Duration::from_secs_f64(mirror.data.len() as f64 / MIRROR_RATE);
    println!(
        "{took:?} for {} MiB, one connection {one_connection:?}; {} requests, {refused} refused; \
         longest with nothing sent {longest_still:?}",
        mirror.data.len() >> 20,
        log.len()
    );
    for asked in &log {
        println!("  {asked:?}");
    }
    assert!(
        longest_still < Duration::from_millis(300),
        "nothing was fetched for {longest_still:?} while the server was there to fetch from"
    );
    /* The seams' checks (a kibibyte or so each) went out once no range was being asked for:
     * asked as each range ended, each took the request the server would have taken for the
     * next range, which was refused and waited out with nothing fetched. */
    let span = |asked: &Asked| -> Option<usize> {
        let (a, b) = asked
            .range
            .as_deref()?
            .strip_prefix("bytes=")?
            .split_once('-')?;
        Some(b.parse::<usize>().ok()? + 1 - a.parse::<usize>().ok()?)
    };
    let is_check = |asked: &&Asked| span(asked).is_some_and(|n| n <= 2048);
    let first_check = log.iter().filter(is_check).map(|asked| asked.came).min();
    if let Some(first_check) = first_check {
        let later: Vec<&Asked> = log
            .iter()
            .filter(|asked| !is_check(asked) && asked.came > first_check)
            .collect();
        assert!(
            later.is_empty(),
            "ranges were asked for after a seam's check: {later:?}"
        );
    }
    // Two connections at once take half as long as one; refusals and the answers' 100 ms
    // allowed for, about 0.6 of it alone, and up to 0.65 beside a whole workspace's tests (4.85
    // to 5.2 s of 8 in four runs beside four copies of the queue tests, 2026-11-12): held to 0.7.
    // Before row 197 it took 1.6 times one connection's.
    assert!(
        took < one_connection.mul_f64(0.7),
        "took {took:?} where one connection takes {one_connection:?}"
    );
}

/// Where the server limits this download, the first answer is read on into the ranges after
/// range 0 (`limits::continue_into`), and on several connections it was asked for the file's
/// first part only (`probe::FIRST_ASK`): it ends inside a range, not at the file's end. That
/// range goes on from there with a request of its own, at once and with no try spent: taken as
/// a connection that dropped, it was said as a retry and waited a backoff out with the server
/// there to fetch from (gaps row S9).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_first_answer_read_on_into_the_ranges_after_ends_inside_one_without_a_retry() {
    // Eight ranges of 2.5 MiB: the first answer's 8 MiB end inside the fourth.
    let mirror = Arc::new(Mirror {
        data: body(20 * 1024 * 1024),
        started: Instant::now(),
        serving: AtomicUsize::new(0),
        bucket: std::sync::Mutex::new((0.0, Instant::now())),
        log: std::sync::Mutex::new(Vec::new()),
    });
    let app = axum::Router::new()
        .route("/mirror.bin", get(mirrored))
        .with_state(Arc::clone(&mirror));
    let addr = serve(app).await;
    let dir = tempdir();
    let url = format!("http://127.0.0.1:{}/mirror.bin", addr.port());
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    let said = tokio::spawn(async move {
        let mut cut_short = Vec::new();
        while let Some(update) = rx.recv().await {
            if let dm_core::Update::Retrying { why, .. } = update {
                if why.starts_with("the connection ended after") {
                    cut_short.push(why);
                }
            }
        }
        cut_short
    });
    let done = dm_core::download_reporting(&client(), &url, &options(dir.path()), Some(tx))
        .await
        .expect("download");
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        mirror.data,
        "bytes differ"
    );
    let log = mirror.log.lock().unwrap().clone();
    let first = log.first().and_then(|asked| asked.range.clone());
    assert_eq!(first.as_deref(), Some("bytes=0-8388607"), "{log:?}");
    // Read on past range 0's end into the fourth range, which goes on from the first answer's end.
    assert!(
        log.iter().any(|asked| asked
            .range
            .as_deref()
            .is_some_and(|r| r.starts_with("bytes=8388608-"))),
        "{log:?}"
    );
    let cut_short = said.await.unwrap();
    assert!(
        cut_short.is_empty(),
        "the first answer's end was taken as a dropped connection: {cut_short:?}\n{log:?}"
    );
}
