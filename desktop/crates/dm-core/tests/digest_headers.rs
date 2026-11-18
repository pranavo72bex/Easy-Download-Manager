// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A server's own word for a file's digest (`Repr-Digest`, `Digest`, `Content-MD5`) checks the
//! finished file as a checksum file beside it does (gaps row W19): a match passes and no
//! checksum file is asked for, a mismatch fails the download, says so and leaves no file.

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use base64::Engine as _;
use dm_core::{client, Queue, State as DownloadState};
use sha2::Digest as _;

use common::tempdir;

/// The file every test downloads: `size` bytes.
fn served(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The digest headers a server sends for `bytes`, of each kind dm reads.
fn repr_digest(bytes: &[u8]) -> (String, String) {
    (
        "repr-digest".into(),
        format!("sha-256=:{}:", b64(&sha2::Sha256::digest(bytes))),
    )
}
fn legacy_digest(bytes: &[u8]) -> (String, String) {
    (
        "digest".into(),
        format!("SHA-512={}", b64(&sha2::Sha512::digest(bytes))),
    )
}
fn content_md5(bytes: &[u8]) -> (String, String) {
    use md5::Digest as _;
    ("content-md5".into(), b64(&md5::Md5::digest(bytes)))
}

/// What the server sends: the file, the headers it adds to every answer of it, whether it
/// answers a range, and every path asked for.
#[derive(Clone)]
struct Site {
    body: Arc<Vec<u8>>,
    headers: Vec<(String, String)>,
    ranges: bool,
    asked: Arc<Mutex<Vec<String>>>,
}

async fn handler(State(site): State<Site>, uri: Uri, asked: HeaderMap) -> Response {
    site.asked.lock().unwrap().push(uri.path().to_owned());
    if uri.path() != "/rel/dm.zip" {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap();
    }
    let total = site.body.len();
    let range = asked
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .filter(|_| site.ranges)
        .and_then(|(from, to)| {
            let from: usize = from.parse().ok()?;
            let to = if to.is_empty() {
                total - 1
            } else {
                to.parse::<usize>().ok()?.min(total - 1)
            };
            (from <= to).then_some((from, to))
        });
    let mut answer = Response::builder()
        .header("content-type", "application/zip")
        .header("accept-ranges", if site.ranges { "bytes" } else { "none" });
    for (name, value) in &site.headers {
        answer = answer.header(name, value);
    }
    match range {
        Some((from, to)) => answer
            .status(StatusCode::PARTIAL_CONTENT)
            .header("content-range", format!("bytes {from}-{to}/{total}"))
            .body(Body::from(site.body[from..=to].to_vec()))
            .unwrap(),
        None => answer.body(Body::from(site.body.to_vec())).unwrap(),
    }
}

/// Serves `body` with `headers` on every answer of it.
async fn site(
    body: Vec<u8>,
    headers: Vec<(String, String)>,
    ranges: bool,
) -> (std::net::SocketAddr, Site) {
    let site = Site {
        body: Arc::new(body),
        headers,
        ranges,
        asked: Arc::default(),
    };
    let addr = common::serve(
        axum::Router::new()
            .fallback(handler)
            .with_state(site.clone()),
    )
    .await;
    (addr, site)
}

/// The file downloaded on `connections` through a queue, as a person's row is.
async fn fetched(
    addr: std::net::SocketAddr,
    dir: &std::path::Path,
    connections: usize,
) -> dm_core::Entry {
    let q = Queue::new(
        client(),
        dm_core::Options {
            connections,
            ..common::options(dir)
        },
        1,
    );
    let id = q.add(&format!("http://{addr}/rel/dm.zip")).await.unwrap();
    q.run().await;
    q.get(id).await.unwrap()
}

/// A digest header that matches: the file is checked, kept, and no checksum file asked for.
async fn passes(header: (String, String), ranges: bool) {
    let body = served(300_000);
    let (addr, site) = site(body.clone(), vec![header.clone()], ranges).await;
    let dir = tempdir();
    let row = fetched(addr, dir.path(), 1).await;
    assert_eq!(row.state, DownloadState::Completed, "{header:?}");
    assert!(row.verified, "{header:?} was not checked");
    // Said of a checksum file only; a header's digest is checked as a typed one, silently.
    assert!(row.said.is_empty(), "{}", row.said);
    assert_eq!(std::fs::read(dir.path().join("out/dm.zip")).unwrap(), body);
    let asked = site.asked.lock().unwrap().clone();
    assert!(asked.iter().all(|p| p == "/rel/dm.zip"), "{asked:?}");
}

/// A digest header that does not match: the download fails, names the hash and the published
/// digest as a checksum file's mismatch does, and the file is not left in the folder.
async fn fails(header: (String, String), algorithm: &str, expected_hex: &str) {
    let (addr, _site) = site(served(300_000), vec![header.clone()], false).await;
    let dir = tempdir();
    let row = fetched(addr, dir.path(), 1).await;
    match &row.state {
        DownloadState::Failed(why) => assert!(
            why.contains(algorithm) && why.contains(expected_hex),
            "unhelpful reason for {header:?}: {why}"
        ),
        other => panic!("a file that failed {header:?} was {other:?}"),
    }
    assert!(!row.verified);
    assert!(
        !dir.path().join("out/dm.zip").exists(),
        "a file known to be wrong was left in the downloads folder"
    );
}

#[tokio::test]
async fn a_matching_repr_digest_checks_the_file() {
    passes(repr_digest(&served(300_000)), false).await;
}

#[tokio::test]
async fn a_repr_digest_that_does_not_match_fails_the_download_and_says_so() {
    let wrong = b"another file";
    fails(
        repr_digest(wrong),
        "sha256",
        &dm_core::hex::encode(sha2::Sha256::digest(wrong)),
    )
    .await;
}

#[tokio::test]
async fn a_matching_digest_header_checks_the_file() {
    passes(legacy_digest(&served(300_000)), false).await;
}

#[tokio::test]
async fn a_digest_header_that_does_not_match_fails_the_download_and_says_so() {
    let wrong = b"another file";
    fails(
        legacy_digest(wrong),
        "sha512",
        &dm_core::hex::encode(sha2::Sha512::digest(wrong)),
    )
    .await;
}

#[tokio::test]
async fn a_matching_content_md5_checks_the_file() {
    passes(content_md5(&served(300_000)), false).await;
}

#[tokio::test]
async fn a_content_md5_that_does_not_match_fails_the_download_and_says_so() {
    use md5::Digest as _;
    let wrong = b"another file";
    fails(
        content_md5(wrong),
        "md5",
        &dm_core::hex::encode(md5::Md5::digest(wrong)),
    )
    .await;
}

/// A 206 from the file's first byte to its last is the whole file: its `Content-MD5` is the
/// file's, and checked.
#[tokio::test]
async fn a_content_md5_on_a_part_that_is_the_whole_file_checks_it() {
    passes(content_md5(&served(300_000)), true).await;
}

/// On several connections each answer is a part of the file: a `Content-MD5` or `Digest` there
/// names that part (here, wrongly, every part the same digest), so neither is taken, while a
/// `Repr-Digest` still names the whole file and is checked.
#[tokio::test]
async fn on_parts_of_the_file_only_a_repr_digest_is_taken() {
    // Past the first ask (8 MiB), so the first answer is a part too.
    let body = served(9 * 1024 * 1024 + 7);
    let wrong = b"another file";
    let (addr, _site) = site(
        body.clone(),
        vec![legacy_digest(wrong), content_md5(wrong)],
        true,
    )
    .await;
    let dir = tempdir();
    let row = fetched(addr, dir.path(), 8).await;
    assert_eq!(row.state, DownloadState::Completed);
    assert!(!row.verified, "a part's digest was taken for the file's");
    assert_eq!(std::fs::read(dir.path().join("out/dm.zip")).unwrap(), body);

    let (addr, _site) = site(body.clone(), vec![repr_digest(&body)], true).await;
    let dir = tempdir();
    let row = fetched(addr, dir.path(), 8).await;
    assert_eq!(row.state, DownloadState::Completed);
    assert!(row.verified, "the Repr-Digest on a part was not checked");

    let (addr, _site) = site(body, vec![repr_digest(wrong)], true).await;
    let dir = tempdir();
    let row = fetched(addr, dir.path(), 8).await;
    assert!(
        matches!(&row.state, DownloadState::Failed(why) if why.contains("sha256")),
        "{:?}",
        row.state
    );
}
