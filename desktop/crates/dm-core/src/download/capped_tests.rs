//! A capped request (`StreamHints::chunk`) against servers that throttle or
//! cut any one response: the file must come out byte for byte, and faster
//! where the throttle is per response.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use futures_util::StreamExt;

use super::*;

const SIZE: usize = 16 * 1024 * 1024;
const CAP: u64 = 1024 * 1024;

fn data() -> Arc<Vec<u8>> {
    Arc::new(
        (0..SIZE)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(i as u8 >> 3))
            .collect(),
    )
}

fn options(dir: &Path) -> Options {
    Options {
        connections: 4,
        control: Control::new(),
        dest_dir: dir.join("out"),
        parts_dir: dir.join("parts"),
        limiter: Limiter::unlimited(),
        room: Default::default(),
        categories: Categories::default(),
        checksum: None,
        quality: crate::stream::Quality::Best,
        sound: None,
        span: None,
        media: false,
        referrer: None,
        proxy: None,
        container: None,
        attempts: 5,
        host_limits: Vec::new(),
        proxy_bypass: Vec::new(),
        subtitles: false,
        sign_in: None,
        save_name: None,
        system_proxy: false,
        remux: false,
        server_dates: false,
        retry_wait: 0,
        record_limit: None,
    }
}

/// How the server misbehaves.
#[derive(Clone, Copy)]
enum Fault {
    None,
    /// Every response is fast for its first megabyte, then crawls.
    ThrottleAfterFirstMegabyte,
    /// The second capped answer ends early, short of what it promised.
    OneShortAnswer,
    /// Ranges are not served: the whole file, every time.
    NoRanges,
    /// Two ranges are served at once; any more are taken and never
    /// answered, as a server that holds connections past its limit does.
    SilentPastTwo,
}

struct Server {
    addr: std::net::SocketAddr,
    /// Every range asked for beyond the probe.
    asked: Arc<Mutex<Vec<(usize, usize)>>>,
}

fn paced(data: Arc<Vec<u8>>, a: usize, b: usize, throttled: bool) -> Body {
    const PIECE: usize = 64 * 1024;
    let pieces: Vec<(usize, usize)> = (a..=b)
        .step_by(PIECE)
        .map(|s| (s, (s + PIECE - 1).min(b)))
        .collect();
    Body::from_stream(futures_util::stream::iter(pieces).then(move |(s, e)| {
        let data = data.clone();
        async move {
            let slow = throttled && s - a >= 1024 * 1024;
            tokio::time::sleep(Duration::from_millis(if slow { 125 } else { 3 })).await;
            Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&data[s..=e]))
        }
    }))
}

