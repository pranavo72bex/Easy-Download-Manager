//! How fast the engine moves bytes when nothing else holds it back.
//!
//! `speed.rs` asks whether one slow connection holds a download up; this asks
//! what the engine itself costs: a loopback server with no pacing at all, so
//! the time is the engine's (requests, buffers, writes, locks), and a server
//! that waits before each answer, as a distant one does, so the time to the
//! first byte shows. Ignored by default: the numbers are for a person
//! comparing two builds on one machine, not a pass or a fail.
//!
//!     cargo test -p dm-core --release --test throughput -- --ignored --nocapture --test-threads=1

mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use dm_core::{client, download};

use common::{options, serve, tempdir};

/// The file, made once: a pattern no range can mistake for another.
fn body(size: usize) -> Bytes {
    Bytes::from(
        (0..size)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add((i >> 16) as u8))
            .collect::<Vec<u8>>(),
    )
}

/// Serves `data` with ranges as fast as the socket takes it, each answer
/// held back `delay` first (a round trip to a distant server).
async fn server(data: Bytes, delay: Duration) -> std::net::SocketAddr {
    let app = axum::Router::new().route(
        "/file.bin",
        get(move |headers: HeaderMap| {
            let data = data.clone();
            async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                let size = data.len();
                let range = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| v.split_once('-'))
                    .map(|(a, b)| {
                        let a: usize = a.parse().unwrap();
                        let b = b.parse::<usize>().unwrap_or(size - 1).min(size - 1);
                        (a, b)
                    });
                let Some((a, b)) = range else {
                    return Response::builder()
                        .status(200)
                        .header("content-length", size)
                        .header("etag", "\"t\"")
                        .body(Body::from(data))
                        .unwrap();
                };
                // Sent in 256 KiB pieces, as a socket would hand them over.
                let piece = data.slice(a..=b);
                let chunks: Vec<Bytes> = (0..piece.len())
                    .step_by(256 * 1024)
                    .map(|s| piece.slice(s..(s + 256 * 1024).min(piece.len())))
                    .collect();
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{size}"))
                    .header("content-length", b - a + 1)
                    .header("etag", "\"t\"")
                    .body(Body::from_stream(futures_util::stream::iter(
                        chunks.into_iter().map(Ok::<_, std::io::Error>),
                    )))
                    .unwrap()
            }
        }),
    );
    serve(app).await
}

/// A TCP relay in front of `upstream` that behaves like a distant server:
/// every byte arrives `rtt / 2` late each way, and a new connection carries
/// nothing for one round trip, as a TCP handshake costs.
async fn distant(upstream: std::net::SocketAddr, rtt: Duration, cap: u64) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((client, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                tokio::time::sleep(rtt).await;
                let Ok(server) = tokio::net::TcpStream::connect(upstream).await else {
                    return;
                };
                let _ = client.set_nodelay(true);
                let _ = server.set_nodelay(true);
                let (cr, cw) = client.into_split();
                let (sr, sw) = server.into_split();
                tokio::join!(delayed(cr, sw, rtt / 2, 0), delayed(sr, cw, rtt / 2, cap));
            });
        }
    });
    addr
}

/// Copies `from` to `to`, each piece `by` after it was read, and no faster
/// than `cap` bytes a second where `cap` is not 0 (a server that limits each
/// connection, which is why a download manager opens several).
async fn delayed(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    by: Duration,
    cap: u64,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(tokio::time::Instant, Vec<u8>)>();
    let reading = async move {
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            match from.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    if tx
                        .send((tokio::time::Instant::now() + by, buf[..n].to_vec()))
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    };
    let writing = async move {
        let started = tokio::time::Instant::now();
        let mut sent = 0u64;
        while let Some((at, piece)) = rx.recv().await {
            tokio::time::sleep_until(at).await;
            if cap > 0 {
                sent += piece.len() as u64;
                tokio::time::sleep_until(
                    started + Duration::from_secs_f64(sent as f64 / cap as f64),
                )
                .await;
            }
            if to.write_all(&piece).await.is_err() {
                return;
            }
        }
        let _ = to.shutdown().await;
    };
    tokio::join!(reading, writing);
}

async fn run(label: &str, size: usize, connections: usize, delay: Duration) {
    run_at(label, size, connections, delay, Duration::ZERO, 0).await;
}

async fn run_at(
    label: &str,
    size: usize,
    connections: usize,
    delay: Duration,
    rtt: Duration,
    cap: u64,
) {
    let data = body(size);
    let addr = server(data.clone(), delay).await;
    let addr = if rtt.is_zero() {
        addr
    } else {
        distant(addr, rtt, cap).await
    };
    let dir = tempdir();
    let url = format!("http://{addr}/file.bin");
    let opts = dm_core::Options {
        connections,
        ..options(dir.path())
    };
    let cpu = cpu_seconds();
    let started = Instant::now();
    let done = download(&client(), &url, &opts)
        .await
        .expect("the download failed");
    let took = started.elapsed().as_secs_f64();
    let cpu = cpu_seconds() - cpu;
    let got = std::fs::read(&done.path).unwrap();
    assert!(got == data[..], "the file is not the file");
    println!(
        "{label:<34} {:>7.0} MB/s  {took:>6.3}s  cpu {cpu:>6.2}s",
        size as f64 / took / 1e6
    );
}

