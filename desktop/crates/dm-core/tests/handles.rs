// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Whether the engine keeps Windows handles after downloads (gaps row 156):
//! it does not, with one range or four, one client for all or one each
//! (2026-11-07: +4, +4 and +0 over forty downloads each). What the app kept
//! was each finished download's toast's (`dm-desktop` `tests/handles.rs`).

#![cfg(windows)]

mod common;
use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use common::{options, tempdir};

fn handles() -> u64 {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-Process -Id {}).HandleCount", std::process::id()),
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

async fn serve_file(size: u32) -> std::net::SocketAddr {
    let data: std::sync::Arc<Vec<u8>> = std::sync::Arc::new((0..size).map(|i| i as u8).collect());
    let app = axum::Router::new().fallback(get(move |headers: HeaderMap| {
        let data = data.clone();
        async move {
            let total = data.len();
            let (from, to) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .map(|(a, b)| {
                    (
                        a.parse::<usize>().unwrap(),
                        b.parse::<usize>().unwrap_or(total - 1).min(total - 1),
                    )
                })
                .unwrap_or((0, total - 1));
            Response::builder()
                .status(206)
                .header("content-range", format!("bytes {from}-{to}/{total}"))
                .header("content-length", (to - from + 1).to_string())
                .body(Body::from(data[from..=to].to_vec()))
                .unwrap()
        }
    }));
    common::serve(app).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a measurement: cargo test -p dm-core --test handles -- --ignored --nocapture"]
async fn handles_after_downloads() {
    let one_range = serve_file(256 * 1024).await;
    let ranged = serve_file(4 * 1024 * 1024).await;
    let dir = tempdir();
    let shared = dm_core::client();
    let run = |addr: std::net::SocketAddr, tag: &'static str, own: bool| {
        let dir = dir.path().to_path_buf();
        let shared = shared.clone();
        async move {
            for i in 0..40 {
                let o = options(&dir.join(format!("{tag}{i}")));
                let client = if own {
                    dm_core::client_with(&dm_core::Proxy::default()).unwrap()
                } else {
                    shared.clone()
                };
                dm_core::download(&client, &format!("http://{addr}/{tag}{i}.bin"), &o)
                    .await
                    .unwrap();
            }
        }
    };
    // Warm up: the runtime's threads and the first connections.
    run(ranged, "w", false).await;
    tokio::time::sleep(std::time::Duration::from_secs(12)).await;
    let mut before = handles();
    println!("HANDLES at rest {before}");
    for (addr, tag, own, what) in [
        (one_range, "a", false, "one range each, one client"),
        (ranged, "b", false, "four ranges each, one client"),
        (ranged, "c", true, "four ranges each, a client each"),
    ] {
        run(addr, tag, own).await;
        tokio::time::sleep(std::time::Duration::from_secs(12)).await;
        let after = handles();
        println!(
            "HANDLES after 40 downloads, {what}: {after} ({:+})",
            after as i64 - before as i64
        );
        before = after;
    }
}
