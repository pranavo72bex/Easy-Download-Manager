// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `configure` puts every part of the engine's program-wide settings in force, each read back
//! where the engine reads it (gaps row A11), and the default puts the engine back as it starts.
//!
//! One test, a program of its own: what it sets is the whole program's, and tests beside it
//! would see it.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::response::Response;
use edm_core::{client, configure, DownloadList, EngineConfig, State};

/// A download of one small file through a queue, as the window's go; its row's state at the end.
async fn a_download(dir: &std::path::Path) -> State {
    async fn handler() -> Response {
        Response::builder()
            .header("content-length", 1024)
            .body(Body::from(vec![7u8; 1024]))
            .unwrap()
    }
    let addr = common::serve(axum::Router::new().fallback(handler)).await;
    let options = edm_core::Options {
        connections: 1,
        ..common::options(dir)
    };
    let q = DownloadList::new(client(), options, 1);
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    tokio::time::timeout(Duration::from_secs(20), q.run())
        .await
        .unwrap();
    q.get(id).await.unwrap().state
}

#[tokio::test]
async fn configure_puts_each_part_in_force_and_the_default_takes_it_back() {
    let dir = common::tempdir();
    let waited = Arc::new(AtomicUsize::new(0));
    let counts = Arc::clone(&waited);
    let config = EngineConfig {
        identify_as_dm: true,
        sound_language: "pt_BR".into(),
        prefer_h264: true,
        connect_first: Some(Arc::new(move |_, _| {
            counts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })),
        second_path: Some("edm-no-such-network0".into()),
        kept_players: Some(dir.path().join("players")),
        only_through: String::new(),
        second_limit: 0,
        third_limit: 0,
        network_allowances: Vec::new(),
        third_path: None,
        more_paths: Vec::new(),
    };

    // A second network that is not there is refused in words, and left off; the rest is in force.
    let said = configure(&config).unwrap_err();
    assert!(said.contains("is not connected"), "{said}");
    assert_eq!(edm_core::first_agent(), edm_core::USER_AGENT);
    assert_eq!(edm_core::sound_language::wanted().as_deref(), Some("pt-BR"));
    assert!(edm_core::prefer_h264::wanted());
    assert_eq!(
        edm_core::youtube::kept_players(),
        Some(dir.path().join("players"))
    );
    assert_eq!(a_download(dir.path()).await, State::Completed);
    assert!(
        waited.load(Ordering::SeqCst) >= 1,
        "the download did not wait on the connection configured"
    );

    // With none chosen, the second network is no reason to say anything.
    let none = EngineConfig {
        second_path: None,
        ..config
    };
    assert_eq!(configure(&none), Ok(()));

    // The default: the engine as it starts.
    assert_eq!(configure(&EngineConfig::default()), Ok(()));
    assert_eq!(edm_core::first_agent(), edm_core::IE_AGENT);
    assert_eq!(edm_core::sound_language::wanted(), None);
    assert!(!edm_core::prefer_h264::wanted());
    assert_eq!(edm_core::youtube::kept_players(), None);
    let before = waited.load(Ordering::SeqCst);
    assert_eq!(
        a_download(&dir.path().join("again")).await,
        State::Completed
    );
    assert_eq!(
        waited.load(Ordering::SeqCst),
        before,
        "a download waited on a connection after it was taken away"
    );
}
