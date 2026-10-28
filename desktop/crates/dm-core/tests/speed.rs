//! How fast a download finishes when one of its connections is slow.
//!
//! A server caps each connection (8 MB/s here), which is why a download
//! manager opens several; and one connection out of several is often far
//! slower than the rest — a congested route, a busy mirror node. With one
//! fixed range per connection, the slow one holds a whole eighth of the file
//! and everything waits for it. Split finer, a fast connection that
//! finishes takes more work and the slow one holds only a small piece.
//!
//! The test asserts the download finishes well inside what one fixed slow
//! range would take, and prints the time:
//!
//!     cargo test -p dm-core --release --test speed -- --nocapture

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, download};
use futures_util::StreamExt;

use common::{options, serve, tempdir};

const SIZE: usize = 32 * 1024 * 1024;
const FAST: u64 = 8 * 1024 * 1024;
const SLOW: u64 = 512 * 1024;
/// Which ranged request is the slow connection: the third one asked.
const SLOW_REQUEST: usize = 3;

fn body() -> Arc<Vec<u8>> {
    Arc::new(
        (0..SIZE)
            .map(|i| (i as u8).wrapping_mul(13).wrapping_add(7))
            .collect(),
    )
}

/// Serves `data[a..=b]` at `rate` bytes a second, in 64 KB chunks.
fn paced(data: Arc<Vec<u8>>, a: usize, b: usize, rate: u64) -> Body {
    const CHUNK: usize = 64 * 1024;
    let chunks: Vec<(usize, usize)> = (a..=b)
        .step_by(CHUNK)
        .map(|s| (s, (s + CHUNK - 1).min(b)))
        .collect();
    let pause = Duration::from_secs_f64(CHUNK as f64 / rate as f64);
    Body::from_stream(futures_util::stream::iter(chunks).then(move |(s, e)| {
        let data = data.clone();
        async move {
            tokio::time::sleep(pause).await;
            Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&data[s..=e]))
        }
    }))
}

/// A server of `data` whose third ranged request is the slow one, and the
/// count of ranged requests it has answered.
async fn slow_server(data: Arc<Vec<u8>>) -> (std::net::SocketAddr, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let (served, counter) = (data.clone(), asked.clone());
    let app = axum::Router::new().route(
        "/big.bin",
        get(move |headers: HeaderMap| {
            let (data, counter) = (served.clone(), counter.clone());
            async move {
                let range = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| v.split_once('-'))
                    .map(|(a, b)| {
                        let a: usize = a.parse().unwrap();
                        let b = b.parse::<usize>().unwrap_or(SIZE - 1).min(SIZE - 1);
                        (a, b)
                    });
                let Some((a, b)) = range else {
                    return Response::builder()
                        .status(200)
                        .header("content-length", SIZE)
                        .body(Body::from(data.to_vec()))
                        .unwrap();
                };
                // The probe's one byte is not a connection worth pacing.
                let n = if b > a {
                    counter.fetch_add(1, Ordering::SeqCst)
                } else {
                    usize::MAX
                };
                let rate = if n == SLOW_REQUEST { SLOW } else { FAST };
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                    .header("content-length", b - a + 1)
                    .header("etag", "\"big\"")
                    .body(paced(data, a, b, rate))
                    .unwrap()
            }
        }),
    );
    (serve(app).await, asked)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_slow_connection_does_not_hold_the_download_up() {
    let data = body();
    let (addr, asked) = slow_server(data.clone()).await;
    let dir = tempdir();
    let url = format!("http://{addr}/big.bin");

    let started = Instant::now();
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("the download failed");
    let took = started.elapsed();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        *data,
        "the file is not the file"
    );

    // One fixed eighth of 32 MB at 512 KB/s is 8 seconds on its own.
    let one_fixed_slow_range = (SIZE as u64 / 8) as f64 / SLOW as f64;
    println!(
        "32 MB, 8 connections, one at 512 KB/s: {:.2}s ({} ranged requests; one fixed slow range alone would be {:.1}s)",
        took.as_secs_f64(),
        asked.load(Ordering::SeqCst),
        one_fixed_slow_range
    );
    assert!(
        took.as_secs_f64() < one_fixed_slow_range * 0.5,
        "the slow connection held the download up: {:.2}s",
        took.as_secs_f64()
    );
}

