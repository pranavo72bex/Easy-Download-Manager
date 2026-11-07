// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download that needs a signed-in session, made with a `cookies.txt`.
//!
//! The extension leaves the browser's cookies alone, so such a download used
//! to finish in the browser. With a cookies file exported from the browser,
//! dm sends the matching cookies itself. A test binary of its own: the
//! cookies in use are the process's.

mod common;

use std::path::Path;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, download};

use common::{options, serve, tempdir};

/// The cookies in use are the process's, so the tests here take turns.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn signed_in_only() -> axum::Router {
    axum::Router::new().route(
        "/private.bin",
        get(|headers: HeaderMap| async move {
            let cookie = headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !cookie.split("; ").any(|c| c == "session=1") {
                return Response::builder()
                    .status(403)
                    .body(Body::from("sign in"))
                    .unwrap();
            }
            Response::builder()
                .status(200)
                .header("content-length", 11)
                .body(Body::from("only for me"))
                .unwrap()
        }),
    )
}

#[tokio::test]
async fn a_cookies_file_signs_dms_requests_in() {
    let _one = ONE_AT_A_TIME.lock().await;
    let addr = serve(signed_in_only()).await;
    let dir = tempdir();
    let url = format!("http://{addr}/private.bin");

    // No cookies: refused, as the signed-in browser alone could fetch it.
    dm_core::cookies::use_file(Path::new("")).unwrap();
    let refused = download(&client(), &url, &options(dir.path())).await;
    assert!(refused.is_err(), "fetched without being signed in");

    // The browser's cookies, exported: the same download completes.
    let file = dir.path().join("cookies.txt");
    std::fs::write(
        &file,
        "# Netscape HTTP Cookie File\n127.0.0.1\tFALSE\t/\tFALSE\t0\tsession\t1\n\
         other.example\tTRUE\t/\tFALSE\t0\tnot\tthis\n",
    )
    .unwrap();
    assert_eq!(dm_core::cookies::use_file(&file).unwrap(), 2);
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("the cookies file did not sign the download in");
    assert_eq!(std::fs::read(&done.path).unwrap(), b"only for me");

    // A file that cannot be read is said, and changes nothing.
    assert!(dm_core::cookies::use_file(&dir.path().join("missing.txt")).is_err());
    assert_eq!(dm_core::cookies::file().as_deref(), Some(file.as_path()));
}

#[tokio::test]
async fn a_handed_over_sign_in_is_its_downloads_alone_and_for_its_server_alone() {
    let _one = ONE_AT_A_TIME.lock().await;
    let addr = serve(signed_in_only()).await;
    let dir = tempdir();
    let url = format!("http://{addr}/private.bin");
    dm_core::cookies::use_file(Path::new("")).unwrap();

    // Through the queue, as a hand-off arrives: the row carries them and
    // its download sends them.
    let q = dm_core::Queue::new(client(), options(dir.path()), 1);
    let id = q
        .add_with(
            &url,
            dm_core::AddOptions {
                cookies: "session=1; theme=dark".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed, "{row:?}");
    assert_eq!(std::fs::read(&row.path).unwrap(), b"only for me");
    assert_eq!(
        row.cookies, "session=1; theme=dark",
        "the row forgot its sign-in"
    );

    // A video page's session, handed over for that download alone, signs
    // it in and is not kept once it is done.
    let once = q
        .add_with(
            &format!("{url}?video"),
            dm_core::AddOptions {
                cookies: "session=1".into(),
                cookies_once: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(once).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed, "{row:?}");
    assert_eq!(row.cookies, "", "a one-download session was kept");
    assert!(!row.cookies_once);

    // A later download from the same site, handed over without them, does
    // not borrow the first one's.
    let other = q.add(&format!("{url}?again")).await.unwrap();
    q.run().await;
    assert!(
        matches!(q.get(other).await.unwrap().state, dm_core::State::Failed(_)),
        "a download that was never signed in was sent another's cookies"
    );

    // An address edited onto another server drops them; one on the same
    // server keeps them.
    let later = |cookies: &str| dm_core::AddOptions {
        later: true,
        cookies: cookies.into(),
        ..Default::default()
    };
    let moved = q.add_with(&url, later("session=1")).await.unwrap();
    assert!(
        q.set_url(moved, &format!("http://{addr}/elsewhere.bin"))
            .await
    );
    assert_eq!(
        q.get(moved).await.unwrap().cookies,
        "session=1",
        "same server"
    );
    assert!(q.set_url(moved, "http://other.example/private.bin").await);
    assert_eq!(
        q.get(moved).await.unwrap().cookies,
        "",
        "kept for another server"
    );
}
