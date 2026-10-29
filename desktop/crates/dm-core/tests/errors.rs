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

/// A stated 100 TB was fetched until the disk was full. It is
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

/// A file over 4 GB bound for a FAT32 drive is refused
/// before a byte is fetched, naming why and what to do, where it used to
/// run to 4 GB and fail there. A test's drive holds nothing over 1,000
/// bytes.
#[tokio::test]
async fn a_file_too_large_for_the_drive_is_refused_before_it_is_fetched() {
    use axum::body::Body;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let fetched = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fetched);
    let handler = move |headers: axum::http::HeaderMap| {
        let counter = Arc::clone(&counter);
        async move {
            if headers.get("range").and_then(|v| v.to_str().ok()) != Some("bytes=0-0") {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            axum::response::Response::builder()
                .status(206)
                .header("content-range", "bytes 0-0/5000")
                .header("content-length", 1)
                .body(Body::from(vec![0u8]))
                .unwrap()
        }
    };
    let addr = common::serve(axum::Router::new().fallback(handler)).await;
    let dir = tempdir();
    let small = dm_core::Options {
        room: dm_core::Room::with_largest_file(|_| Some(1000)),
        ..options(dir.path())
    };
    let said = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/big.iso"),
        &small,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.starts_with("too large for this drive"), "{said}");
    assert!(said.contains("FAT32"), "{said}");
    assert_eq!(fetched.load(Ordering::SeqCst), 0, "ranges were fetched");
}

/// Two downloads started together each checked the free space
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

/// A response whose headers are beyond any limit is read the same way each
/// time it is asked for: it fails at once, not after the back-off between
/// attempts that is for a server having a bad moment.
#[tokio::test]
async fn a_response_with_unreadable_headers_is_not_retried() {
    use std::io::Write;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut stream = stream;
            let mut buf = [0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nX-Pad: ");
            let pad = vec![b'x'; 1024 * 1024];
            let _ = stream.write_all(&pad);
            let _ = stream.write_all(b"\r\n\r\n0123456789");
        }
    });
    let dir = tempdir();
    let opts = dm_core::Options {
        attempts: 5,
        ..options(dir.path())
    };
    let started = std::time::Instant::now();
    let result =
        dm_core::download(&dm_core::client(), &format!("http://{addr}/f.bin"), &opts).await;
    assert!(
        result.is_err(),
        "a response of unreadable headers was taken as a file"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?} to give up on an answer that cannot be read",
        started.elapsed()
    );
    assert!(
        hits.load(std::sync::atomic::Ordering::SeqCst) <= 3,
        "asked {} times",
        hits.load(std::sync::atomic::Ordering::SeqCst)
    );
}

/// A server that sends every byte a range asked for but says in
/// `Content-Length` that more are coming, then hangs up. The range is whole:
/// it was asked again for ever, the row at 100 % and "running" (BUGLOG.md,
/// BUG-001, found by qa/tests `length_wrong`).
#[tokio::test]
async fn a_range_that_came_whole_under_a_longer_stated_length_is_finished() {
    use std::io::{Read, Write};
    const SIZE: usize = 300_000;
    let body: Vec<u8> = (0..SIZE).map(|i| (i * 7 % 251) as u8).collect();
    let served = body.clone();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let served = served.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
                let range = head
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .map(|r| {
                        let (a, b) = r.trim().split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b = b.parse::<usize>().map_or(SIZE - 1, |b| b.min(SIZE - 1));
                        (a, b)
                    });
                let (status, first, last) = match range {
                    Some((a, b)) => ("206 Partial Content", a, b),
                    None => ("200 OK", 0, SIZE - 1),
                };
                let part = &served[first..=last];
                let said = format!(
                    "HTTP/1.1 {status}\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {first}-{last}/{SIZE}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    part.len() + 1000
                );
                let _ = stream.write_all(said.as_bytes());
                let _ = stream.write_all(part);
                let _ = stream.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        dm_core::download(&dm_core::client(), &format!("http://{addr}/lie.bin"), &opts),
    )
    .await
    .expect("a range that came whole was asked for again and again");
    let finished = done.expect("the download failed");
    assert_eq!(std::fs::read(finished.path).unwrap(), body);
}

/// A certificate the client does not trust is an answer, not a bad moment:
/// asking again gets the same certificate. It was retried as a failed
/// connection, the row "running" at nothing for over two minutes before it
/// said why (BUGLOG.md, BUG-002, found by qa/tests `bad_certificates`).
#[tokio::test]
async fn an_untrusted_certificate_is_said_at_once_not_retried() {
    use tokio_rustls::rustls;
    let made = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let key = rustls::pki_types::PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![made.cert.der().clone()], key)
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = hits.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = acceptor.accept(tcp).await;
            });
        }
    });
    let dir = tempdir();
    let opts = dm_core::Options {
        attempts: 10,
        ..options(dir.path())
    };
    let started = std::time::Instant::now();
    let said = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        dm_core::download(
            &dm_core::client(),
            &format!("https://localhost:{port}/f.bin"),
            &opts,
        ),
    )
    .await
    .expect("still trying after a minute")
    .unwrap_err()
    .to_string();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?} to give up on a certificate that will not change",
        started.elapsed()
    );
    assert!(said.to_lowercase().contains("certificate"), "{said}");
    assert!(
        hits.load(std::sync::atomic::Ordering::SeqCst) <= 2,
        "asked {} times",
        hits.load(std::sync::atomic::Ordering::SeqCst)
    );
}
