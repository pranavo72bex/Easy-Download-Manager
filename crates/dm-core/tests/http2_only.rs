// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A server that speaks HTTP/2 alone (gaps row F9): dm asks over HTTP/1.1, as it asks every
//! server, and asks again over HTTP/2 where it is refused.

use axum::body::Body;
use axum::response::Response;
use axum::routing::get;

mod common;
use common::tempdir;

/// What the server serves: a megabyte that an out-of-order join would show.
fn body() -> Vec<u8> {
    (0..1_000_000u32).map(|i| (i % 251) as u8).collect()
}

#[tokio::test]
async fn a_server_that_speaks_http2_alone_is_downloaded_whole() {
    let app = axum::Router::new().route(
        "/f.bin",
        get(|| async { Response::new(Body::from(body())) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let service = hyper_util::service::TowerToHyperService::new(app.clone());
            tokio::spawn(async move {
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .http2_only()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                .await;
            });
        }
    });
    // Asked over HTTP/1.1 alone, it refuses in words dm knows for that.
    let refused = dm_core::client()
        .get(format!("http://{addr}/f.bin"))
        .send()
        .await
        .expect_err("an HTTP/1.1 request was answered");
    // In words, as the engine reads a failure: the error and each cause under it.
    let mut why = refused.to_string();
    let mut cause = std::error::Error::source(&refused);
    while let Some(under) = cause {
        why.push_str(&format!(": {under}"));
        cause = under.source();
    }
    assert!(dm_core::refuses_http1(&why), "{why}");
    let dir = tempdir();
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/f.bin"),
        &common::options(dir.path()),
    )
    .await
    .expect("an HTTP/2-only server is downloaded");
    assert_eq!(std::fs::read(&done.path).unwrap(), body());
}
