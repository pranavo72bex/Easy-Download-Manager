// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The seams between ranges fetched on several connections are fetched a
//! second time over a fresh request and compared, as reference A does; bytes
//! that differ restart the download on one connection.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use dm_core::{client, download};

mod common;
use common::{options, tempdir};

const MB: usize = 1024 * 1024;

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

#[derive(Clone)]
struct Site {
    body: Arc<Vec<u8>>,
    /// A small range (a re-check of a seam) is answered with other bytes.
    corrupt_checks: bool,
    /// How many small ranges were asked for.
    checks: Arc<AtomicUsize>,
}

async fn answer(State(site): State<Site>, headers: HeaderMap) -> Response {
    let total = site.body.len();
    let (a, b) = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| {
            (
                a.parse::<usize>().unwrap_or(0),
                b.parse::<usize>().unwrap_or(total - 1).min(total - 1),
            )
        })
        .unwrap_or((0, total - 1));
    let mut bytes = site.body[a..=b].to_vec();
    // A check of a seam is a few kilobytes from the middle of the file;
    // the pieces themselves are whole megabytes.
    if a > 0 && b - a < 8192 {
        site.checks.fetch_add(1, Ordering::SeqCst);
        if site.corrupt_checks {
            for byte in bytes.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }
    }
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("accept-ranges", "bytes")
        .header("content-range", format!("bytes {a}-{b}/{total}"))
        .header("content-length", b - a + 1)
        .header("etag", "\"s\"")
        .body(Body::from(bytes))
        .unwrap()
}

async fn serve(corrupt_checks: bool) -> (String, Arc<AtomicUsize>) {
    let checks = Arc::new(AtomicUsize::new(0));
    let site = Site {
        body: Arc::new(pattern(8 * MB)),
        corrupt_checks,
        checks: checks.clone(),
    };
    let app = axum::Router::new().fallback(answer).with_state(site);
    let addr = common::serve(app).await;
    (format!("http://{addr}/f.bin"), checks)
}

/// A server that, for any range request that starts at least 64 bytes into
/// the file, corrupts bytes at positions 10–60 of the returned slice but
/// leaves byte 0 intact.  The lead window fetched by a range-N request starts
/// at `seam - 64`; position 0 of the response is `seam - 64` (unchanged), but
/// positions 10–60 are corrupted.  The previous range wrote the honest bytes
/// at those same file offsets, so the assembly-time comparison catches the
/// disagreement.  A 1-byte check (position 0 only) would pass; the 64-byte
/// check catches the interior mismatch.
async fn serve_partial_corrupt() -> (String, Arc<AtomicUsize>) {
    let checks = Arc::new(AtomicUsize::new(0));
    let body = Arc::new(pattern(8 * MB));
    let checks_c = checks.clone();
    let body_c = body.clone();

    let app = axum::Router::new().fallback(move |headers: axum::http::HeaderMap| {
        let checks = checks_c.clone();
        let body = body_c.clone();
        async move {
            let total = body.len();
            let (a, b) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .map(|(a, b)| {
                    (
                        a.parse::<usize>().unwrap_or(0),
                        b.parse::<usize>().unwrap_or(total - 1).min(total - 1),
                    )
                })
                .unwrap_or((0, total - 1));
            let mut bytes = body[a..=b].to_vec();
            // Any range that starts 64+ bytes in is a lead-bearing request;
            // corrupt positions 10–60 (not 0, so a 1-byte check passes).
            if a >= 64 {
                checks.fetch_add(1, Ordering::SeqCst);
                for i in 10..60usize {
                    if let Some(byte) = bytes.get_mut(i) {
                        *byte = byte.wrapping_add(1);
                    }
                }
            }
            axum::response::Response::builder()
                .status(axum::http::StatusCode::PARTIAL_CONTENT)
                .header("accept-ranges", "bytes")
                .header("content-range", format!("bytes {a}-{b}/{total}"))
                .header("content-length", b - a + 1)
                .header("etag", "\"s\"")
                .body(axum::body::Body::from(bytes))
                .unwrap()
        }
    });
    let addr = common::serve(app).await;
    (format!("http://{addr}/f.bin"), checks)
}

#[tokio::test]
async fn honest_ranges_pass_the_second_look_and_stay_on_several_connections() {
    let (url, checks) = serve(false).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts).await.expect("download");
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(8 * MB));
    assert!(done.connections > 1, "{} connection", done.connections);
    // Every small range counts: the seams, and the last bytes of each finished
    // piece asked for again, of which a split makes more.
    let looked = checks.load(Ordering::SeqCst);
    assert!(
        (1..=24).contains(&looked),
        "{looked} seams were looked at again"
    );
}

