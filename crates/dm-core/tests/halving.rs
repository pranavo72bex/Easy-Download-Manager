// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! When a connection with nothing left to fetch halves a range still under way, and when it
//! leaves it be. A halving costs a request, and the range's own connection, stopped short of the
//! end it asked for, drops whatever the server had already sent past the new end: on a fast link
//! that is megabytes fetched twice. So a range is halved only where its own connection would take
//! longer than a new request is worth, judged at the pace it is going, and never for an answer
//! that is merely on its way (gaps rows 171 and 177).

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, download};
use futures_util::StreamExt;

use common::{options, serve, tempdir};

const MB: usize = 1024 * 1024;

fn body(n: usize) -> Arc<Vec<u8>> {
    Arc::new(
        (0..n)
            .map(|i| (i as u8).wrapping_mul(29).wrapping_add(3))
            .collect(),
    )
}

/// The range a request asks for, if it names its end: the probe's `bytes=0-` does not.
fn closed(headers: &HeaderMap, total: usize) -> Option<(usize, usize)> {
    let (a, b) = headers
        .get("range")?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?
        .split_once('-')?;
    Some((a.parse().ok()?, b.parse::<usize>().ok()?.min(total - 1)))
}

/// A server of `data` under one ETag, answering a range that starts at `late` after `delay`,
/// and every other at once; with what each request asked for, in the order they came.
async fn server(
    data: Arc<Vec<u8>>,
    late: Option<usize>,
    delay: Duration,
) -> (String, Arc<Mutex<Vec<(usize, usize)>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |headers: HeaderMap| {
            let (data, seen) = (data.clone(), seen.clone());
            async move {
                let total = data.len();
                let range = closed(&headers, total);
                // Not the probe's, which asks with no `If-Range` (it learns the validator): on
                // several connections it names its end too, the file's first part.
                if let Some(range) = range.filter(|_| headers.contains_key("if-range")) {
                    seen.lock().unwrap().push(range);
                }
                let (a, b) = range.unwrap_or((0, total - 1));
                if late == Some(a) {
                    tokio::time::sleep(delay).await;
                }
                let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = data[a..=b]
                    .chunks(64 * 1024)
                    .map(|c| Ok(bytes::Bytes::copy_from_slice(c)))
                    .collect();
                Response::builder()
                    .status(206)
                    .header("accept-ranges", "bytes")
                    .header("etag", "\"h\"")
                    .header("content-range", format!("bytes {a}-{b}/{total}"))
                    .header("content-length", b - a + 1)
                    .body(Body::from_stream(futures_util::stream::iter(chunks)))
                    .unwrap()
            }
        }),
    );
    (format!("http://{}/f.bin", serve(app).await), asked)
}

/// The ranges fetched (not the checks of a seam), as asked: each after the first starts 64
/// bytes before its own first byte, which the range before it holds (`LEAD_BYTES`).
fn ranges(asked: &[(usize, usize)]) -> Vec<(usize, usize)> {
    asked
        .iter()
        .copied()
        .filter(|(a, b)| b - a + 1 > 2 * 1024)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ranges_about_to_end_are_not_halved() {
    let data = body(4 * MB);
    let (url, asked) = server(data.clone(), None, Duration::ZERO).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts)
        .await
        .expect("the download failed");
    assert!(
        std::fs::read(&done.path).unwrap() == *data,
        "not the file served"
    );
    // Four ranges of a megabyte, the first on the probe's answer: three asked for, and none
    // halved, each being over in milliseconds at the pace it went.
    let fetched = ranges(&asked.lock().unwrap());
    assert_eq!(
        fetched.len(),
        3,
        "ranges were halved however soon they would end: {fetched:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_range_whose_answer_is_on_its_way_is_not_halved_for_it() {
    let data = body(4 * MB);
    // The third range's answer comes 300 ms after it is asked for; the others' at once.
    let third = 2 * MB - 64;
    let (url, asked) = server(data.clone(), Some(third), Duration::from_millis(300)).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts)
        .await
        .expect("the download failed");
    assert!(
        std::fs::read(&done.path).unwrap() == *data,
        "not the file served"
    );
    // Its connection was waiting on a busy server, which a new request would wait on as well:
    // the free connections left it be, and it fetched its range whole when its answer came.
    let fetched = ranges(&asked.lock().unwrap());
    assert_eq!(
        fetched.len(),
        3,
        "a range waiting for its answer was halved: {fetched:?}"
    );
    assert!(fetched.contains(&(third, 3 * MB - 1)), "{fetched:?}");
}

/// A server of `data` whose first answer to reach the file's last byte sends its first 16 KiB,
/// then nothing for 20 s, its connection left open; every other answer at once.
async fn stalling_at_the_end(data: Arc<Vec<u8>>) -> String {
    let stalled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |headers: HeaderMap| {
            let (data, stalled) = (data.clone(), stalled.clone());
            async move {
                let total = data.len();
                let (a, b) = closed(&headers, total).unwrap_or((0, total - 1));
                let stall = a > 0
                    && b == total - 1
                    && !stalled.swap(true, std::sync::atomic::Ordering::SeqCst);
                let chunks: Vec<bytes::Bytes> = data[a..=b]
                    .chunks(16 * 1024)
                    .map(bytes::Bytes::copy_from_slice)
                    .collect();
                let sent = std::sync::atomic::AtomicUsize::new(a);
                let stream = futures_util::stream::iter(chunks).then(move |chunk| {
                    let at = sent.fetch_add(chunk.len(), std::sync::atomic::Ordering::SeqCst);
                    async move {
                        if stall && at > a {
                            tokio::time::sleep(Duration::from_secs(20)).await;
                        }
                        Ok::<_, std::io::Error>(chunk)
                    }
                });
                Response::builder()
                    .status(206)
                    .header("accept-ranges", "bytes")
                    .header("etag", "\"h\"")
                    .header("content-range", format!("bytes {a}-{b}/{total}"))
                    .header("content-length", b - a + 1)
                    .body(Body::from_stream(stream))
                    .unwrap()
            }
        }),
    );
    format!("http://{}/f.bin", serve(app).await)
}

/// The last connection still fetching goes quiet while the others have ended: it is dropped
/// and its range asked again within moments, not waited on for the stall window's 5 s or the
/// server's 20 (gaps row K1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_range_stalled_at_the_end_is_asked_again_within_moments() {
    let data = body(4 * MB);
    let url = stalling_at_the_end(data.clone()).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let began = std::time::Instant::now();
    let done = download(&client(), &url, &opts)
        .await
        .expect("the download failed");
    let took = began.elapsed();
    assert!(
        std::fs::read(&done.path).unwrap() == *data,
        "not the file served"
    );
    // It stalled: the others did wait on it, a moment.
    assert!(took > Duration::from_secs(1), "nothing stalled: {took:?}");
    assert!(
        took < dm_test_support::load::allowing_for_load(Duration::from_millis(4_000)),
        "the others waited on the stalled range: {took:?}"
    );
}
