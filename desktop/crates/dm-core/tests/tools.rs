// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The tool dm hands YouTube's solver scripts to (QuickJS): fetched by dm
//! itself where the computer has none.

mod common;

use dm_core::tools::{fetch_into, Sources, Tool};
use futures_util::StreamExt as _;
use sha2::Digest as _;

/// A server standing in for QuickJS's release: one Linux build.
async fn releases() -> String {
    use axum::routing::get;
    let app = axum::Router::new().route("/q/qjs-linux-x86_64", get(|| async { &b"a quickjs"[..] }));
    format!("http://{}", common::serve(app).await)
}

/// What dm fetches is kept only when it matches the build it knows, and is
/// placed whole; a copy that does not match is refused and nothing is left.
#[tokio::test]
async fn fetched_tools_are_kept_only_when_they_match() {
    let base = releases().await;
    let sources = Sources {
        quickjs: format!("{base}/q"),
        quickjs_digests: vec![(
            "qjs-linux-x86_64".into(),
            dm_core::hex::encode(sha2::Sha256::digest(b"a quickjs")),
        )],
    };
    let dir = common::tempdir();
    let client = dm_core::client();
    let quickjs = fetch_into(
        &client,
        Tool::QuickJs,
        dir.path(),
        &sources,
        "linux",
        "x86_64",
    )
    .await
    .expect("the matching QuickJS");
    assert_eq!(std::fs::read(&quickjs).unwrap(), b"a quickjs");
    let lying = Sources {
        quickjs_digests: vec![("qjs-linux-x86_64".into(), "ab".repeat(32))],
        ..sources.clone()
    };
    let elsewhere = common::tempdir();
    let said = fetch_into(
        &client,
        Tool::QuickJs,
        elsewhere.path(),
        &lying,
        "linux",
        "x86_64",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("does not match"), "{said}");
    assert!(std::fs::read_dir(elsewhere.path())
        .unwrap()
        .next()
        .is_none());

    // A system QuickJS publishes no build for.
    let said = fetch_into(
        &client,
        Tool::QuickJs,
        elsewhere.path(),
        &sources,
        "freebsd",
        "x86_64",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("publishes no build"), "{said}");
}

/// The real thing, from GitHub: QuickJS's build for this computer, fetched,
/// checked and run. Network, so only where asked (DM_NETWORK_TESTS=1).
#[tokio::test]
async fn the_official_build_is_fetched_and_run() {
    if std::env::var_os("DM_NETWORK_TESTS").is_none() {
        eprintln!("DM_NETWORK_TESTS not set — skipping");
        return;
    }
    let dir = common::tempdir();
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let path = fetch_into(
        &dm_core::client(),
        Tool::QuickJs,
        dir.path(),
        &dm_core::tools::official(),
        os,
        arch,
    )
    .await
    .unwrap_or_else(|e| panic!("QuickJS: {e}"));
    let ran = std::process::Command::new(&path)
        .arg("--help")
        .output()
        .unwrap();
    assert!(
        ran.status.success() || !ran.stdout.is_empty() || !ran.stderr.is_empty(),
        "QuickJS did not run"
    );
}

