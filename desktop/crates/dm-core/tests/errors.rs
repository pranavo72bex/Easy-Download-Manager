//! What a failed connection says to the person reading the row.

mod common;
use common::{options, tempdir};

async fn failure(url: &str) -> String {
    let dir = tempdir();
    let once = dm_core::Options {
        attempts: 1,
        ..options(dir.path())
    };
    dm_core::download(&dm_core::client(), url, &once)
        .await
        .unwrap_err()
        .to_string()
}

#[tokio::test]
async fn a_name_that_does_not_resolve_is_said_plainly() {
    // `.invalid` never resolves, by RFC 2606.
    let said = failure("http://no-such-server.invalid/f.bin").await;
    assert!(
        said.starts_with("the server's name could not be found (check the address)"),
        "{said}"
    );
    assert!(!said.contains("error sending request"), "{said}");
}

#[tokio::test]
async fn a_refused_connection_is_said_plainly() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let said = failure(&format!("http://127.0.0.1:{port}/f.bin")).await;
    assert!(
        said.starts_with("the server refused the connection"),
        "{said}"
    );
    assert!(!said.contains("error sending request"), "{said}");
}

#[tokio::test]
async fn a_refusal_whose_body_never_ends_is_answered_promptly() {
    /* The body of a 500 was read whole, decompressed, before it was cut to
     * an explanation's length: one that never ended held the download and
     * grew without bound. Read through a whole download, with one attempt. */
    use axum::body::Body;
    let endless = || async {
        let chunk = bytes::Bytes::from(vec![b'x'; 64 * 1024]);
        let forever =
            futures_util::stream::repeat_with(move || Ok::<_, std::io::Error>(chunk.clone()));
        axum::response::Response::builder()
            .status(500)
            .header("content-type", "text/plain")
            .body(Body::from_stream(forever))
            .unwrap()
    };
    let addr = common::serve(axum::Router::new().fallback(endless)).await;
    let said = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        failure(&format!("http://{addr}/f.bin")),
    )
    .await
    .expect("a refusal's endless body was read to its end");
    assert!(said.contains("500"), "{said}");
}

/// Gap 60: a stated 100 TB was fetched until the disk was full. It is
/// refused before a byte is fetched, saying how much it needs.
#[tokio::test]
async fn a_file_larger_than_the_disk_is_refused_before_it_is_fetched() {
    use axum::body::Body;
    use axum::http::HeaderMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    const HUGE: u64 = 100 * 1024 * 1024 * 1024 * 1024 * 1024;
    let fetched = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fetched);
    let handler = move |headers: HeaderMap| {
        let counter = Arc::clone(&counter);
        async move {
            let range = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            if range != "bytes=0-0" {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            axum::response::Response::builder()
                .status(206)
                .header("content-range", format!("bytes 0-0/{HUGE}"))
                .header("content-length", 1)
                .body(Body::from(vec![0u8]))
                .unwrap()
        }
    };
    let addr = common::serve(axum::Router::new().fallback(handler)).await;
    let said = failure(&format!("http://{addr}/huge.iso")).await;
    assert!(said.starts_with("not enough room"), "{said}");
    assert!(said.contains("100.0 PB"), "{said}");
    // The download folder the person chose, not dm's staging folder.
    assert!(!said.contains("parts"), "{said}");
    assert_eq!(fetched.load(Ordering::SeqCst), 0, "ranges were fetched");
}

/// Gap 94: two downloads started together each checked the free space
/// alone, so together they could still fill the disk. The second is
/// checked against what the first still needs, and passes once the first
/// has ended.
#[tokio::test]
async fn downloads_under_way_count_against_the_free_space_of_the_next() {
    use axum::body::Body;
    const SIZE: usize = 6 * 1024 * 1024;
    let app = axum::Router::new()
        .route(
            "/stalled.bin",
            axum::routing::get(|| async {
                let never = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
                axum::response::Response::builder()
                    .header("content-length", SIZE)
                    .body(Body::from_stream(never))
                    .unwrap()
            }),
        )
        .route(
            "/whole.bin",
            axum::routing::get(|| async { Body::from(vec![7u8; SIZE]) }),
        );
    let addr = common::serve(app).await;
    let dir = tempdir();
    // A disk with room for one of the two, not both.
    let room = dm_core::Room::with_free_space(|_| Some(10 * 1024 * 1024));
    let base = |name: &str| dm_core::Options {
        attempts: 1,
        parts_dir: dir.path().join(name),
        dest_dir: dir.path().join("out"),
        room: room.clone(),
        ..options(dir.path())
    };

    let first = base("first");
    let control = first.control.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let url = format!("http://{addr}/stalled.bin");
    let running = tokio::spawn(async move {
        dm_core::download_reporting(&dm_core::client(), &url, &first, Some(tx)).await
    });
    // Named is sent once the free-space check has passed and claimed.
    while !matches!(rx.recv().await, Some(dm_core::Update::Named { .. }) | None) {}

    let whole = format!("http://{addr}/whole.bin");
    let said = dm_core::download(&dm_core::client(), &whole, &base("second"))
        .await
        .unwrap_err()
        .to_string();
    assert!(said.starts_with("not enough room"), "{said}");
    assert!(said.contains("under way"), "{said}");

    control.cancel();
    let _ = running.await;
    let done = dm_core::download(&dm_core::client(), &whole, &base("third"))
        .await
        .expect("refused after the first download ended");
    assert_eq!(done.bytes, SIZE as u64);
}
