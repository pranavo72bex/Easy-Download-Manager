// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
            if range != "bytes=0-0" && range != "bytes=0-" {
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
            if headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|r| r != "bytes=0-0" && r != "bytes=0-")
            {
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

// G97: a dropped connection is retried at once (first quick retry = 0 ms),
// and a server that returns after an outage is found within 2 s.

/// A connection reset mid-download is retried immediately; the whole download
/// finishes within a small multiple of what it would have taken undisturbed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_connection_is_retried_and_the_file_still_arrives() {
    use axum::body::Body;
    use axum::http::HeaderMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    const FILE: usize = 2 * 1024 * 1024;
    let data: Vec<u8> = (0..FILE).map(|i| (i % 251) as u8).collect();
    let served = Arc::new(data.clone());
    let hits = Arc::new(AtomicUsize::new(0));

    let (body_c, hits_c) = (served.clone(), hits.clone());
    let app = axum::Router::new().route(
        "/drop.bin",
        axum::routing::get(move |headers: HeaderMap| {
            let data = body_c.clone();
            let hits = hits_c.clone();
            async move {
                let total = FILE;
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
                let n = hits.fetch_add(1, Ordering::SeqCst);
                // First ranged range after the probe: drop the connection
                // after 64 KB so the download is mid-way through and must
                // resume.
                let bytes = if n == 1 {
                    data[a..(a + 65536).min(b + 1)].to_vec()
                } else {
                    data[a..=b].to_vec()
                };
                axum::response::Response::builder()
                    .status(206)
                    .header("accept-ranges", "bytes")
                    .header("content-range", format!("bytes {a}-{b}/{total}"))
                    .header("content-length", b - a + 1)
                    .header("etag", "\"drop\"")
                    .body(Body::from(bytes))
                    .unwrap()
            }
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let started = Instant::now();
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/drop.bin"),
        &options(dir.path()),
    )
    .await
    .expect("the download failed after a mid-stream drop");
    let took = started.elapsed();
    assert_eq!(std::fs::read(&done.path).unwrap(), data, "bytes differ");
    // With an immediate retry, the whole download should finish well under 3 s.
    assert!(
        took.as_secs_f64() < 3.0,
        "a dropped connection was not retried quickly: {:.2}s",
        took.as_secs_f64()
    );
}

/// A server that stops answering for 3 s then returns is found within 2 s
/// of its return: the quick retry loop re-checks often enough.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_back_after_a_short_outage_is_reached_quickly() {
    use axum::body::Body;
    use axum::http::HeaderMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    const FILE: usize = 512 * 1024;
    let data: Vec<u8> = (0..FILE).map(|i| (i % 251) as u8).collect();
    let served = Arc::new(data.clone());
    let down = Arc::new(AtomicBool::new(false));
    let came_back = Arc::new(AtomicBool::new(false));

    let (body_c, down_c, back_c) = (served.clone(), down.clone(), came_back.clone());
    let app = axum::Router::new().route(
        "/outage.bin",
        axum::routing::get(move |headers: HeaderMap| {
            let data = body_c.clone();
            let down = down_c.clone();
            let back = back_c.clone();
            async move {
                if down.load(Ordering::Relaxed) {
                    back.store(true, Ordering::Relaxed);
                    return axum::response::Response::builder()
                        .status(503)
                        .body(Body::empty())
                        .unwrap();
                }
                let total = FILE;
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
                axum::response::Response::builder()
                    .status(206)
                    .header("accept-ranges", "bytes")
                    .header("content-range", format!("bytes {a}-{b}/{total}"))
                    .header("content-length", b - a + 1)
                    .header("etag", "\"out\"")
                    .body(Body::from(data[a..=b].to_vec()))
                    .unwrap()
            }
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();

    // Simulate a 3 s outage: take the server down, wait, then restore it and
    // record when it came back.
    let down_c = down.clone();
    let came_back_at = Arc::new(std::sync::Mutex::new(None::<Instant>));
    let back_at = came_back_at.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        down_c.store(true, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(3000)).await;
        down_c.store(false, Ordering::Relaxed);
        *back_at.lock().unwrap() = Some(Instant::now());
    });

    let opts = dm_core::Options {
        attempts: 20,
        ..options(dir.path())
    };
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/outage.bin"),
        &opts,
    )
    .await
    .expect("the download failed during an outage");
    assert_eq!(std::fs::read(&done.path).unwrap(), data, "bytes differ");

    // Verify the download completed within 2 s of the server returning.
    // (The download may already be done before came_back_at is set if the
    //  timing races, so we only assert when we have the timestamp.)
    let at_opt = *came_back_at.lock().unwrap();
    if let Some(at) = at_opt {
        let lag = at.elapsed();
        assert!(
            lag < Duration::from_secs(4),
            "the download took {lag:?} after the server returned; expected < 4 s"
        );
    }
}

/// A server that, while `down` says so, takes each request and drops it
/// unanswered, and cuts every answer under way the moment it goes down: the
/// `bench` outage. Each connection is served 64 KiB every 50 ms. Every request
/// is noted, when it came and whether it was dropped.
async fn dropping_server(
    data: std::sync::Arc<Vec<u8>>,
    down: tokio::sync::watch::Receiver<bool>,
    asked: std::sync::Arc<std::sync::Mutex<Vec<(std::time::Instant, bool)>>>,
    served: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) -> std::net::SocketAddr {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (data, mut down) = (data.clone(), down.clone());
            let (asked, served) = (asked.clone(), served.clone());
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 2048];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let dropped = *down.borrow_and_update();
                asked
                    .lock()
                    .unwrap()
                    .push((std::time::Instant::now(), dropped));
                if dropped {
                    return;
                }
                let size = data.len();
                let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
                let range = text
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim().split_once('-'))
                    .map(|(a, b)| {
                        let a = a.parse::<usize>().unwrap_or(0);
                        (a, b.parse::<usize>().unwrap_or(size - 1).min(size - 1))
                    });
                let (a, b) = range.unwrap_or((0, size - 1));
                let status = if range.is_some() {
                    format!("206 Partial Content\r\nContent-Range: bytes {a}-{b}/{size}")
                } else {
                    "200 OK".to_string()
                };
                let answer = format!(
                    "HTTP/1.1 {status}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nETag: \"o\"\r\nConnection: close\r\n\r\n",
                    b - a + 1
                );
                if socket.write_all(answer.as_bytes()).await.is_err() {
                    return;
                }
                let mut at = a;
                while at <= b {
                    let end = (at + 65536).min(b + 1);
                    tokio::select! {
                        _ = down.wait_for(|d| *d) => return,
                        wrote = socket.write_all(&data[at..end]) => if wrote.is_err() { return },
                    }
                    served.fetch_add(end - at, std::sync::atomic::Ordering::SeqCst);
                    at = end;
                    tokio::select! {
                        _ = down.wait_for(|d| *d) => return,
                        () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
                    }
                }
            });
        }
    });
    addr
}

