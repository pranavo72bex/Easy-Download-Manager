// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A slow disk slows the download and does not grow dm's memory (gaps row 185): the parts'
//! writes held to 1 MB/s, eight connections to a server that would send 50 MB/s. A connection
//! reads from the network only once its last piece is written, so what dm holds stays where it
//! was once the connections are up, and the server is held back by the network itself. A test
//! binary of its own: resident memory is the whole process's.

mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use dm_core::{client, download_reporting, Update};

use common::{options, serve, tempdir};

/// Larger than the test lets through at the disk's pace, so it is never whole.
const SIZE: usize = 64 << 20;
/// The disk's pace, all connections together.
const DISK: u64 = 1 << 20;
/// What the server sends a connection at a time, and how often: about 6.4 MB/s each, 50 MB/s
/// over eight.
const PIECE: usize = 64 * 1024;
const PACE: Duration = Duration::from_millis(10);

async fn server() -> String {
    let data: Arc<Vec<u8>> = Arc::new((0..SIZE).map(|i| (i % 253) as u8).collect());
    let app = axum::Router::new().fallback(move |headers: HeaderMap| {
        let data = data.clone();
        async move {
            let (a, b) = headers
                .get("range")
                .and_then(|r| r.to_str().ok())
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.split_once('-'))
                .map(|(a, b)| {
                    let a = a.parse::<usize>().unwrap_or(0);
                    let b = b.parse::<usize>().unwrap_or(SIZE - 1).min(SIZE - 1);
                    (a, b)
                })
                .unwrap_or((0, SIZE - 1));
            // Each piece cut as it is sent: the server's own memory stays flat too.
            let stream = futures_util::stream::unfold((a, data), move |(at, data)| async move {
                if at > b {
                    return None;
                }
                let end = (at + PIECE).min(b + 1);
                tokio::time::sleep(PACE).await;
                let piece = bytes::Bytes::copy_from_slice(&data[at..end]);
                Some((Ok::<_, std::io::Error>(piece), (end, data)))
            });
            Response::builder()
                .status(206)
                .header("accept-ranges", "bytes")
                .header("etag", "\"slow\"")
                .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                .header("content-length", b - a + 1)
                .body(Body::from_stream(stream))
                .unwrap()
        }
    });
    format!("http://{}", serve(app).await)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_disk_slows_the_download_and_does_not_grow_its_memory() {
    let base = server().await;
    let dir = tempdir();
    let options = dm_core::Options {
        connections: 8,
        ..options(dir.path())
    };
    dm_core::slow_part_writes_to(DISK).await;
    let fetched = Arc::new(AtomicU64::new(0));
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let told = fetched.clone();
    tokio::spawn(async move {
        while let Some(update) = rx.recv().await {
            if let Update::Progress(progress) = update {
                told.store(progress.downloaded, Ordering::SeqCst);
            }
        }
    });
    let control = options.control.clone();
    let running = {
        let url = format!("{base}/big.bin");
        tokio::spawn(async move { download_reporting(&client(), &url, &options, Some(tx)).await })
    };
    let resident = || memory_stats::memory_stats().unwrap().physical_mem as i64;

    // The connections up and writing.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let settled = resident();
    let began = (Instant::now(), fetched.load(Ordering::SeqCst));
    let mut most = settled;
    while began.0.elapsed() < Duration::from_secs(6) {
        tokio::time::sleep(Duration::from_millis(250)).await;
        most = most.max(resident());
    }
    let took = began.0.elapsed().as_secs_f64();
    let written = fetched.load(Ordering::SeqCst) - began.1;
    control.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(10), running).await;
    dm_core::slow_part_writes_to(0).await;

    let grew = (most - settled) / 1024;
    println!(
        "resident {} KB after 2 s, at most {grew} KB more in {took:.1} s; {written} bytes \
         written meanwhile",
        settled / 1024
    );
    assert!(
        grew < 10 * 1024,
        "dm's memory grew {grew} KB while its disk held it back"
    );
    // Held to the disk's pace, give or take a second of it, where the server would have sent
    // the whole file in a second and a half.
    assert!(
        (written as f64) < (took + 1.0) * DISK as f64,
        "{written} bytes in {took:.1} s, past a disk of {DISK} bytes a second"
    );
    assert!(written > 0, "nothing was written");
}