/// While a tool is fetched, the bytes so far and the length are told, for
/// the row's status to say (docs/gaps.md row 35).
#[tokio::test(flavor = "multi_thread")]
async fn the_bytes_of_a_fetch_are_told_while_it_runs() {
    use axum::routing::get;
    let program = b"#!/bin/sh\necho slow\n".repeat(50);
    let digest = dm_core::hex::encode(sha2::Sha256::digest(&program));
    let served = program.clone();
    let app = axum::Router::new().route(
        "/q/qjs-linux-x86_64",
        get(move || async move {
            // In four pieces, a moment apart, as a large file comes.
            let pieces: Vec<Vec<u8>> = served
                .chunks(served.len() / 4 + 1)
                .map(<[u8]>::to_vec)
                .collect();
            let stream = futures_util::stream::iter(pieces).then(|piece| async move {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                Ok::<_, std::io::Error>(bytes::Bytes::from(piece))
            });
            axum::response::Response::builder()
                .header("Content-Length", served.len())
                .body(axum::body::Body::from_stream(stream))
                .unwrap()
        }),
    );
    let base = format!("http://{}", common::serve(app).await);
    let sources = Sources {
        quickjs: format!("{base}/q"),
        quickjs_digests: vec![("qjs-linux-x86_64".into(), digest)],
    };
    let dir = common::tempdir();
    let folder = dir.path().to_path_buf();
    let total = program.len() as u64;
    let fetching = tokio::spawn(async move {
        fetch_into(
            &dm_core::client(),
            Tool::QuickJs,
            &folder,
            &sources,
            "linux",
            "x86_64",
        )
        .await
    });
    let mut seen = None;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Some((got, said_total)) = dm_core::tools::preparing_progress() {
            if got > 0 {
                seen = Some((got, said_total));
                break;
            }
        }
    }
    fetching.await.unwrap().expect("the fetch");
    let (got, said_total) = seen.expect("the bytes so far were never told");
    assert!(got > 0 && got <= total, "{got} of {total}");
    assert_eq!(said_total, Some(total));
    assert_eq!(
        dm_core::tools::preparing_progress(),
        None,
        "told as fetching after the fetch"
    );
}

/// A server standing in for the release whose build answers `first` the
/// first time it is asked, then the build; and how often it was asked.
async fn flaky_release(first: u16) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use axum::routing::get;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let asked = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = asked.clone();
    let app = axum::Router::new().route(
        "/q/qjs-linux-x86_64",
        get(move || {
            let n = counted.fetch_add(1, Ordering::SeqCst);
            async move {
                let status = if n == 0 { first } else { 200 };
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    &b"a quickjs"[..],
                )
            }
        }),
    );
    (format!("http://{}/q", common::serve(app).await), asked)
}

fn linux_build_from(base: String) -> Sources {
    Sources {
        quickjs: base,
        quickjs_digests: vec![(
            "qjs-linux-x86_64".into(),
            dm_core::hex::encode(sha2::Sha256::digest(b"a quickjs")),
        )],
    }
}

/// A server's passing trouble is tried again; a refusal is not.
#[tokio::test]
async fn a_passing_server_error_is_tried_again_and_a_missing_build_is_not() {
    use std::sync::atomic::Ordering;
    let client = dm_core::client();
    let (base, asked) = flaky_release(503).await;
    let dir = common::tempdir();
    let path = fetch_into(
        &client,
        Tool::QuickJs,
        dir.path(),
        &linux_build_from(base),
        "linux",
        "x86_64",
    )
    .await
    .expect("the build, the second time");
    assert_eq!(std::fs::read(path).unwrap(), b"a quickjs");
    assert_eq!(asked.load(Ordering::SeqCst), 2);

    let (base, asked) = flaky_release(404).await;
    let dir = common::tempdir();
    let said = fetch_into(
        &client,
        Tool::QuickJs,
        dir.path(),
        &linux_build_from(base),
        "linux",
        "x86_64",
    )
    .await
    .unwrap_err();
    assert!(
        matches!(said, dm_core::Error::Refused { status: 404, .. }),
        "{said}"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1, "a refusal was asked again");
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}

/// A build dm knows no digest for is not fetched at all.
#[tokio::test]
async fn a_build_with_no_digest_in_dm_is_not_fetched() {
    use std::sync::atomic::Ordering;
    let (base, asked) = flaky_release(200).await;
    let sources = Sources {
        quickjs: base,
        quickjs_digests: Vec::new(),
    };
    let dir = common::tempdir();
    let said = fetch_into(
        &dm_core::client(),
        Tool::QuickJs,
        dir.path(),
        &sources,
        "linux",
        "x86_64",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("no QuickJS digest"), "{said}");
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}