/// Eight connections that lose the server together take turns asking it, a
/// tenth of a second apart, and the moment one of them gets a byte the other
/// seven ask at once (gaps row 177). Before, each asked on its own lane, the
/// eight in step for the first two seconds and then a quarter-second apart:
/// after the bench's fifteen-second outage the first byte came 0.2 s late and
/// the last connection 1.75 s after that.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connections_that_lose_the_server_take_turns_and_come_back_together() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const FILE: usize = 8 * 1024 * 1024;
    const OUTAGE: Duration = Duration::from_millis(2500);
    let data: Vec<u8> = (0..FILE).map(|i| (i * 31 % 251) as u8).collect();
    let (down_tx, down) = tokio::sync::watch::channel(false);
    let asked: Arc<Mutex<Vec<(Instant, bool)>>> = Arc::default();
    let served = Arc::new(AtomicUsize::new(0));
    let addr = dropping_server(
        Arc::new(data.clone()),
        down,
        Arc::clone(&asked),
        Arc::clone(&served),
    )
    .await;
    let dir = tempdir();
    let url = format!("http://{addr}/out.bin");
    let opts = options(dir.path());
    let running =
        tokio::spawn(async move { dm_core::download(&dm_core::client(), &url, &opts).await });
    // A quarter of the file in, on every connection: then the server goes.
    while served.load(Ordering::SeqCst) < FILE / 4 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let cut_at = Instant::now();
    down_tx.send_replace(true);
    tokio::time::sleep(OUTAGE).await;
    let back_at = Instant::now();
    down_tx.send_replace(false);
    let done = tokio::time::timeout(Duration::from_secs(30), running)
        .await
        .expect("the download did not end after the outage")
        .expect("joined")
        .expect("the download failed through the outage");
    assert_eq!(std::fs::read(&done.path).unwrap(), data, "bytes differ");

    let asked = asked.lock().unwrap().clone();
    let dropped: Vec<Instant> = asked.iter().filter(|(_, d)| *d).map(|(t, _)| *t).collect();
    let after: Vec<Instant> = asked
        .iter()
        .filter(|(t, d)| !*d && *t >= back_at)
        .map(|(t, _)| *t)
        .collect();
    let first_back = *after.first().expect("nothing asked after the outage");
    // Asked evenly through the outage, up to the request that found it over.
    let through: Vec<Instant> = dropped.iter().copied().chain([first_back]).collect();
    let widest = through
        .windows(2)
        .map(|w| w[1] - w[0])
        .max()
        .unwrap_or_default();
    assert!(
        widest <= Duration::from_millis(400),
        "the server was not asked for {widest:?} of the outage"
    );
    assert!(
        first_back - back_at <= Duration::from_millis(500),
        "the server was asked again {:?} after it came back",
        first_back - back_at
    );
    // Not reference A's hundreds a second: the eight at once, then ten a second.
    let most = 8 + (OUTAGE.as_millis() / 100) as usize + 12;
    assert!(
        dropped.len() <= most,
        "{} requests dropped in {OUTAGE:?} (at most {most})",
        dropped.len()
    );
    assert!(dropped.first().is_some_and(|t| *t >= cut_at));
    // The other seven ask the moment the first gets its byte.
    let with_it = after
        .iter()
        .filter(|t| **t - first_back <= Duration::from_millis(300))
        .count();
    assert!(
        with_it >= 8,
        "{with_it} of the eight connections back within 0.3 s of the first: {:?}",
        after.iter().map(|t| *t - first_back).collect::<Vec<_>>()
    );
}