/// This process's user and system time, in seconds.
fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map(|(_, rest)| rest.split_whitespace().collect())
        .unwrap_or_default();
    let tick = |i: usize| {
        fields
            .get(i)
            .and_then(|f| f.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    (tick(11) + tick(12)) / 100.0
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "a benchmark: run by hand"]
async fn throughput() {
    const MB: usize = 1024 * 1024;
    // `DM_BENCH_SMALL=1`: only the small files, which take seconds, not the
    // half-gigabyte ones, which take minutes in a debug build.
    if std::env::var_os("DM_BENCH_SMALL").is_none() {
        run("512 MB, 1 connection", 512 * MB, 1, Duration::ZERO).await;
        run("512 MB, 8 connections", 512 * MB, 8, Duration::ZERO).await;
        run("512 MB, 16 connections", 512 * MB, 16, Duration::ZERO).await;
        run(
            "4 MB, 8 conn, 100 ms server",
            4 * MB,
            8,
            Duration::from_millis(100),
        )
        .await;
        run(
            "64 KB, 8 conn, 100 ms server",
            64 * 1024,
            8,
            Duration::from_millis(100),
        )
        .await;
    }
    let rtt = Duration::from_millis(80);
    run_at(
        "64 KB, 8 conn, 80 ms RTT",
        64 * 1024,
        8,
        Duration::ZERO,
        rtt,
        0,
    )
    .await;
    run_at("1 MB, 1 conn, 80 ms RTT", MB, 1, Duration::ZERO, rtt, 0).await;
    run_at("8 MB, 8 conn, 80 ms RTT", 8 * MB, 8, Duration::ZERO, rtt, 0).await;
    run_at(
        "64 MB, 8 conn, 80 ms RTT",
        64 * MB,
        8,
        Duration::ZERO,
        rtt,
        0,
    )
    .await;
    // A server that holds each connection to 1 MB/s, 40 ms away.
    let (near, cap) = (Duration::from_millis(40), 1024 * 1024);
    run_at(
        "768 KB, 8 conn, 1 MB/s each",
        768 * 1024,
        8,
        Duration::ZERO,
        near,
        cap,
    )
    .await;
    run_at(
        "1.5 MB, 8 conn, 1 MB/s each",
        3 * MB / 2,
        8,
        Duration::ZERO,
        near,
        cap,
    )
    .await;
    run_at(
        "3 MB, 8 conn, 1 MB/s each",
        3 * MB,
        8,
        Duration::ZERO,
        near,
        cap,
    )
    .await;
    run_at(
        "6 MB, 8 conn, 1 MB/s each",
        6 * MB,
        8,
        Duration::ZERO,
        near,
        cap,
    )
    .await;
    run_at(
        "40 MB, 8 conn, 1 MB/s each",
        40 * MB,
        8,
        Duration::ZERO,
        near,
        cap,
    )
    .await;
    // The other side of the same choice: a distant server that holds no
    // connection down. Splitting a small file there only costs a round trip.
    for (label, size) in [
        ("512 KB", MB / 2),
        ("1.5 MB", 3 * MB / 2),
        ("3 MB", 3 * MB),
        ("6 MB", 6 * MB),
    ] {
        run_at(
            &format!("{label}, 8 conn, 80 ms RTT, no cap"),
            size,
            8,
            Duration::ZERO,
            rtt,
            0,
        )
        .await;
    }
}

/// A video page through the helper: yt-dlp downloading the stream itself,
/// against yt-dlp reading the page and dm fetching it (`download_page`),
/// from a server holding each connection to 1 MB/s, 40 ms away. Needs the
/// real yt-dlp, which reads a plain file as a page with one format.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a benchmark: run by hand"]
async fn video_helper() {
    if !dm_core::ytdlp::usable() {
        eprintln!("no yt-dlp: skipped");
        return;
    }
    const MB: usize = 1024 * 1024;
    for size in [8 * MB, 32 * MB] {
        let data = body(size);
        let addr = server(data.clone(), Duration::ZERO).await;
        let addr = distant(addr, Duration::from_millis(40), MB as u64).await;
        let url = format!("http://{addr}/file.bin");
        for own in [false, true] {
            let dir = tempdir();
            let opts = options(dir.path());
            let started = Instant::now();
            let done = if own {
                dm_core::ytdlp::download_page(&client(), &url, &opts, None).await
            } else {
                dm_core::ytdlp::download_ytdlp(&url, &opts, None).await
            }
            .expect("the download failed");
            let took = started.elapsed().as_secs_f64();
            assert!(std::fs::read(&done.path).unwrap() == data[..]);
            println!(
                "{:>3} MB, {:<28} {took:>6.2}s  {:>5.1} MB/s",
                size / MB,
                if own {
                    "yt-dlp reads, dm fetches"
                } else {
                    "yt-dlp fetches"
                },
                size as f64 / took / 1e6
            );
        }
    }
}
