//! A connection that finished its piece takes the next one on the same
//! socket, without a new connect and sign-in (docs/reference-behavior.md). The server here keeps connections open and
//! counts both what it accepted and what it was asked.

mod common;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::{options, tempdir};

const SIZE: usize = 32 * 1024 * 1024;

/// Serves `served` with ranges and keep-alive on every connection, counting
/// connections accepted and requests answered.
fn keep_alive_server(served: Arc<Vec<u8>>) -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (accepted, asked) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (a, q) = (accepted.clone(), asked.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            a.fetch_add(1, Ordering::SeqCst);
            let (served, q) = (served.clone(), q.clone());
            std::thread::spawn(move || {
                let mut stream = stream;
                loop {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        if stream.read(&mut byte).unwrap_or(0) == 0 {
                            return;
                        }
                        head.push(byte[0]);
                    }
                    q.fetch_add(1, Ordering::SeqCst);
                    let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
                    let head_only = head.starts_with("head ");
                    let range = head
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|r| {
                            let (a, b) = r.trim().split_once('-')?;
                            let a: usize = a.parse().ok()?;
                            let b = b.parse::<usize>().map_or(SIZE - 1, |b| b.min(SIZE - 1));
                            Some((a, b))
                        });
                    let (status, first, last) = match range {
                        Some((a, b)) => ("206 Partial Content", a, b),
                        None => ("200 OK", 0, SIZE - 1),
                    };
                    let part = &served[first..=last];
                    let said = format!(
                        "HTTP/1.1 {status}\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {first}-{last}/{SIZE}\r\nContent-Length: {}\r\n\r\n",
                        part.len()
                    );
                    if stream.write_all(said.as_bytes()).is_err() {
                        return;
                    }
                    if !head_only && stream.write_all(part).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (format!("http://{addr}/reuse.bin"), accepted, asked)
}

#[tokio::test]
async fn pieces_after_the_first_are_fetched_on_connections_already_open() {
    let body: Arc<Vec<u8>> = Arc::new((0..SIZE).map(|i| (i as u8).wrapping_mul(31)).collect());
    let (url, accepted, asked) = keep_alive_server(body.clone());
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = dm_core::download(&dm_core::client(), &url, &opts)
        .await
        .expect("the download failed");
    assert_eq!(std::fs::read(done.path).unwrap(), *body);
    let (accepted, asked) = (
        accepted.load(Ordering::SeqCst),
        asked.load(Ordering::SeqCst),
    );
    eprintln!("{asked} requests on {accepted} connections");
    // 32 MB over 4 connections: more requests than connections means pieces
    // after the first four were fetched (how many depends on how fast the
    // machine is, since a piece is halved for a connection that is free:
    // 7 on a busy CI runner, 16 or more on a quiet one, so no fixed count).
    assert!(
        asked > 4,
        "only {asked} requests on 4 connections: the file was not cut into pieces"
    );
    // Fewer connections than requests: at least one carried a second
    // piece. (A fixed ceiling read as flaky on a slow machine, which opens
    // a few more before the first piece is done.)
    assert!(
        accepted < asked,
        "{accepted} connections for {asked} requests: finished connections were not reused"
    );
}
