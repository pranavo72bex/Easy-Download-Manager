// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a stream's segment costs in memory: its own file, so its own process, as
//! `memory.rs` is (resident memory is the process's, and other tests' bodies would be in it).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use dm_core::{client, download};

mod common;
use common::{options, tempdir};

const MB: usize = 1024 * 1024;
const SEGMENT: usize = 160 * MB;

/// A stream of one segment of 160 MiB in the clear is written as it comes, not held whole
/// first: the process grows by far less than the segment while it is fetched, and the file is
/// the segment byte for byte (gaps row Y17).
#[tokio::test(flavor = "multi_thread")]
async fn a_streams_segment_is_written_as_it_comes() {
    if memory_stats::memory_stats().is_none() {
        eprintln!("this platform does not report memory; skipping");
        return;
    }
    // Made once and handed out by reference: the server holds it, and that is before the
    // measuring starts.
    let segment = bytes::Bytes::from((0..SEGMENT).map(|i| (i % 251) as u8).collect::<Vec<u8>>());
    async fn handler(State(segment): State<bytes::Bytes>, uri: axum::http::Uri) -> Response {
        let ok = |body: Body, length: usize| {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-length", length)
                .body(body)
                .unwrap()
        };
        match uri.path() {
            "/v/index.m3u8" => {
                let list = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n\
                            #EXTINF:10.0,\nseg0.ts\n#EXT-X-ENDLIST\n";
                ok(Body::from(list), list.len())
            }
            "/v/seg0.ts" => ok(Body::from(segment.clone()), segment.len()),
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap(),
        }
    }
    let addr = common::serve(
        axum::Router::new()
            .fallback(handler)
            .with_state(segment.clone()),
    )
    .await;
    let resident = || memory_stats::memory_stats().unwrap().physical_mem as u64;

    let before = resident();
    let peak = Arc::new(AtomicU64::new(before));
    let done = Arc::new(AtomicBool::new(false));
    let sampler = {
        let (peak, done) = (Arc::clone(&peak), Arc::clone(&done));
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                peak.fetch_max(resident(), Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        })
    };
    let dir = tempdir();
    let fetched = download(
        &client(),
        &format!("http://{addr}/v/index.m3u8"),
        &options(dir.path()),
    )
    .await;
    done.store(true, Ordering::Relaxed);
    sampler.join().unwrap();
    let fetched = fetched.expect("the stream failed");
    assert_eq!(fetched.bytes, SEGMENT as u64);
    assert!(
        std::fs::read(&fetched.path).unwrap() == segment,
        "the file is not the segment"
    );
    let grew = peak.load(Ordering::Relaxed).saturating_sub(before);
    assert!(
        grew < (64 * MB) as u64,
        "fetching a {SEGMENT}-byte segment grew the process by {grew} bytes at its most: the \
         segment is being held"
    );
}
