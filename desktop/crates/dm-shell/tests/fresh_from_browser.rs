// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A browser's hand-off reaching a row whose signed address ran out: the row
//! takes the browser's fresh address and goes on (gaps rows 57 and 121), and
//! a renewal no row waits for is dropped. A hand-off the window asks about is
//! asked about first, as contract row B63 has it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use dm_core::{AddOptions, Queue, State as DownloadState};
use dm_shell::host::Request;
use dm_shell::settings::Settings;
use dm_shell::shell::{take_from_browser, Taken};

const MB: usize = 1024 * 1024;

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

/// `/f.bin?t=old` answers once (the first look) and then 403, as a signed
/// address that ran out does; any other token is good.
async fn answer(State(old): State<Arc<AtomicUsize>>, uri: Uri, headers: HeaderMap) -> Response {
    if uri.query().unwrap_or("").contains("t=old") && old.fetch_add(1, Ordering::SeqCst) >= 1 {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("Request has expired"))
            .unwrap();
    }
    let total = 3 * MB;
    let body = pattern(total);
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
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("accept-ranges", "bytes")
        .header("content-range", format!("bytes {a}-{b}/{total}"))
        .header("content-length", b - a + 1)
        .header("etag", "\"v1\"")
        .body(Body::from(body[a..=b].to_vec()))
        .unwrap()
}

async fn site() -> std::net::SocketAddr {
    let app = axum::Router::new()
        .fallback(answer)
        .with_state(Arc::new(AtomicUsize::new(0)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn queue(dir: &std::path::Path) -> (Queue, Settings) {
    let saved = Settings {
        download_dir: dir.join("out"),
        ..Settings::default()
    };
    let mut options = saved.options(dir.join("parts"), dm_core::Limiter::unlimited());
    options.attempts = 1;
    (Queue::new(dm_core::client(), options, 1), saved)
}

/// What the extension's hand-off is filed as: from the browser, with the page.
fn handed(url: String, renews: bool) -> Request {
    Request {
        url,
        referrer: "http://page.example/watch".into(),
        from_browser: true,
        renews,
        ..Request::default()
    }
}

/// A row from the browser whose address has run out, waiting for a fresh one.
async fn waiting(q: &Queue, addr: std::net::SocketAddr) -> dm_core::Id {
    let id = q
        .add_with(
            &format!("http://{addr}/f.bin?t=old"),
            AddOptions {
                referrer: "http://page.example/watch".into(),
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert!(row.fresh_link, "the row does not wait: {:?}", row.state);
    id
}

#[tokio::test]
async fn a_renewed_address_takes_the_waiting_rows_place_and_the_file_finishes() {
    let addr = site().await;
    let dir = tempfile::tempdir().unwrap();
    let (q, saved) = queue(dir.path());
    let id = waiting(&q, addr).await;

    let taken = take_from_browser(
        &q,
        &saved,
        handed(format!("http://{addr}/f.bin?t=new"), true),
        false,
    )
    .await;
    assert_eq!(taken, Taken::Adopted(id));
    assert_eq!(q.list().await.len(), 1, "a second row was added");
    q.run().await;
    let done = q.get(id).await.unwrap();
    assert_eq!(done.state, DownloadState::Completed, "{:?}", done.state);
    assert!(done.url.ends_with("t=new"), "{}", done.url);
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(3 * MB));
}

#[tokio::test]
async fn an_ordinary_hand_off_of_the_same_file_takes_the_waiting_row_unless_the_window_asks() {
    let addr = site().await;
    let dir = tempfile::tempdir().unwrap();
    let (q, saved) = queue(dir.path());
    let id = waiting(&q, addr).await;

    // Download details opens first where it is on (B63), as it always did.
    let asked = take_from_browser(
        &q,
        &saved,
        handed(format!("http://{addr}/f.bin?t=again"), false),
        true,
    )
    .await;
    assert!(matches!(asked, Taken::Ask(_)), "{asked:?}");
    // With it off, the waiting row takes the browser's address.
    let taken = take_from_browser(
        &q,
        &saved,
        handed(format!("http://{addr}/f.bin?t=again"), false),
        false,
    )
    .await;
    assert_eq!(taken, Taken::Adopted(id));
    assert_eq!(q.list().await.len(), 1);
}

#[tokio::test]
async fn a_renewal_no_row_waits_for_is_dropped_and_adds_nothing() {
    let addr = site().await;
    let dir = tempfile::tempdir().unwrap();
    let (q, saved) = queue(dir.path());

    let taken = take_from_browser(
        &q,
        &saved,
        handed(format!("http://{addr}/f.bin?t=new"), true),
        false,
    )
    .await;
    assert_eq!(taken, Taken::Dropped);
    assert!(q.list().await.is_empty());

    // An ordinary hand-off is asked about or added, as before.
    let asked = take_from_browser(
        &q,
        &saved,
        handed(format!("http://{addr}/f.bin?t=new"), false),
        true,
    )
    .await;
    assert!(matches!(asked, Taken::Ask(_)), "{asked:?}");
    let added = take_from_browser(
        &q,
        &saved,
        handed(format!("http://{addr}/f.bin?t=new"), false),
        false,
    )
    .await;
    assert!(matches!(added, Taken::Added(Some(_))), "{added:?}");
    assert_eq!(q.list().await.len(), 1);
}
