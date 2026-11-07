// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A site that answers a browser's name with "429 Too many requests" and serves
//! dm's own. Wikimedia's media servers did on 2026-11-07: Internet Explorer's
//! and Firefox's names were told to wait ten minutes (`Retry-After: 600`, "please
//! contact noc@wikimedia.org to discuss a less disruptive approach") from the
//! first request, and dm's name got the file at once. dm asks once more by its
//! own name; a site that limits every name is not asked by each name in turn.
//!
//! One test, because the switch (Options › Advanced › Identify as dm only) is
//! process-wide: tests of it that ran side by side would see each other's.

mod common;
use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use common::{options, tempdir};
use std::sync::{Arc, Mutex};

const BODY: &[u8] = b"the file's own bytes";

/// What a download from a server that answers 429 to every agent `limit`
/// picks ended with, and the agent of every request it sent, in order.
async fn download_from(
    limit: fn(&str) -> bool,
    attempts: u32,
) -> (Result<std::path::PathBuf, String>, Vec<String>) {
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let log = seen.clone();
    let app = axum::Router::new().fallback(get(move |headers: HeaderMap| {
        let log = log.clone();
        async move {
            let agent = headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("(none)")
                .to_string();
            log.lock().unwrap().push(agent.clone());
            if limit(&agent) {
                return Response::builder()
                    .status(429)
                    .header("retry-after", "600")
                    .body(Body::from("Too many requests"))
                    .unwrap();
            }
            Response::builder()
                .header("content-length", BODY.len().to_string())
                .body(Body::from(BODY))
                .unwrap()
        }
    }));
    let addr = common::serve(app).await;
    let dir = tempdir();
    let mut asked = options(dir.path());
    asked.attempts = attempts;
    let outcome = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/video.webm"),
        &asked,
    )
    .await
    .map(|done| {
        assert_eq!(
            std::fs::read(&done.path).unwrap(),
            BODY,
            "the file is not whole"
        );
        done.path
    })
    .map_err(|e| e.to_string());
    let all = seen.lock().unwrap().clone();
    (outcome, all)
}

/// A server that serves the first request in any name, slowly, and answers 429
/// to the ranges after it in a browser's name (Wikimedia, the same day, now and
/// then): the file, and the agent and first byte of every request, in order.
async fn download_limiting_ranges() -> (Vec<u8>, Vec<u8>, Vec<(String, usize)>) {
    use futures_util::StreamExt;
    let data: Vec<u8> = (0..4 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    let seen: Arc<Mutex<Vec<(String, usize)>>> = Default::default();
    let (served, log) = (Arc::new(data.clone()), seen.clone());
    let app = axum::Router::new().fallback(get(move |headers: HeaderMap| {
        let (data, log) = (served.clone(), log.clone());
        async move {
            let agent = headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("(none)")
                .to_string();
            let total = data.len();
            let (from, to) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .map(|(a, b)| {
                    let a: usize = a.parse().unwrap();
                    (a, b.parse::<usize>().unwrap_or(total - 1).min(total - 1))
                })
                .unwrap_or((0, total - 1));
            log.lock().unwrap().push((agent.clone(), from));
            if from > 0 && !agent.starts_with("dm/") {
                return Response::builder()
                    .status(429)
                    .header("retry-after", "600")
                    .body(Body::from("Too many requests"))
                    .unwrap();
            }
            let chunks: Vec<_> = data[from..=to]
                .chunks(16 * 1024)
                .map(bytes::Bytes::copy_from_slice)
                .collect();
            let slow = futures_util::stream::iter(chunks).then(|c| async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                Ok::<_, std::io::Error>(c)
            });
            Response::builder()
                .status(206)
                .header("content-range", format!("bytes {from}-{to}/{total}"))
                .header("content-length", (to - from + 1).to_string())
                .body(Body::from_stream(slow))
                .unwrap()
        }
    }));
    let addr = common::serve(app).await;
    let dir = tempdir();
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/large.webm"),
        &options(dir.path()),
    )
    .await
    .expect("the download failed");
    let got = std::fs::read(&done.path).unwrap();
    let all = seen.lock().unwrap().clone();
    (data, got, all)
}

#[tokio::test]
async fn a_browsers_name_told_too_many_requests_is_asked_again_by_dms_own_once() {
    dm_core::set_identify_as_dm(false);

    // The first request served in the browser's name, the ranges after it
    // limited in it: the ranges go on in dm's name, and once a range has, no
    // request goes out in the browser's name again.
    let (served, got, requests) = download_limiting_ranges().await;
    assert!(got == served, "the file is not the one served");
    let ranges: Vec<_> = requests.iter().filter(|(_, from)| *from > 0).collect();
    assert!(
        ranges.iter().any(|(a, _)| a == dm_core::IE_AGENT),
        "the download was not split, so the limit was never met: {requests:?}"
    );
    let first_by_dm = requests
        .iter()
        .position(|(a, from)| *from > 0 && a.starts_with("dm/"))
        .unwrap_or_else(|| panic!("no range was asked in dm's name: {requests:?}"));
    assert!(
        requests[first_by_dm..]
            .iter()
            .all(|(a, _)| a.starts_with("dm/")),
        "the browser's name was used again after the limit: {requests:?}"
    );

    // A site that limits browsers' names only: the file, by dm's name, with one
    // request in the browser's name and none after it.
    let (outcome, agents) = download_from(|a| !a.starts_with("dm/"), 5).await;
    assert!(outcome.is_ok(), "{outcome:?} {agents:?}");
    assert_eq!(
        agents.first().map(String::as_str),
        Some(dm_core::IE_AGENT),
        "{agents:?}"
    );
    assert_eq!(
        agents.get(1).map(String::as_str),
        Some(dm_core::USER_AGENT),
        "{agents:?}"
    );
    assert_eq!(
        agents.iter().filter(|a| *a == dm_core::IE_AGENT).count(),
        1,
        "the browser's name was asked again after it was limited: {agents:?}"
    );

    // A site that limits everyone: dm's name is tried once, no third name, and
    // the download fails as a limited one ("too many requests"), not as a refusal.
    let (outcome, agents) = download_from(|_| true, 1).await;
    let said = outcome.expect_err("a server that limits every name gave a file");
    assert_eq!(
        agents,
        [dm_core::IE_AGENT, dm_core::USER_AGENT],
        "every name but one more was asked of a server that limits them all"
    );
    assert!(
        said.contains("429") || said.to_lowercase().contains("too many"),
        "{said}"
    );

    // With Identify as dm only on, a limit on dm's own name is a limit, not a
    // reason to try a browser's.
    dm_core::set_identify_as_dm(true);
    let (outcome, agents) = download_from(|a| a.starts_with("dm/"), 1).await;
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(agents, [dm_core::USER_AGENT], "{agents:?}");
    dm_core::set_identify_as_dm(false);
}
