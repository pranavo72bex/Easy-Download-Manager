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

/// Refuses the first `fail` requests with 503, then serves the file.
struct Flaky {
    data: Vec<u8>,
    seen: AtomicUsize,
    fail: usize,
}

async fn flaky(State(f): State<Arc<Flaky>>, headers: HeaderMap) -> Response {
    if f.seen.fetch_add(1, Ordering::SeqCst) < f.fail {
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
    let quick =
        dm_core::proxy::client_with_timeout(&Default::default(), Duration::from_secs(1)).unwrap();
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
    let busy = Arc::clone(&b);
    let chunks: Vec<Vec<u8>> = bytes.chunks(64 * 1024).map(<[u8]>::to_vec).collect();
    let paced = futures_util::stream::iter(chunks.into_iter().enumerate()).then(move |(i, c)| {
        let busy = Arc::clone(&busy);
        let last = i + 1 == bytes.len().div_ceil(64 * 1024);
        async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if last {
                busy.now.fetch_sub(1, Ordering::SeqCst);
            }
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
/// a seventh of curl's speed.
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
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        busy.data,
        "bytes differ"
    );
    assert!(
        busy.most.load(Ordering::SeqCst) > 0,
        "the server turned none away"
    );
    // 4 MiB at 2 MB/s a connection on two: about two seconds. The refusals
    // waited out took sixteen.
    assert!(
        took < Duration::from_secs(8),
        "took {took:?}: the refusals were waited out"
    );
}
