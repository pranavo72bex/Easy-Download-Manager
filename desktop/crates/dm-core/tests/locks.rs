// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A write of the queue file that takes long, as on a slow or busy disk,
//! holds back nothing but the queue's next write (gaps row 184): the list is
//! read at once, other downloads' progress keeps moving, and a change a
//! person makes meanwhile is answered at once and written by the next write.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use dm_core::{client, AddOptions, Id, Queue, State};

use common::{serve, tempdir};

/// Each file's size: long enough at the server's pace to last the test.
const SIZE: usize = 8 * 1024 * 1024;
/// What the server sends at a time, and how long it waits between: about
/// 400 KB a second a connection.
const PIECE: usize = 8 * 1024;
const PACE: Duration = Duration::from_millis(20);
/// How long each write of the queue file is made to take.
const HELD: Duration = Duration::from_secs(2);

/// A paced server of one file under any name.
async fn server() -> String {
    let data: Arc<Vec<u8>> = Arc::new((0..SIZE).map(|i| (i % 251) as u8).collect());
    let app = axum::Router::new().fallback(move |headers: HeaderMap| {
        let data = data.clone();
        async move {
            let from = headers
                .get("range")
                .and_then(|r| r.to_str().ok())
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.split('-').next())
                .and_then(|a| a.parse::<usize>().ok())
                .unwrap_or(0)
                .min(SIZE - 1);
            let pieces: Vec<bytes::Bytes> = data[from..]
                .chunks(PIECE)
                .map(bytes::Bytes::copy_from_slice)
                .collect();
            let stream =
                futures_util::stream::unfold(pieces.into_iter(), |mut pieces| async move {
                    let piece = pieces.next()?;
                    tokio::time::sleep(PACE).await;
                    Some((Ok::<_, std::io::Error>(piece), pieces))
                });
            Response::builder()
                .status(206)
                .header("accept-ranges", "bytes")
                .header("etag", "\"same\"")
                .header("content-range", format!("bytes {from}-{}/{SIZE}", SIZE - 1))
                .header("content-length", SIZE - from)
                .body(Body::from_stream(stream))
                .unwrap()
        }
    });
    format!("http://{}", serve(app).await)
}

/// How far each of `ids` has got, as the list says.
async fn fetched(queue: &Queue, ids: &[Id]) -> Vec<u64> {
    let rows = queue.list_shared().await;
    ids.iter()
        .map(|id| {
            rows.iter()
                .find(|r| r.id == *id)
                .map_or(0, |r| r.progress.downloaded)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_write_of_the_queue_holds_back_no_read_no_download_and_no_change() {
    let base = server().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    let queue = Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir.path())
        },
        4,
    );
    queue.persist_to(store.clone()).await;
    let a = queue.add(&format!("{base}/a.bin")).await.unwrap();
    let b = queue.add(&format!("{base}/b.bin")).await.unwrap();
    // One that waits, for the changes a person makes.
    let c = queue
        .add_with(
            &format!("{base}/c.bin"),
            AddOptions {
                later: true,
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();
    let running = queue.clone();
    tokio::spawn(async move { running.run().await });
    let started = Instant::now();
    while fetched(&queue, &[a, b]).await.contains(&0) {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the downloads never got going"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Every save the adds and the starts owed is written by now.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // A change whose write takes two seconds.
    queue.slow_writes_by(HELD);
    let writes = queue.saves_written();
    let first = queue.clone();
    tokio::spawn(async move { first.set_description(c, "one").await });
    let asked = Instant::now();
    while queue.saves_written() == writes {
        assert!(
            asked.elapsed() < Duration::from_secs(5),
            "nothing was written"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let writing = Instant::now();

    // Meanwhile: a change made by a person is answered at once.
    let asked = Instant::now();
    assert!(queue.set_description(c, "two").await);
    let answered = asked.elapsed();
    assert!(
        answered < Duration::from_millis(100),
        "a change waited {answered:?} for the write under way"
    );
    // The list is read at once, and the downloads keep moving.
    let mut slowest = Duration::ZERO;
    let mut seen = vec![fetched(&queue, &[a, b]).await];
    while writing.elapsed() < HELD - Duration::from_millis(300) {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let asked = Instant::now();
        let rows = queue.list_shared().await;
        slowest = slowest.max(asked.elapsed());
        assert_eq!(rows.len(), 3);
        seen.push(fetched(&queue, &[a, b]).await);
    }
    assert!(
        slowest < Duration::from_millis(10),
        "a read of the list took {slowest:?} during the write"
    );
    // Both further on in each half second of it: progress is told every
    // quarter of a second.
    for window in seen.windows(10) {
        let (then, now) = (&window[0], &window[9]);
        assert!(
            now.iter().zip(then).all(|(now, then)| now > then),
            "a download stood still during the write: {seen:?}"
        );
    }

    // The write under way ends, and the next has the change made meanwhile.
    queue.slow_writes_by(Duration::ZERO);
    queue.flush().await;
    let rows = queue.list().await;
    let row = rows.iter().find(|r| r.id == c).unwrap();
    assert_eq!(row.description, "two");
    assert_eq!(row.state, State::Paused);
    let kept = Queue::new(client(), common::options(dir.path()), 1);
    kept.persist_to(store).await;
    let rows = kept.list().await;
    let row = rows
        .iter()
        .find(|r| r.id == c)
        .expect("the row was not kept");
    assert_eq!(
        row.description, "two",
        "the change made meanwhile was not written"
    );
}