/// A range halved under the slow connection is written into the plan beside
/// the parts, so a pause after the split, and a resume under another
/// connection setting, still put every part at its own offset.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_paused_after_its_ranges_were_halved_resumes_whole() {
    let data = body();
    let (addr, _) = slow_server(data.clone()).await;
    let dir = tempdir();
    let url = format!("http://{addr}/big.bin");
    let opts = options(dir.path());
    let plan = dir.path().join("parts").join("plan.json");
    let ranges = |plan: &std::path::Path| {
        std::fs::read(plan)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v["ranges"].as_array().map(Vec::len))
            .unwrap_or(0)
    };

    let running = {
        let (url, opts) = (url.clone(), opts.clone());
        tokio::spawn(async move { download(&client(), &url, &opts).await })
    };
    // The plan starts at one range a connection (8); more is a range halved.
    let mut planned = 0;
    for _ in 0..400 {
        planned = planned.max(ranges(&plan));
        if planned > 9 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    opts.control.pause();
    let first = running.await.unwrap();
    assert!(planned > 8, "no range was halved: {planned} ranges");
    if first.is_ok() {
        // Finished before the pause landed: still the file, still fine.
        println!("finished before the pause; the resume was not exercised");
        return;
    }

    let again = dm_core::Options {
        connections: 3,
        control: dm_core::Control::new(),
        ..opts
    };
    println!("paused at {planned} ranges; resuming on 3 connections");
    let done = download(&client(), &url, &again)
        .await
        .expect("the resume failed");
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        *data,
        "a halved range was put back at the wrong offset"
    );
}

/// A server that takes only three requests at once and answers 429 to the
/// rest, as rate-limited servers do: the connections it refuses step aside
/// and the download finishes on the ones it takes, rather than failing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_refuses_extra_connections_still_gets_the_file_whole() {
    use std::sync::atomic::AtomicUsize;
    let data = body();
    let busy = Arc::new(AtomicUsize::new(0));
    let refused = Arc::new(AtomicUsize::new(0));
    let (served, open, said_no) = (data.clone(), busy.clone(), refused.clone());
    let app = axum::Router::new().route(
        "/big.bin",
        get(move |headers: HeaderMap| {
            let (data, open, said_no) = (served.clone(), open.clone(), said_no.clone());
            async move {
                let range = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| v.split_once('-'))
                    .map(|(a, b)| {
                        let a: usize = a.parse().unwrap();
                        (a, b.parse::<usize>().unwrap_or(SIZE - 1).min(SIZE - 1))
                    });
                let Some((a, b)) = range else {
                    return Response::builder()
                        .status(200)
                        .header("content-length", SIZE)
                        .body(Body::from(data.to_vec()))
                        .unwrap();
                };
                if b > a && open.fetch_add(1, Ordering::SeqCst) >= 3 {
                    open.fetch_sub(1, Ordering::SeqCst);
                    said_no.fetch_add(1, Ordering::SeqCst);
                    return Response::builder()
                        .status(429)
                        .header("retry-after", "0")
                        .body(Body::from("Too Many Requests"))
                        .unwrap();
                }
                // Paced, and the slot given back when the body is done.
                const CHUNK: usize = 64 * 1024;
                let chunks: Vec<(usize, usize)> = (a..=b)
                    .step_by(CHUNK)
                    .map(|s| (s, (s + CHUNK - 1).min(b)))
                    .collect();
                let pause = Duration::from_secs_f64(CHUNK as f64 / FAST as f64);
                // Freed however the body ends, as a server frees a slot on a
                // disconnect: finished, or cut off when a range is halved.
                struct Slot(Arc<AtomicUsize>);
                impl Drop for Slot {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                let slot = (b > a).then(|| Arc::new(Slot(open.clone())));
                let stream = futures_util::stream::iter(chunks).then(move |(s, e)| {
                    let (data, _held) = (data.clone(), slot.clone());
                    async move {
                        tokio::time::sleep(pause).await;
                        Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&data[s..=e]))
                    }
                });
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                    .header("content-length", b - a + 1)
                    .header("etag", "\"big\"")
                    .body(Body::from_stream(stream))
                    .unwrap()
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/big.bin"),
        &options(dir.path()),
    )
    .await
    .expect("a server limiting connections failed the download");
    assert_eq!(std::fs::read(&done.path).unwrap(), *data, "not the file");
    let refused = refused.load(Ordering::SeqCst);
    assert!(
        refused > 0,
        "the server never refused, so this proved nothing"
    );
    println!("8 connections against a 3-at-once server: finished whole; {refused} requests refused with 429");
}

