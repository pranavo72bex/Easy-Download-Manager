// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A checksum file published beside a download (`dm.zip.sha256`, `SHA256SUMS`) is fetched and
//! the file checked against it when no digest was typed (gaps row F5): a match says so on the
//! row, a mismatch fails the download as a typed digest's does, and none is said nothing of.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::Response;
use dm_core::{client, Queue, State as DownloadState};

use common::tempdir;

/// The file every test downloads.
fn served() -> Vec<u8> {
    (0..300_000u32).map(|i| (i % 251) as u8).collect()
}

fn sha256_of(bytes: &[u8]) -> String {
    use sha2::Digest;
    dm_core::hex::encode(sha2::Sha256::digest(bytes))
}

/// An answer the server gives besides the file: its status, headers and body.
type Answer = (u16, Vec<(String, String)>, String);

/// What the server answers besides the file, by path, and every path asked for.
#[derive(Clone, Default)]
struct Site {
    extra: Arc<Mutex<HashMap<String, Answer>>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Site {
    fn answer(&self, path: &str, body: &str) {
        self.extra
            .lock()
            .unwrap()
            .insert(path.into(), (200, Vec::new(), body.into()));
    }

    fn redirect(&self, path: &str, to: &str) {
        self.extra.lock().unwrap().insert(
            path.into(),
            (302, vec![("location".into(), to.into())], String::new()),
        );
    }
}

async fn handler(State(site): State<Site>, uri: Uri) -> Response {
    let path = uri.path().to_string();
    site.asked.lock().unwrap().push(path.clone());
    if path == "/rel/dm.zip" {
        return Response::builder()
            .header("content-type", "application/zip")
            .body(Body::from(served()))
            .unwrap();
    }
    match site.extra.lock().unwrap().get(&path).cloned() {
        Some((status, headers, body)) => {
            let mut answer = Response::builder().status(status);
            for (name, value) in headers {
                answer = answer.header(name, value);
            }
            answer.body(Body::from(body)).unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap(),
    }
}

async fn site() -> (std::net::SocketAddr, Site) {
    let site = Site::default();
    let addr = common::serve(
        axum::Router::new()
            .fallback(handler)
            .with_state(site.clone()),
    )
    .await;
    (addr, site)
}

/// The file downloaded through a queue, as a person's row is.
async fn fetched(addr: std::net::SocketAddr, dir: &std::path::Path) -> dm_core::Entry {
    let q = Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir)
        },
        1,
    );
    let id = q.add(&format!("http://{addr}/rel/dm.zip")).await.unwrap();
    q.run().await;
    q.get(id).await.unwrap()
}

#[tokio::test]
async fn a_matching_checksum_file_beside_the_download_checks_it_and_says_so() {
    let (addr, site) = site().await;
    site.answer(
        "/rel/dm.zip.sha256",
        &format!("{}  dm.zip\n", sha256_of(&served())),
    );
    let dir = tempdir();

    let row = fetched(addr, dir.path()).await;
    assert_eq!(row.state, DownloadState::Completed);
    assert!(row.verified, "the checksum file was not checked");
    assert_eq!(row.said, dm_core::CHECKED_BESIDE);
    assert!(dm_core::NOTES.contains(&row.said.as_str()));
    assert_eq!(
        std::fs::read(dir.path().join("out/dm.zip")).unwrap(),
        served()
    );
}

#[tokio::test]
async fn a_checksum_file_that_does_not_match_fails_the_download_as_a_typed_one_does() {
    let (addr, site) = site().await;
    site.answer("/rel/dm.zip.sha256", &"0".repeat(64));
    let dir = tempdir();

    let row = fetched(addr, dir.path()).await;
    match &row.state {
        DownloadState::Failed(why) => assert!(
            why.contains("sha256") && why.contains(&"0".repeat(64)),
            "unhelpful reason: {why}"
        ),
        other => panic!("a file that failed its published digest was {other:?}"),
    }
    assert!(!row.verified);
    assert!(
        !dir.path().join("out/dm.zip").exists(),
        "a file known to be wrong was left in the downloads folder"
    );
}

