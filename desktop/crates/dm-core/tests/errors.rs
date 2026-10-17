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
    assert_eq!(fetched.load(Ordering::SeqCst), 0, "ranges were fetched");
}
