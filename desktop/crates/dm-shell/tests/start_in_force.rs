// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the saved settings put in force across the engine is in force from the
//! start, not first after Options' OK: "Identify as dm only" (contract row B51:
//! "the change applies at once and is kept") and a dial-up or VPN connection
//! (gaps row 93, G84). Both were left as the engine began until a setting
//! changed (found 2026-11-08, beside reference A).

use std::time::Duration;

use dm_shell::settings::Settings;
use dm_shell::shell;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_settings_are_in_force_from_the_start() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the one test in this binary, so the process-wide home is its own
    // and nothing else is reading the environment while it is set.
    unsafe { std::env::set_var("DM_HOME", home.path()) };
    let mut saved = Settings {
        download_dir: home.path().join("out"),
        identify_as_dm: true,
        ..Settings::default()
    };
    saved.dialup.enabled = true;
    saved.dialup.connection = "dm test connection that does not exist".into();
    saved.dialup.attempts = 1;
    saved.dialup.wait_seconds = 0;
    saved.save().unwrap();

    let started = shell::start(&shell::orders(&[])).await;
    assert_eq!(
        dm_core::first_agent(),
        dm_core::USER_AGENT,
        "a saved Identify as dm only was not in force at the start"
    );

    // The connection is waited for: on Windows one it does not know fails the download in
    // Windows' words, with nothing sent; elsewhere there is nothing to dial.
    let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = std::sync::Arc::clone(&served);
    let app = axum::Router::new().fallback(move || {
        let count = std::sync::Arc::clone(&count);
        async move {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![7u8; 512]
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    let queue = started.queue;
    let id = queue.add(&format!("http://{addr}/f.bin")).await.unwrap();
    tokio::time::timeout(Duration::from_secs(60), queue.run())
        .await
        .unwrap();
    let state = queue.get(id).await.unwrap().state;
    if cfg!(windows) {
        match state {
            dm_core::State::Failed(why) => assert!(why.contains("did not come up"), "{why}"),
            other => panic!("a saved dial-up connection was not waited for: {other:?}"),
        }
        assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), 0);
    } else {
        assert_eq!(state, dm_core::State::Completed);
    }
}