/// A server of `pattern(size)` that records every closed range it is asked for.
async fn serve_recording(size: usize) -> (String, Arc<std::sync::Mutex<Vec<(usize, usize)>>>) {
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (body, seen) = (Arc::new(pattern(size)), asked.clone());
    let app = axum::Router::new().fallback(move |headers: HeaderMap| {
        let (body, seen) = (body.clone(), seen.clone());
        async move {
            let total = body.len();
            let (a, b) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .map(|(a, b)| {
                    let a = a.parse::<usize>().unwrap_or(0);
                    match b.parse::<usize>() {
                        Ok(b) => {
                            seen.lock().unwrap().push((a, b));
                            (a, b.min(total - 1))
                        }
                        Err(_) => (a, total - 1),
                    }
                })
                .unwrap_or((0, total - 1));
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("accept-ranges", "bytes")
                .header("content-range", format!("bytes {a}-{b}/{total}"))
                .header("content-length", b - a + 1)
                .header("etag", "\"s\"")
                .body(Body::from(body[a..=b].to_vec()))
                .unwrap()
        }
    });
    let addr = common::serve(app).await;
    (format!("http://{addr}/f.bin"), asked)
}

/// Each seam is asked for again once, in one request across it, and the file's end once: the
/// end of each range is looked at in the same request as the start of the next, where it was
/// asked for apart as well, and a seam was asked for again at the end besides (gaps row 177).
#[tokio::test]
async fn each_seam_is_asked_for_once_across_it_and_the_files_end_once() {
    let size = 8 * MB;
    let (url, asked) = serve_recording(size).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts).await.expect("download");
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(size));
    let asked = asked.lock().unwrap().clone();
    let (checks, ranges): (Vec<_>, Vec<_>) = asked.iter().copied().partition(|(a, b)| b - a < 2048);
    // Where each range begins: the probe's at 0, each other's 64 bytes after its request does.
    let mut starts: Vec<usize> = ranges.iter().map(|(a, _)| a + 64).collect();
    starts.sort_unstable();
    assert_eq!(
        checks.len(),
        starts.len() + 1,
        "not one check a seam and one for the end: {checks:?} for ranges starting at {starts:?}"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &(a, b) in &checks {
        assert!(b - a < 1024, "a check of {} bytes", b - a + 1);
        let at = starts.iter().find(|&&seam| a < seam && seam <= b);
        let looked_at = match at {
            Some(&seam) => seam,
            None => {
                assert_eq!(b, size - 1, "a check across no seam: {a}-{b}");
                size
            }
        };
        assert!(
            seen.insert(looked_at),
            "{looked_at} asked for twice: {checks:?}"
        );
    }
}

/// A server that leaves the seam boundary byte correct but corrupts bytes
/// 10–60 of the lead window. A 1-byte overlap would miss this; 64 bytes catch
/// it. This test verifies G96: dm fetches and compares LEAD_BYTES = 64 bytes.
#[tokio::test]
async fn a_corrupt_seam_interior_is_caught_by_the_64_byte_overlap() {
    let (url, checks) = serve_partial_corrupt().await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = dm_core::download(&dm_core::client(), &url, &opts)
        .await
        .expect("the download failed instead of going on with one connection");
    // Fell back to one connection because the seam bytes disagreed.
    assert_eq!(
        done.connections, 1,
        "it did not fall back to one connection when seam interior differed"
    );
    assert!(checks.load(Ordering::SeqCst) >= 1, "no seam check was made");
}

#[tokio::test]
async fn seams_that_differ_when_asked_for_again_restart_the_file_on_one_connection() {
    let (url, checks) = serve(true).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let seen = tokio::spawn(async move {
        let mut said = Vec::new();
        while let Some(update) = rx.recv().await {
            if let dm_core::Update::Retrying { why, .. } = update {
                said.push(why);
            }
        }
        said
    });
    let done = dm_core::download_reporting(&client(), &url, &opts, Some(tx))
        .await
        .expect("the download failed instead of going on with one connection");
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        pattern(8 * MB),
        "the file is not the one served"
    );
    assert_eq!(
        done.connections, 1,
        "it did not fall back to one connection"
    );
    assert!(checks.load(Ordering::SeqCst) >= 1);
    let said = seen.await.unwrap();
    assert!(
        said.iter().any(|why| why.contains("did not agree")),
        "the row was not told why: {said:?}"
    );
}