#[tokio::test]
async fn a_release_list_of_digests_in_the_same_folder_is_read_for_this_file() {
    let (addr, site) = site().await;
    site.answer(
        "/rel/SHA256SUMS",
        &format!(
            "{}  other.zip\n{} *dm.zip\n{}  dm.zip.asc\n",
            "ab".repeat(32),
            sha256_of(&served()),
            "cd".repeat(32)
        ),
    );
    let dir = tempdir();

    let row = fetched(addr, dir.path()).await;
    assert_eq!(row.state, DownloadState::Completed);
    assert!(
        row.verified,
        "the list's line for this file was not checked"
    );
    assert_eq!(row.said, dm_core::CHECKED_BESIDE);
}

#[tokio::test]
async fn with_no_checksum_file_the_download_finishes_quickly_and_says_nothing() {
    let (addr, site) = site().await;
    let dir = tempdir();

    let started = Instant::now();
    let done = dm_core::download(
        &client(),
        &format!("http://{addr}/rel/dm.zip"),
        &common::options(dir.path()),
    )
    .await
    .unwrap();
    assert!(!done.verified);
    assert!(started.elapsed() < Duration::from_secs(5));

    // One GET for each checksum file, and none outside the file's folder.
    let asked = site.asked.lock().unwrap().clone();
    for name in [
        "dm.zip.sha256",
        "dm.zip.sha512",
        "dm.zip.sha1",
        "dm.zip.md5",
        "SHA256SUMS",
        "SHA512SUMS",
    ] {
        let path = format!("/rel/{name}");
        assert_eq!(asked.iter().filter(|p| **p == path).count(), 1, "{path}");
    }
    assert!(asked.iter().all(|p| p.starts_with("/rel/")), "{asked:?}");

    // A row says nothing of a look that found nothing.
    let row = fetched(addr, tempdir().path()).await;
    assert_eq!(row.state, DownloadState::Completed);
    assert!(!row.verified);
    assert!(row.said.is_empty(), "{}", row.said);
}

#[tokio::test]
async fn a_checksum_file_for_another_file_or_on_another_server_is_not_taken() {
    let (addr, site) = site().await;
    // Names a different file: not this one's digest, though it is the wrong one.
    site.answer(
        "/rel/dm.zip.sha256",
        &format!("{}  other.zip\n", "0".repeat(64)),
    );
    // Sent on to another server (`localhost` is not `127.0.0.1` to a redirect).
    site.redirect(
        "/rel/dm.zip.sha512",
        &format!("http://localhost:{}/rel/x.sha512", addr.port()),
    );
    site.answer("/rel/x.sha512", &"0".repeat(128));
    let dir = tempdir();

    let row = fetched(addr, dir.path()).await;
    assert_eq!(row.state, DownloadState::Completed);
    assert!(!row.verified);
    assert!(row.said.is_empty());
}

#[tokio::test]
async fn a_typed_digest_is_checked_and_no_checksum_file_is_asked_for() {
    let (addr, site) = site().await;
    let dir = tempdir();
    let options = dm_core::Options {
        checksum: dm_core::Checksum::parse(&sha256_of(&served())),
        ..common::options(dir.path())
    };
    let done = dm_core::download(&client(), &format!("http://{addr}/rel/dm.zip"), &options)
        .await
        .unwrap();
    assert!(done.verified);
    let asked = site.asked.lock().unwrap().clone();
    assert!(asked.iter().all(|p| p == "/rel/dm.zip"), "{asked:?}");
}

/// A row fetched again says what this run checked, not what an earlier run did: checked beside
/// once, then again with a typed digest and no checksum file, it no longer says the first.
#[tokio::test]
async fn a_row_fetched_again_does_not_keep_an_earlier_runs_word() {
    let (addr, site) = site().await;
    site.answer(
        "/rel/dm.zip.sha256",
        &format!("{}  dm.zip\n", sha256_of(&served())),
    );
    let dir = tempdir();
    let q = Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir.path())
        },
        1,
    );
    let id = q.add(&format!("http://{addr}/rel/dm.zip")).await.unwrap();
    q.run().await;
    assert_eq!(q.get(id).await.unwrap().said, dm_core::CHECKED_BESIDE);

    site.extra.lock().unwrap().clear();
    q.set_checksum(id, dm_core::Checksum::parse(&sha256_of(&served())))
        .await;
    assert!(q.redownload(id).await);
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert!(row.verified, "the typed digest was not checked");
    assert!(row.said.is_empty(), "{}", row.said);
}
