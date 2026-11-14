// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm ending while twenty downloads run (gaps row 186): each is stopped as
//! a pause stops it and awaited, within two seconds, and put back in line;
//! the next start finishes every file byte for byte, and fetches again
//! little more than what was on its way when each connection let go.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use dm_core::{client, Queue, State};

use common::{serve, tempdir};

/// Each file's size: long enough at the server's pace that every download
/// is still running when dm ends.
const SIZE: usize = 2 * 1024 * 1024;
/// The downloads running at once.
const DOWNLOADS: usize = 20;
/// Connections per download.
const CONNECTIONS: usize = 2;
/// What the server sends at a time, and how long it waits between: about
/// 400 KB a second a connection.
const PIECE: usize = 8 * 1024;
const PACE: Duration = Duration::from_millis(20);

fn body() -> Arc<Vec<u8>> {
    Arc::new(
        (0..SIZE)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
            .collect(),
    )
}

fn range(headers: &HeaderMap) -> Option<(usize, usize)> {
    let (a, b) = headers
        .get("range")?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?
        .split_once('-')?;
    let a = a.parse().ok()?;
    let b = if b.is_empty() {
        SIZE - 1
    } else {
        b.parse::<usize>().ok()?.min(SIZE - 1)
    };
    Some((a, b))
}

/// A server of one file under many names, paced, counting the bytes it has
/// handed to the connection.
async fn server(data: Arc<Vec<u8>>, sent: Arc<AtomicUsize>) -> String {
    // A fallback, not a routed path: every name is the one file.
    let app = axum::Router::new().fallback(move |headers: HeaderMap| {
        let (data, sent) = (data.clone(), sent.clone());
        async move {
            let (a, b) = range(&headers).unwrap_or((0, SIZE - 1));
            let pieces: Vec<bytes::Bytes> = data[a..=b]
                .chunks(PIECE)
                .map(bytes::Bytes::copy_from_slice)
                .collect();
            let stream = futures_util::stream::unfold(
                (pieces.into_iter(), sent),
                |(mut pieces, sent)| async move {
                    let piece = pieces.next()?;
                    tokio::time::sleep(PACE).await;
                    sent.fetch_add(piece.len(), Ordering::SeqCst);
                    Some((Ok::<_, std::io::Error>(piece), (pieces, sent)))
                },
            );
            Response::builder()
                .status(206)
                .header("accept-ranges", "bytes")
                .header("etag", "\"same\"")
                .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                .header("content-length", b - a + 1)
                .body(Body::from_stream(stream))
                .unwrap()
        }
    });
    format!("http://{}", serve(app).await)
}

fn queue(dir: &std::path::Path) -> Queue {
    Queue::new(
        client(),
        dm_core::Options {
            connections: CONNECTIONS,
            ..common::options(dir)
        },
        DOWNLOADS,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quitting_with_twenty_downloads_running_stops_them_within_two_seconds_and_they_resume_whole(
) {
    let data = body();
    let sent = Arc::new(AtomicUsize::new(0));
    let base = server(data.clone(), sent.clone()).await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");

    let first = queue(dir.path());
    first.persist_to(store.clone()).await;
    let mut ids = Vec::new();
    for i in 0..DOWNLOADS {
        ids.push(first.add(&format!("{base}/f{i}.bin")).await.unwrap());
    }
    let running = first.clone();
    tokio::spawn(async move { running.run().await });
    // Every one under way, none near its end.
    let started = Instant::now();
    loop {
        let rows = first.list().await;
        let moving = rows
            .iter()
            .filter(|r| r.state == State::Running && r.progress.downloaded > 0)
            .count();
        if moving == DOWNLOADS {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "only {moving} of {DOWNLOADS} got going"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    // What Exit and SIGTERM do.
    let asked = Instant::now();
    let left = first.shut_down(Duration::from_secs(2)).await;
    let took = asked.elapsed();
    // What each row was doing when the deadline came, said with the failure:
    // on Windows CI all 20 were once still going after 2 s (2026-11-14).
    let at_deadline: Vec<_> = first
        .list()
        .await
        .iter()
        .map(|r| (r.state.clone(), r.progress.downloaded))
        .collect();
    assert_eq!(
        left, 0,
        "{left} runs were still going at the deadline, after {took:?}: {at_deadline:?}"
    );
    assert!(took < Duration::from_secs(2), "the shutdown took {took:?}");
    let rows = first.list().await;
    assert!(
        rows.iter().all(|r| r.state == State::Queued),
        "not every row was put back in line: {:?}",
        rows.iter().map(|r| &r.state).collect::<Vec<_>>()
    );
    // Nothing more is asked of the server once it is down.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let sent_by_then = sent.load(Ordering::SeqCst);
    let staged: u64 = first
        .list()
        .await
        .iter()
        .map(|r| r.progress.downloaded)
        .sum();
    drop(first);

    // The next start, from the same store.
    let second = queue(dir.path());
    second.persist_to(store.clone()).await;
    let rows = second.list().await;
    assert_eq!(rows.len(), DOWNLOADS, "rows were lost");
    second.run().await;
    let rows = second.list().await;
    for row in &rows {
        assert_eq!(
            row.state,
            State::Completed,
            "{} ended {:?}",
            row.url,
            row.state
        );
        let got = std::fs::read(&row.path).unwrap();
        assert!(got == *data, "{} is not the file served", row.path);
    }
    // What was fetched twice: everything served, less the files. Each
    // part's end is no longer asked for again and compared at the resume
    // (`RESUME_CHECK`, 64 KiB), since the stop put it on the disk before
    // writing that down, and what had already come when the stop was seen
    // is written rather than dropped (gaps row 186). What is left is what
    // the server had sent and dm had not yet been given when it let go,
    // counted here as the server hands it to the connection, and the seams
    // the restart checks (195,840 bytes for these files fetched straight
    // through). Measured on macOS: 2,981,120 bytes before (about 73 KiB a
    // connection, the same as a person's pause and resume), 0.78 to 1.47 MB
    // after in five runs (19 to 36 KiB).
    let served = sent.load(Ordering::SeqCst);
    let twice = served.saturating_sub(SIZE * DOWNLOADS);
    let before = (sent_by_then as u64).saturating_sub(staged) as usize;
    let after = twice.saturating_sub(before);
    // Two parts, bounded apart. Before the restart: what the server had
    // written to a connection that dm had not read when it let go, which
    // the kernel's buffers on loopback hold (one in ten runs alone held
    // 90 KiB a connection, 3.6 MB in all, 2026-11-12), so up to 128 KiB a
    // connection. After: what the restart asks again, the seams included,
    // which is dm's own and held to 48 KiB a connection.
    // Windows' loopback holds more: 5,293,312 bytes for the 40 connections
    // on its CI runner (about 132 KiB each, 2026-11-14), so 256 KiB there.
    let per_connection = if cfg!(windows) { 256 } else { 128 } * 1024;
    let held_by_buffers = DOWNLOADS * CONNECTIONS * per_connection;
    let allowed = DOWNLOADS * CONNECTIONS * 48 * 1024;
    println!(
        "{twice} bytes fetched twice: {before} before the restart (at most {held_by_buffers}), \
         {after} by it (at most {allowed})"
    );
    assert!(
        before <= held_by_buffers,
        "{before} bytes sent before the restart were never staged ({sent_by_then} sent, \
         {staged} staged), more than {held_by_buffers}"
    );
    assert!(
        after <= allowed,
        "{after} bytes fetched again by the restart, more than {allowed}"
    );
}