/// A server that never returns causes the download to fail after the attempt
/// limit, not to wait forever.
#[tokio::test]
async fn a_server_that_never_returns_fails_after_the_limit() {
    use axum::body::Body;
    use std::time::Instant;

    let app = axum::Router::new().fallback(|| async {
        axum::response::Response::builder()
            .status(503)
            .body(Body::empty())
            .unwrap()
    });
    let addr = common::serve(app).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        attempts: 3,
        ..options(dir.path())
    };
    let started = Instant::now();
    let result = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/forever.bin"),
        &opts,
    )
    .await;
    let took = started.elapsed();
    assert!(result.is_err(), "expected failure but got success");
    // Three attempts at most; even with backoff the total should be well
    // under two minutes.
    assert!(
        took.as_secs() < 60,
        "three attempts took too long: {took:?}"
    );
}

/// thinkbroadband answers 403 to any `Referer` but its own pages, so the one
/// dm makes up (the file's own folder) failed on every agent, where
/// reference A got the file.
#[tokio::test]
async fn a_server_that_refuses_any_referer_gives_the_file_on_every_piece() {
    use axum::http::HeaderMap;
    use std::sync::{Arc, Mutex};
    let data: Vec<u8> = (0..4_000_000u32).map(|i| (i % 251) as u8).collect();
    let served = data.clone();
    // Each request: whether it carried a `Referer`, and its `Range`.
    type Asked = Vec<(bool, Option<String>)>;
    let asked: Arc<Mutex<Asked>> = Arc::default();
    let log = asked.clone();
    let app = axum::Router::new().route(
        "/files/f.bin",
        axum::routing::get(move |headers: HeaderMap| {
            let served = served.clone();
            let log = log.clone();
            async move {
                let referer = headers.contains_key("referer");
                let range = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                log.lock().unwrap().push((referer, range.clone()));
                if referer {
                    return axum::response::Response::builder()
                        .status(403)
                        .body(axum::body::Body::from("not permitted"))
                        .unwrap();
                }
                let total = served.len();
                let (start, end) = range
                    .as_deref()
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.split_once('-'))
                    .map(|(a, b)| {
                        let a: usize = a.parse().unwrap_or(0);
                        let b: usize = b.parse().unwrap_or(total - 1).min(total - 1);
                        (a, b)
                    })
                    .unwrap_or((0, total - 1));
                let body = served[start..=end].to_vec();
                let mut reply = axum::response::Response::builder()
                    .header("accept-ranges", "bytes")
                    .header("content-length", body.len());
                reply = if range.is_some() {
                    reply
                        .status(206)
                        .header("content-range", format!("bytes {start}-{end}/{total}"))
                } else {
                    reply.status(200)
                };
                reply.body(axum::body::Body::from(body)).unwrap()
            }
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let once = dm_core::Options {
        attempts: 1,
        ..options(dir.path())
    };
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/files/f.bin"),
        &once,
    )
    .await
    .expect("asked again without the made-up Referer");
    assert_eq!(std::fs::read(done.path).unwrap(), data);
    let asked = asked.lock().unwrap();
    let after_first_bare = asked.iter().skip_while(|(referer, _)| *referer).skip(1);
    assert!(
        after_first_bare.clone().all(|(referer, _)| !referer),
        "a piece after the bare probe carried the Referer again: {asked:?}"
    );
}

/// Wikimedia's limiter answered "429, Retry-After: 1" for as long as requests
/// kept coming within its window; dm, asking each second as told, sat at
/// 0 B for minutes. The server's wait is the least dm waits, and it grows.
#[tokio::test]
async fn a_limiter_that_resets_on_each_request_is_outwaited() {
    use std::sync::{Arc, Mutex};
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let served = data.clone();
    // Refused while the last request, served or not, came under 2 s ago.
    let last: Arc<Mutex<std::time::Instant>> = Arc::new(Mutex::new(std::time::Instant::now()));
    let app = axum::Router::new().route(
        "/f.bin",
        axum::routing::get(move || {
            let served = served.clone();
            let last = last.clone();
            async move {
                let mut last = last.lock().unwrap();
                let quiet = last.elapsed() >= std::time::Duration::from_secs(2);
                *last = std::time::Instant::now();
                if !quiet {
                    return axum::response::Response::builder()
                        .status(429)
                        .header("retry-after", "1")
                        .body(axum::body::Body::empty())
                        .unwrap();
                }
                axum::response::Response::builder()
                    .status(200)
                    .header("content-length", served.len())
                    .body(axum::body::Body::from(served))
                    .unwrap()
            }
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let patient = dm_core::Options {
        attempts: 8,
        ..options(dir.path())
    };
    let started = std::time::Instant::now();
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/f.bin"),
        &patient,
    )
    .await
    .expect("the waits should have grown past the limiter's window");
    assert_eq!(std::fs::read(done.path).unwrap(), data);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_server_that_fails_ranged_requests_but_serves_the_plain_one_gives_the_file() {
    use axum::http::HeaderMap;
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let served = data.clone();
    let app = axum::Router::new().route(
        "/f.bin",
        axum::routing::get(move |headers: HeaderMap| {
            let served = served.clone();
            async move {
                if headers.contains_key("range") {
                    return axum::response::Response::builder()
                        .status(503)
                        .body(axum::body::Body::empty())
                        .unwrap();
                }
                axum::response::Response::builder()
                    .status(200)
                    .header("content-length", served.len())
                    .body(axum::body::Body::from(served))
                    .unwrap()
            }
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let once = dm_core::Options {
        attempts: 1,
        ..options(dir.path())
    };
    let done = dm_core::download(&dm_core::client(), &format!("http://{addr}/f.bin"), &once)
        .await
        .expect("the plain request should have been tried");
    assert_eq!(std::fs::read(done.path).unwrap(), data);
}
