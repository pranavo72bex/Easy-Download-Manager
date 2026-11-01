//! A signed address from the browser that has expired: the row waits for a
//! fresh address for the same file instead of being left failed, and the
//! browser's next hand-over of that file takes its place and goes on from the
//! parts already staged, as reference A does.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use dm_core::{client, AddOptions, Options, Queue, State as DownloadState};

mod common;
use common::tempdir;

const MB: usize = 1024 * 1024;

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

#[derive(Clone)]
struct Site {
    /// How often the old address was asked for: only the first is answered.
    old: Arc<AtomicUsize>,
}

/// `/f.bin?t=old|new|bigger`, `/g.bin?t=new`: the old address answers once
/// (the probe) and then says 403, as a signed address that ran out does.
async fn answer(State(site): State<Site>, uri: Uri, headers: HeaderMap) -> Response {
    let token = uri.query().unwrap_or("").to_string();
    if token.contains("t=old") && site.old.fetch_add(1, Ordering::SeqCst) >= 1 {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("Request has expired"))
            .unwrap();
    }
    let total = if token.contains("t=bigger") {
        7 * MB
    } else {
        6 * MB
    };
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

async fn site() -> (std::net::SocketAddr, Site) {
    let site = Site {
        old: Arc::new(AtomicUsize::new(0)),
    };
    let app = axum::Router::new()
        .fallback(answer)
        .with_state(site.clone());
    (common::serve(app).await, site)
}

fn queue(dir: &std::path::Path) -> Queue {
    Queue::new(
        client(),
        Options {
            connections: 4,
            attempts: 1,
            ..common::options(dir)
        },
        1,
    )
}

/// What the browser's hand-over carries: the page it came from.
fn from_browser() -> AddOptions {
    AddOptions {
        referrer: "http://page.example/watch".into(),
        fresh_link_ok: true,
        ..AddOptions::default()
    }
}

#[tokio::test]
async fn a_browser_row_whose_address_expired_waits_and_a_fresh_address_takes_its_place() {
    let (addr, _) = site().await;
    let dir = tempdir();
    let q = queue(dir.path());

    let id = q
        .add_with(&format!("http://{addr}/f.bin?t=old"), from_browser())
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert!(
        matches!(&row.state, DownloadState::Failed(why) if why.contains(dm_core::LINK_EXPIRED)),
        "{:?}",
        row.state
    );
    assert!(row.fresh_link, "the row does not wait for a fresh address");
    assert!(row.progress.total.is_some() && !row.filename.is_empty());

    // The browser hands over the same file at a fresh address.
    let again = q
        .add_with(&format!("http://{addr}/f.bin?t=new"), from_browser())
        .await;
    assert_eq!(again, Some(id), "a second row was added");
    assert_eq!(q.list().await.len(), 1);

    q.run().await;
    let done = q.get(id).await.unwrap();
    assert_eq!(done.state, DownloadState::Completed, "{:?}", done.state);
    assert!(!done.fresh_link);
    assert!(done.url.ends_with("t=new"), "{}", done.url);
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(6 * MB));
}

#[tokio::test]
async fn a_fresh_address_for_another_size_or_name_is_a_new_row_not_the_waiting_one() {
    let (addr, _) = site().await;
    let dir = tempdir();
    let q = queue(dir.path());

    let waiting = q
        .add_with(&format!("http://{addr}/f.bin?t=old"), from_browser())
        .await
        .unwrap();
    q.run().await;
    assert!(q.get(waiting).await.unwrap().fresh_link);

    // The name alone is not enough: another size.
    let other_size = q
        .add_with(&format!("http://{addr}/f.bin?t=bigger"), from_browser())
        .await
        .unwrap();
    assert_ne!(other_size, waiting);
    // The same size under another name.
    let other_name = q
        .add_with(&format!("http://{addr}/g.bin?t=new"), from_browser())
        .await
        .unwrap();
    assert_ne!(other_name, waiting);

    let row = q.get(waiting).await.unwrap();
    assert!(
        row.fresh_link,
        "a different file took the waiting row's place"
    );
    assert!(matches!(row.state, DownloadState::Failed(_)));
    assert_eq!(q.list().await.len(), 3);
}

#[tokio::test]
async fn a_download_typed_in_whose_address_expires_is_failed_as_before() {
    let (addr, _) = site().await;
    let dir = tempdir();
    let q = queue(dir.path());

    // No page, no cookies: nothing came from a browser.
    let id = q
        .add_with(&format!("http://{addr}/f.bin?t=old"), AddOptions::default())
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert!(matches!(row.state, DownloadState::Failed(_)));
    assert!(!row.fresh_link);
    // And a person's Retry still works for a row that waits.
}

#[tokio::test]
async fn a_persons_retry_of_a_waiting_row_works_and_ends_the_wait() {
    let (addr, _) = site().await;
    let dir = tempdir();
    let q = queue(dir.path());

    let id = q
        .add_with(&format!("http://{addr}/f.bin?t=old"), from_browser())
        .await
        .unwrap();
    q.run().await;
    assert!(q.get(id).await.unwrap().fresh_link);
    assert!(q.retry(id).await);
    assert!(
        !q.get(id).await.unwrap().fresh_link,
        "a retried row went on waiting"
    );
}

#[tokio::test]
async fn the_wait_is_saved_with_the_row_and_survives_a_restart() {
    let (addr, _) = site().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");

    let first = queue(dir.path());
    first.persist_to(store.clone()).await;
    let id = first
        .add_with(&format!("http://{addr}/f.bin?t=old"), from_browser())
        .await
        .unwrap();
    first.run().await;
    assert!(first.get(id).await.unwrap().fresh_link);
    first.flush().await;

    // A new run of dm reads the row back still waiting, and the fresh
    // address of the browser still finds it.
    let second = queue(dir.path());
    second.persist_to(store).await;
    let row = second.get(id).await.unwrap();
    assert!(row.fresh_link, "the wait was lost with the restart");
    let again = second
        .add_with(&format!("http://{addr}/f.bin?t=new"), from_browser())
        .await;
    assert_eq!(again, Some(id));
}
