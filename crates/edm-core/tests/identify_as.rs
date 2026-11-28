// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The first name a request goes out with, and the switch that makes it EDM's own
//! (Options › Advanced › Identify as EDM only).
//!
//! One test, because the switch is process-wide: tests of it that ran side by
//! side would see each other's setting.

mod common;
use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use common::{options, tempdir};
use std::sync::{Arc, Mutex};

/// Every agent a download of one small file sent, in order, from a server that
/// turns away any agent for which `refuse` is true.
async fn agents_of_a_download(refuse: fn(&str) -> bool) -> Vec<String> {
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
            if refuse(&agent) {
                return Response::builder()
                    .status(403)
                    .body(Body::from("no"))
                    .unwrap();
            }
            Response::builder()
                .header("content-length", "4")
                .body(Body::from("data"))
                .unwrap()
        }
    }));
    let addr = common::serve(app).await;
    let dir = tempdir();
    edm_core::download(
        &edm_core::client(),
        &format!("http://{addr}/a.bin"),
        &options(dir.path()),
    )
    .await
    .expect("the download failed");
    let all = seen.lock().unwrap().clone();
    all
}

#[tokio::test]
async fn the_first_agent_is_a_browsers_unless_the_switch_asks_for_dms_own() {
    // As it is by default.
    edm_core::set_identify_as_dm(false);
    assert_eq!(edm_core::first_agent(), edm_core::IE_AGENT);
    let agents = agents_of_a_download(|_| false).await;
    assert_eq!(agents.first().map(String::as_str), Some(edm_core::IE_AGENT));

    // With the switch on, EDM's own name goes first.
    edm_core::set_identify_as_dm(true);
    assert_eq!(edm_core::first_agent(), edm_core::USER_AGENT);
    let agents = agents_of_a_download(|_| false).await;
    assert_eq!(
        agents.first().map(String::as_str),
        Some(edm_core::USER_AGENT)
    );
    assert!(agents[0].starts_with("EDM/"), "{agents:?}");

    // A site that turns EDM's full name away (this one refuses a code host in
    // the agent) is still asked again with the others, not asked the same
    // name twice.
    let agents = agents_of_a_download(|a| a.to_ascii_lowercase().contains("github")).await;
    assert_eq!(agents[0], edm_core::USER_AGENT);
    assert_eq!(
        agents
            .iter()
            .filter(|a| **a == edm_core::USER_AGENT)
            .count(),
        1,
        "{agents:?}"
    );
    assert!(
        agents.iter().any(|a| *a == edm_core::PLAIN_AGENT),
        "{agents:?}"
    );

    edm_core::set_identify_as_dm(false);
}
