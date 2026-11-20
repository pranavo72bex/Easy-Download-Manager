// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Move/Rename for a download that failed part-way (B54): its parts go with it to the folder
//! chosen, and tried again it goes on from them there rather than from nothing. Beside
//! `queue.rs`'s `an_unfinished_download_given_another_folder_and_name_lands_there`, whose
//! failed download had fetched nothing; a file of its own, as `queue.rs` is long.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use dm_core::{client, DownloadList, State as DownloadState};

mod common;
use common::tempdir;

/// The file's size, and where the first fetch of it is cut.
const SIZE: usize = 1024 * 1024;
const CUT_AT: usize = SIZE / 2;

/// A server that, until `open` is set, cuts the first fetch of the file at [`CUT_AT`] and
/// answers every later one 404, which the engine's own retries do not cover: the download
/// fails with half of it staged. Every request but a one-byte look (`bytes=0-0`), which is
/// answered whole, is noted with its range and whether the server was open then.
#[derive(Clone)]
struct Cut {
    data: Arc<Vec<u8>>,
    open: Arc<AtomicBool>,
    fetches: Arc<AtomicUsize>,
    asked: Arc<Mutex<Vec<(bool, String)>>>,
}

async fn cut(State(s): State<Cut>, headers: HeaderMap) -> Response {
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let (a, b) = range
        .strip_prefix("bytes=")
        .and_then(|r| r.split_once('-'))
        .map(|(a, b)| {
            (
                a.parse::<usize>().unwrap_or(0),
                b.parse::<usize>().unwrap_or(SIZE - 1).min(SIZE - 1),
            )
        })
        .unwrap_or((0, SIZE - 1));
    if a > b {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .body(Body::empty())
            .unwrap();
    }
    let open = s.open.load(Ordering::SeqCst);
    let probe = a == 0 && b == 0;
    if !probe {
        s.asked.lock().unwrap().push((open, range.clone()));
    }
    let mut body = s.data[a..=b].to_vec();
    if !open && !probe {
        if s.fetches.fetch_add(1, Ordering::SeqCst) > 0 {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("no"))
                .unwrap();
        }
        // Said whole in its length, sent to half: the connection drops there.
        body.truncate(CUT_AT.saturating_sub(a));
    }
    let mut answer = Response::builder()
        .header("accept-ranges", "bytes")
        .header("content-length", b - a + 1)
        .header("etag", "\"cut\"");
    answer = if range.is_empty() {
        answer.status(StatusCode::OK)
    } else {
        answer
            .status(StatusCode::PARTIAL_CONTENT)
            .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
    };
    answer.body(Body::from(body)).unwrap()
}

/// Where a range asks to start, for one that names a start.
fn start_of(range: &str) -> Option<usize> {
    let (a, _) = range.strip_prefix("bytes=")?.split_once('-')?;
    a.parse().ok()
}

#[tokio::test]
async fn a_download_failed_part_way_moved_elsewhere_goes_on_from_its_parts() {
    let data: Vec<u8> = (0..SIZE).map(|i| (i % 251) as u8).collect();
    let server = Cut {
        data: Arc::new(data.clone()),
        open: Arc::new(AtomicBool::new(false)),
        fetches: Arc::new(AtomicUsize::new(0)),
        asked: Arc::new(Mutex::new(Vec::new())),
    };
    let state = server.clone();
    let app = axum::Router::new().fallback(cut).with_state(state);
    let addr = common::serve(app).await;
    let dir = tempdir();
    let elsewhere = dir.path().join("elsewhere");
    let q = DownloadList::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir.path())
        },
        1,
    );
    let id = q.add(&format!("http://{addr}/half.bin")).await.unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert!(
        matches!(row.state, DownloadState::Failed(_)),
        "{:?}",
        row.state
    );
    let old_parts = std::path::PathBuf::from(&row.parts_dir);
    assert!(old_parts.is_dir(), "no parts staged at {}", row.parts_dir);

    server.open.store(true, Ordering::SeqCst);
    q.place_unfinished(id, Some(elsewhere.clone()), None)
        .await
        .unwrap();
    let placed = q.get(id).await.unwrap();
    assert!(
        std::path::Path::new(&placed.parts_dir).starts_with(&elsewhere),
        "{}",
        placed.parts_dir
    );
    assert!(std::path::Path::new(&placed.parts_dir).is_dir());
    assert!(!old_parts.exists(), "the parts were left behind");
    q.run().await;

    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed, "{:?}", row.state);
    assert_eq!(std::path::Path::new(&row.path), elsewhere.join("half.bin"));
    assert_eq!(std::fs::read(&row.path).unwrap(), data, "bytes differ");
    let after: Vec<String> = server
        .asked
        .lock()
        .unwrap()
        .iter()
        .filter(|(open, _)| *open)
        .map(|(_, range)| range.clone())
        .collect();
    /* Each run's first request asks from the start (`bytes=0-`, the probe), its body let go of
     * where parts are staged; the rest is asked for from what was staged, less the end of it
     * fetched again and compared (`RESUME_CHECK`, 64 KiB): past the start, and not past where
     * the first fetch was cut. */
    assert!(
        after
            .iter()
            .any(|r| start_of(r).is_some_and(|a| a > 0 && a <= CUT_AT)),
        "not asked for the rest from its parts: {after:?}"
    );
}