/// Under a speed limit the shown speed is the limit, from the first second
/// on. A person watching a 500 KB/s ceiling saw about 1 MB/s for
/// the first ten to twenty seconds of a start or a resume, and an ETA half
/// what it was, while the file grew at 500 KB/s.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_shown_speed_keeps_to_the_limit_from_the_first_second() {
    const LIMIT: u64 = 500 * 1024;
    const FILE: usize = 3 * 1024 * 1024;
    let data: Arc<Vec<u8>> = Arc::new((0..FILE).map(|i| i as u8).collect());
    let served = data.clone();
    let app = axum::Router::new().route(
        "/limited.bin",
        get(move |headers: HeaderMap| {
            let data = served.clone();
            async move {
                let (a, b) = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| v.split_once('-'))
                    .map(|(a, b)| {
                        let a: usize = a.parse().unwrap();
                        (a, b.parse::<usize>().unwrap_or(FILE - 1).min(FILE - 1))
                    })
                    .unwrap_or((0, FILE - 1));
                Response::builder()
                    .status(206)
                    .header("accept-ranges", "bytes")
                    .header("content-length", b - a + 1)
                    .header("content-range", format!("bytes {a}-{b}/{FILE}"))
                    .body(Body::from(data[a..=b].to_vec()))
                    .unwrap()
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let mut opts = options(dir.path());
    opts.limiter = dm_core::Limiter::new(LIMIT);

    let (tx, mut rx) = tokio::sync::mpsc::channel(1024);
    let started = Instant::now();
    let watching = tokio::spawn(async move {
        let mut seen = Vec::new();
        while let Some(update) = rx.recv().await {
            if let dm_core::Update::Progress(p) = update {
                seen.push((started.elapsed(), p.speed, p.downloaded));
            }
        }
        seen
    });
    let url = format!("http://{addr}/limited.bin");
    let done = dm_core::download_reporting(&client(), &url, &opts, Some(tx))
        .await
        .expect("the download failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
    let seen = watching.await.unwrap();

    let limit = LIMIT as f64;
    let late: Vec<_> = seen
        .iter()
        // After the first second, and before the last sample, which
        // reports however short a final interval was.
        .filter(|(at, _, downloaded)| *at > Duration::from_secs(1) && *downloaded < FILE as u64)
        .collect();
    assert!(late.len() > 5, "too few samples: {seen:?}");
    for (at, speed, _) in &late {
        assert!(
            (speed - limit).abs() <= limit * 0.1,
            "at {at:?} the speed shown was {:.0} KB/s under a {} KB/s limit: {seen:?}",
            speed / 1024.0,
            LIMIT / 1024
        );
    }
}

/// A connection that goes silent after its first chunk is dropped and asked
/// again once the others have shown it up, not waited out for the whole
/// read timeout (30 seconds).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connection_gone_silent_is_asked_again_before_the_read_timeout() {
    let data = body();
    let asked = Arc::new(AtomicUsize::new(0));
    let (served, counter) = (data.clone(), asked.clone());
    let app = axum::Router::new().route(
        "/big.bin",
        get(move |headers: HeaderMap| {
            let (data, counter) = (served.clone(), counter.clone());
            async move {
                let (a, b) = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| v.split_once('-'))
                    .map(|(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()))
                    .unwrap();
                let silent = b > a && counter.fetch_add(1, Ordering::SeqCst) == SLOW_REQUEST;
                let body = if silent {
                    // One chunk, then nothing for a minute.
                    let first = bytes::Bytes::copy_from_slice(&data[a..a + 64 * 1024]);
                    Body::from_stream(
                        futures_util::stream::once(async move { Ok::<_, std::io::Error>(first) })
                            .chain(futures_util::stream::once(async {
                                tokio::time::sleep(Duration::from_secs(60)).await;
                                Ok(bytes::Bytes::new())
                            })),
                    )
                } else {
                    Body::from(data[a..=b].to_vec())
                };
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                    .header("content-length", b - a + 1)
                    .header("etag", "\"big\"")
                    .body(body)
                    .unwrap()
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let started = Instant::now();
    let done = download(
        &client(),
        &format!("http://{addr}/big.bin"),
        &options(dir.path()),
    )
    .await
    .expect("the download failed");
    let took = started.elapsed();
    println!("32 MB, one connection silent: {:.2}s", took.as_secs_f64());
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
    assert!(
        took.as_secs_f64() < 15.0,
        "the silent connection was waited out: {:.2}s",
        took.as_secs_f64()
    );
}
