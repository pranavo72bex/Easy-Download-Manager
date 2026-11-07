// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download a site guards with a request header (a bearer token), made
//! with the headers the browser's own request carried (gaps row 116).

mod common;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::client;

use common::{options, serve, tempdir};

fn token_only() -> axum::Router {
    axum::Router::new().route(
        "/guarded.bin",
        get(|headers: HeaderMap| async move {
            let ok = headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer t")
                && headers.get("x-play-session").is_some();
            if !ok {
                return Response::builder()
                    .status(403)
                    .body(Body::from("no token"))
                    .unwrap();
            }
            Response::builder()
                .status(200)
                .header("content-length", 7)
                .body(Body::from("guarded"))
                .unwrap()
        }),
    )
}

#[tokio::test]
async fn the_browsers_request_headers_are_sent_on_every_request_of_the_row() {
    let addr = serve(token_only()).await;
    let dir = tempdir();
    let url = format!("http://{addr}/guarded.bin");
    let q = dm_core::Queue::new(client(), options(dir.path()), 1);

    // Without them: refused, as it is to anything that is not the browser.
    let bare = q.add(&url).await.unwrap();
    q.run().await;
    assert!(matches!(
        q.get(bare).await.unwrap().state,
        dm_core::State::Failed(_)
    ));

    // With them, as a hand-off arrives; the ones dm sets itself are dropped.
    let id = q
        .add_with(
            &format!("{url}?again"),
            dm_core::AddOptions {
                headers: vec![
                    ("Authorization".into(), "Bearer t".into()),
                    ("X-Play-Session".into(), "s1".into()),
                    ("Range".into(), "bytes=5-".into()),
                    ("Host".into(), "evil.example".into()),
                ],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed, "{row:?}");
    assert_eq!(std::fs::read(&row.path).unwrap(), b"guarded");
    assert_eq!(row.headers.len(), 2, "{:?}", row.headers);
}

#[test]
fn headers_go_to_their_own_server_only() {
    let sign_in = dm_core::cookies::SignIn::with_headers(
        "https://files.example/a.bin",
        "",
        &[("Authorization".into(), "Bearer t".into())],
    )
    .unwrap();
    assert_eq!(sign_in.headers_for("https://files.example/b.bin").len(), 1);
    assert!(sign_in
        .headers_for("https://mirror.example/b.bin")
        .is_empty());
    assert_eq!(sign_in.for_url("https://files.example/b.bin"), None);
}
