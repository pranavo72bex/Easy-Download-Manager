//! What the engine holds on to.
//!
//! Its own file, and deliberately the only test in it. Resident memory is a
//! property of the process, not of a function, and Rust runs the tests in one
//! binary in parallel — measured alongside a suite that allocates 64 MB
//! bodies of its own, the reading says nothing about this code. A file of its
//! own is a process of its own.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use dm_core::{client, download};
use rand::RngCore;

mod common;
use common::{options, serve as spawn, tempdir};

const MB: usize = 1024 * 1024;
const SIZE: usize = 32 * MB;

fn body(size: usize) -> Vec<u8> {
    let mut out = vec![0u8; size];
    rand::thread_rng().fill_bytes(&mut out);
    out
}

/// Ranged, so the download is segmented as a real one would be.
async fn serve(content: Vec<u8>) -> SocketAddr {
    async fn handler(
        State(content): State<std::sync::Arc<Vec<u8>>>,
        headers: HeaderMap,
    ) -> Response {
        let total = content.len() as u64;
        let range = headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes="))
            .map(str::to_owned);

        match range {
            Some(spec) => {
                let (from, to) = spec.split_once('-').unwrap_or(("0", ""));
                let start: u64 = from.parse().unwrap_or(0);
                let end: u64 = to.parse().unwrap_or(total - 1).min(total - 1);
                let slice = content[start as usize..=end as usize].to_vec();
                Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header("content-length", slice.len())
                    .header("content-range", format!("bytes {start}-{end}/{total}"))
                    .body(Body::from(slice))
                    .unwrap()
            }
            None => Response::builder()
                .status(StatusCode::OK)
                .header("content-length", total)
                .body(Body::from(content.to_vec()))
                .unwrap(),
        }
    }

    spawn(
        axum::Router::new()
            .fallback(handler)
            .with_state(std::sync::Arc::new(content)),
    )
    .await
}

#[tokio::test]
async fn repeated_downloads_do_not_grow_the_process() {
    /* The engine streams: a chunk arrives, is written, and is dropped. If
     * anything held on to a chunk, a part, or a whole file, memory would
     * track what has been downloaded rather than what is in flight — and a
     * download manager is a program left running for days.
     *
     * Measured across repeats rather than across one download, because the
     * first of anything grows an allocator's arenas and a connection pool,
     * and calling that a leak would fail this for the wrong reason. */
    if memory_stats::memory_stats().is_none() {
        eprintln!("this platform does not report memory; skipping");
        return;
    }

    let addr = serve(body(SIZE)).await;
    let url = format!("http://{addr}/f.bin");
    let resident = || memory_stats::memory_stats().unwrap().physical_mem;

    let once = |n: usize| {
        let url = url.clone();
        async move {
            let dir = tempdir();
            let done = download(&client(), &url, &options(dir.path()))
                .await
                .unwrap_or_else(|e| panic!("download {n} failed: {e}"));
            assert_eq!(done.bytes, SIZE as u64);
        }
    };

    /* As many times as are measured, so everything allocated once has been:
     * glibc gives each thread that allocates its own arena, and eight
     * connections a download meet more threads than two downloads warm.
     * With two, Arch's makepkg check() saw 21 MB of arenas across six and
     * failed; a file held per download would still be 32 MB each. */
    const REPEATS: usize = 6;
    for n in 0..REPEATS {
        once(n).await;
    }
    let before = resident();

    for n in 0..REPEATS {
        once(n + REPEATS).await;
    }
    let after = resident();

    let grew = after.saturating_sub(before);
    /* Keeping one file per download would be 192 MB across six. The limit
     * is a third of that, because glibc's per-thread arenas can keep growing
     * past any warm-up on some systems, and that is the allocator, not the
     * engine holding anything. */
    assert!(
        grew < 2 * SIZE,
        "six {SIZE}-byte downloads grew the process by {grew} bytes; anything \
         approaching {SIZE} per download means a file is being held"
    );
}