async fn server(data: Arc<Vec<u8>>, fault: Fault) -> Server {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let big = Arc::new(AtomicUsize::new(0));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |headers: HeaderMap| {
            let (data, asked, big) = (data.clone(), seen.clone(), big.clone());
            let in_flight = in_flight.clone();
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
                let whole = || {
                    Response::builder()
                        .status(200)
                        .header("content-length", SIZE)
                        .header("etag", "\"f\"")
                        .body(Body::from(data.to_vec()))
                        .unwrap()
                };
                let Some((a, b)) = range.filter(|_| !matches!(fault, Fault::NoRanges)) else {
                    return whole();
                };
                if b > a {
                    asked.lock().unwrap().push((a, b));
                }
                let n = if b > a {
                    big.fetch_add(1, Ordering::SeqCst)
                } else {
                    0
                };
                // Past two at once: taken, and never answered.
                if matches!(fault, Fault::SilentPastTwo) && b > a {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst);
                    if now >= 2 {
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_secs(600)).await;
                        return Response::builder().status(500).body(Body::empty()).unwrap();
                    }
                    // The slot is given back when the response is dropped,
                    // whether the client read it whole or hung up.
                    struct Slot(Arc<AtomicUsize>);
                    impl Drop for Slot {
                        fn drop(&mut self) {
                            self.0.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    let slot = Slot(in_flight.clone());
                    let body =
                        Body::from_stream(paced(data.clone(), a, b, false).into_data_stream().map(
                            move |chunk| {
                                let _held = &slot;
                                chunk
                            },
                        ));
                    return Response::builder()
                        .status(206)
                        .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                        .header("content-length", b - a + 1)
                        .header("etag", "\"f\"")
                        .body(body)
                        .unwrap();
                }
                // Short: promises everything, sends half, and ends.
                let cut = matches!(fault, Fault::OneShortAnswer) && n == 2 && b - a > 1024;
                let sent = if cut { a + (b - a) / 2 } else { b };
                let throttled = matches!(fault, Fault::ThrottleAfterFirstMegabyte);
                let mut res = Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                    .header("etag", "\"f\"");
                if !cut {
                    res = res.header("content-length", b - a + 1);
                }
                res.body(paced(data, a, sent, throttled)).unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server { addr, asked }
}

async fn fetch(server: &Server, chunk: Option<u64>, options: &Options) -> (Vec<u8>, Duration) {
    let url = format!("http://{}/f.bin", server.addr);
    let started = Instant::now();
    let done = download_video_stream_with(
        &crate::client(),
        &url,
        options,
        None,
        StreamHints {
            chunk,
            ..Default::default()
        },
    )
    .await
    .expect("the download failed");
    (std::fs::read(done.path).unwrap(), started.elapsed())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capped_requests_assemble_the_same_file_in_slices() {
    let data = data();
    let server = server(data.clone(), Fault::None).await;
    let dir = tempfile::tempdir().unwrap();
    let (got, _) = fetch(&server, Some(CAP), &options(dir.path())).await;
    assert!(got == *data, "the capped file is not the file");
    let asked = server.asked.lock().unwrap();
    assert!(
        asked
            .iter()
            .all(|&(a, b)| (b - a) as u64 <= CAP + RESUME_CHECK),
        "a request asked for more than the cap: {asked:?}"
    );
    assert!(
        asked.len() >= SIZE / CAP as usize,
        "{} requests",
        asked.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_cap_asks_as_it_always_did() {
    let data = data();
    let server = server(data.clone(), Fault::None).await;
    let dir = tempfile::tempdir().unwrap();
    let (got, _) = fetch(&server, None, &options(dir.path())).await;
    assert!(got == *data);
    assert!(
        server.asked.lock().unwrap().len() <= 4 + 16,
        "split far more than before"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cap_smaller_than_the_floor_is_raised_to_it() {
    let data = data();
    let server = server(data.clone(), Fault::None).await;
    let dir = tempfile::tempdir().unwrap();
    // 100 bytes: below the resume check, which would never move forward.
    let (got, _) = fetch(&server, Some(100), &options(dir.path())).await;
    assert!(got == *data);
    // Slices of 100 bytes would be a hundred thousand requests; the floor
    // keeps it to about one a megabyte, and a few more for tails and splits.
    let asked = server.asked.lock().unwrap().len();
    assert!(asked <= 3 * SIZE / MIN_CHUNK as usize, "{asked} requests");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_answers_the_whole_file_to_a_capped_range_still_gives_the_file() {
    let data = data();
    let server = server(data.clone(), Fault::NoRanges).await;
    let dir = tempfile::tempdir().unwrap();
    let (got, _) = fetch(&server, Some(CAP), &options(dir.path())).await;
    assert!(got == *data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_short_answer_under_a_cap_is_resumed_not_spliced() {
    let data = data();
    let server = server(data.clone(), Fault::OneShortAnswer).await;
    let dir = tempfile::tempdir().unwrap();
    let (got, _) = fetch(&server, Some(CAP), &options(dir.path())).await;
    assert!(got == *data, "a short slice was kept as whole");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_capped_download_resumes_byte_exact() {
    let data = data();
    let server = server(data.clone(), Fault::ThrottleAfterFirstMegabyte).await;
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path());
    let url = format!("http://{}/f.bin", server.addr);
    let (c, o) = (crate::client(), opts.clone());
    let first = tokio::spawn(async move {
        download_video_stream_with(
            &c,
            &url,
            &o,
            None,
            StreamHints {
                chunk: Some(CAP),
                ..Default::default()
            },
        )
        .await
    });
    // Paused once ranges are being fetched, not after a guess at how long
    // that takes to begin.
    for _ in 0..400 {
        if server.asked.lock().unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    opts.control.pause();
    let first = first.await.unwrap();
    if first.is_ok() {
        return; // finished before the pause landed
    }
    let again = Options {
        control: Control::new(),
        ..opts
    };
    let (got, _) = fetch(&server, Some(CAP), &again).await;
    assert!(got == *data, "the resume spliced the file wrong");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cap_beats_a_server_that_throttles_every_response() {
    let data = data();
    let server = server(data.clone(), Fault::ThrottleAfterFirstMegabyte).await;
    let (plain_dir, capped_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (plain, without) = fetch(&server, None, &options(plain_dir.path())).await;
    let (capped, with) = fetch(&server, Some(CAP), &options(capped_dir.path())).await;
    assert!(plain == *data && capped == *data);
    println!(
        "16 MiB, 4 connections, each response slows after 1 MiB: no cap {:.2}s, cap {:.2}s",
        without.as_secs_f64(),
        with.as_secs_f64()
    );
    assert!(
        with < without,
        "the cap did not help: {with:?} vs {without:?}"
    );
}

/// A server that serves two connections and holds every further one open
/// without a byte: the held ones step aside at their read timeout, their
/// ranges go to the two being served, and the file comes whole in seconds
/// rather than failing after every held connection's tries.
#[tokio::test(flavor = "multi_thread")]
async fn connections_a_server_holds_unanswered_step_aside_for_the_served() {
    let data = data();
    let server = server(data.clone(), Fault::SilentPastTwo).await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = options(dir.path());
    options.connections = 6;
    options.attempts = 3;
    // Two seconds, not the thirty of the default: the held connections are
    // found out that much sooner.
    let client = crate::builder_with(Duration::from_secs(2), false)
        .build()
        .unwrap();
    let url = format!("http://{}/f.bin", server.addr);
    let started = Instant::now();
    let done = download(&client, &url, &options).await.expect("the file");
    let took = started.elapsed();
    assert_eq!(std::fs::read(&done.path).unwrap(), *data, "byte for byte");
    assert!(
        took < Duration::from_secs(12),
        "the held connections were waited out: {took:?}"
    );
    let _ = server.asked;
}
