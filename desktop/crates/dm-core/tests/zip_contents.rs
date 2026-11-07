// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What is inside a ZIP, learned from its end with range requests: the names
//! and sizes, without the archive being fetched.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use dm_core::{client, zip_contents};

mod common;

fn archive(files: usize, each: usize) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.add_directory("pack/", stored).unwrap();
        for n in 0..files {
            zip.start_file(format!("pack/file-{n:03}.bin"), stored)
                .unwrap();
            zip.write_all(&vec![n as u8; each]).unwrap();
        }
        zip.set_comment("made for a test").unwrap();
        zip.finish().unwrap();
    }
    buffer.into_inner()
}

#[derive(Clone)]
struct Site {
    body: Arc<Vec<u8>>,
    ranges: bool,
    /// How many bytes of body the server has sent: a listing must not cost the
    /// archive.
    sent: Arc<AtomicU64>,
}

async fn answer(State(site): State<Site>, headers: HeaderMap) -> Response {
    let total = site.body.len();
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| {
            (
                a.parse::<usize>().unwrap_or(0),
                b.parse::<usize>().unwrap_or(total - 1).min(total - 1),
            )
        });
    match range {
        Some((a, b)) if site.ranges => {
            site.sent.fetch_add((b - a + 1) as u64, Ordering::SeqCst);
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("accept-ranges", "bytes")
                .header("content-range", format!("bytes {a}-{b}/{total}"))
                .header("content-length", b - a + 1)
                .header("etag", "\"z\"")
                .body(Body::from(site.body[a..=b].to_vec()))
                .unwrap()
        }
        _ => {
            site.sent.fetch_add(total as u64, Ordering::SeqCst);
            Response::builder()
                .status(StatusCode::OK)
                .header("content-length", total)
                .body(Body::from(site.body.to_vec()))
                .unwrap()
        }
    }
}

async fn serve(body: Vec<u8>, ranges: bool) -> (String, Arc<AtomicU64>) {
    let sent = Arc::new(AtomicU64::new(0));
    let site = Site {
        body: Arc::new(body),
        ranges,
        sent: sent.clone(),
    };
    let app = axum::Router::new().fallback(answer).with_state(site);
    let addr = common::serve(app).await;
    (format!("http://{addr}/pack.zip"), sent)
}

#[tokio::test]
async fn the_files_of_a_zip_are_listed_from_its_end_without_fetching_it() {
    // About 3 MB of archive: the listing must cost a small part of it.
    let body = archive(30, 100_000);
    let total = body.len() as u64;
    let (url, sent) = serve(body, true).await;

    let entries = zip_contents(&client(), &url, None, None)
        .await
        .expect("a listing");
    assert_eq!(entries.len(), 31);
    assert!(entries[0].is_folder() && entries[0].name == "pack/");
    assert_eq!(entries[1].name, "pack/file-000.bin");
    assert_eq!(entries[1].size, 100_000);
    assert_eq!(entries[30].name, "pack/file-029.bin");
    assert!(
        sent.load(Ordering::SeqCst) < total / 10,
        "{} of {total} bytes were fetched for a list",
        sent.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn a_server_without_ranges_gives_no_listing() {
    let (url, _) = serve(archive(3, 1000), false).await;
    assert!(zip_contents(&client(), &url, None, None).await.is_none());
}

#[tokio::test]
async fn something_that_is_not_a_zip_gives_no_listing() {
    let (url, _) = serve(vec![b'x'; 200_000], true).await;
    assert!(zip_contents(&client(), &url, None, None).await.is_none());
}
