// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A server that goes quiet part-way: what it sent is on disk, and written
//! down in its range's ledger, within the ledger's quarter second, not held
//! in the write buffer until the server speaks again. A connection gathers
//! 64 KiB before it writes (gaps row 176), so less than that, sent and then
//! nothing, was otherwise held for as long as the server kept quiet: off
//! the disk a preview reads, and past the ledger a crash goes back to.

mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::response::Response;
use axum::routing::get;
use common::{options, serve, tempdir};

/// What the server sends before it goes quiet: under the write buffer.
const SENT: usize = 40 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_a_quiet_server_sent_is_written_down_within_a_quarter_second() {
    let app = axum::Router::new().route(
        "/quiet.bin",
        get(|| async {
            let first =
                futures_util::stream::iter([Ok::<Vec<u8>, std::io::Error>(vec![7u8; SENT])]);
            let never = futures_util::stream::pending();
            Response::builder()
                .header("content-length", (1024 * 1024).to_string())
                .body(Body::from_stream(futures_util::StreamExt::chain(
                    first, never,
                )))
                .unwrap()
        }),
    );
    let url = format!("http://{}/quiet.bin", serve(app).await);
    let dir = tempdir();
    let asked = options(dir.path());
    let control = asked.control.clone();
    let task =
        tokio::spawn(async move { dm_core::download(&dm_core::client(), &url, &asked).await });

    // The one range's ledger, read as it is written.
    let ledger = dir.path().join("parts").join("done-0000");
    let started = Instant::now();
    let said = loop {
        let said = std::fs::read_to_string(&ledger)
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if said >= SENT as u64 || started.elapsed() > Duration::from_secs(5) {
            break said;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let took = started.elapsed();
    control.cancel();
    let _ = task.await;
    assert_eq!(said, SENT as u64, "the ledger said {said} after {took:?}");
    // The request, its answer and the quarter second, with room for a busy
    // computer: not the five seconds a stalled connection is judged after.
    assert!(took < Duration::from_secs(2), "written down after {took:?}");
}
