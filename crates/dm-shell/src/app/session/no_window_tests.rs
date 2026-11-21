// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::app::handoff::{Asking, Route};

/// Two presentations each subscribed to the session's events hear the
/// same download added, paused and removed, through the reads alone.
#[test]
fn subscribers_hear_a_downloads_life_through_the_sessions_reads() {
    use crate::app::events::DownloadEvent;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let settings = backend.settings();
    let mut session = Session::default();
    let window = session.events.subscribe(None);
    let script = session.events.subscribe(None);
    session.read(&backend, &settings);
    let id = backend
        .add_with(
            "http://127.0.0.1:9/a.bin",
            dm_core::AddOptions {
                later: true,
                ..dm_core::AddOptions::default()
            },
        )
        .unwrap();
    session.read(&backend, &settings);
    backend.remove(id);
    session.read(&backend, &settings);
    for subscriber in [&window, &script] {
        let got = subscriber.drain();
        assert!(
            matches!(got.first(), Some(DownloadEvent::Added { id: added, .. }) if *added == id),
            "{got:?}"
        );
        assert_eq!(got.last(), Some(&DownloadEvent::Removed(id)), "{got:?}");
    }
}

/// Rows the presentation added are its own (not "started elsewhere"),
/// and a refused address is counted without losing the rest.
#[test]
fn adding_marks_the_rows_as_this_presentations_and_counts_what_was_refused() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let settings = backend.settings();
    let mut session = Session::default();
    session.read(&backend, &settings);
    let now = dm_core::AddOptions::default();
    let added = session.add(
        &backend,
        vec![
            ("http://127.0.0.1:9/a.bin".into(), now.clone()),
            ("not an address".into(), now.clone()),
        ],
    );
    assert_eq!(added.rows.len(), 1);
    assert_eq!(added.rows[0].1, "http://127.0.0.1:9/a.bin");
    assert_eq!(added.outcome(), Ok(1));
    assert!(matches!(added.partly(), Some((1, _))), "{added:?}");
    let read = session.read(&backend, &settings);
    assert!(read.detected.started_elsewhere.is_empty(), "its own");
    let none = session.add(&backend, vec![("not an address".into(), now)]);
    assert!(none.outcome().is_err() && none.partly().is_none());
}

/// Rows added off the window's thread (Import's errand) are its own
/// once the adding says so, though reads were made while they were
/// added; a browser's hand-off added meanwhile is still told as started
/// elsewhere, at the first read after (gaps row 183).
#[test]
fn rows_added_off_the_thread_are_its_own_and_a_hand_off_beside_them_is_not() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    // A server that never answers: the downloads stay running or waiting.
    let addr = runtime.block_on(async {
        let app = axum::Router::new().fallback(|| async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            ""
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    let settings = backend.settings();
    let mut session = Session::default();
    session.read(&backend, &settings);
    let adding = session.adding();
    let list: String = (0..3)
        .map(|i| format!("http://{addr}/list{i}.bin\n"))
        .collect();
    let errand = {
        let backend = backend.clone();
        std::thread::spawn(move || crate::app::import::import(&backend, &[], &list, false))
    };
    let imported = errand.join().unwrap();
    assert_eq!(imported.added, 3);
    // A browser's hand-off, added while the list is: not the window's.
    let handed = backend
        .add_with(
            &format!("http://{addr}/handed.bin"),
            dm_core::AddOptions::default(),
        )
        .unwrap();
    let during = session.read(&backend, &settings);
    assert!(
        during.detected.started_elsewhere.is_empty(),
        "held while added"
    );
    adding.added(&imported.new);
    let after = session.read(&backend, &settings);
    assert_eq!(after.detected.started_elsewhere, vec![handed]);
    assert!(session
        .read(&backend, &settings)
        .detected
        .started_elsewhere
        .is_empty());
    // An adding that ends without saying (its errand panicked) leaves
    // its rows to be judged as any new row.
    let lost = session.adding();
    let stray = backend
        .add_with(
            &format!("http://{addr}/stray.bin"),
            dm_core::AddOptions::default(),
        )
        .unwrap();
    assert!(session
        .read(&backend, &settings)
        .detected
        .started_elsewhere
        .is_empty());
    drop(lost);
    assert_eq!(
        session.read(&backend, &settings).detected.started_elsewhere,
        vec![stray]
    );
}

#[test]
fn a_presentation_with_no_window_drives_a_download_to_when_done() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let addr = runtime.block_on(async {
        let app = axum::Router::new().fallback(|| async { "eleven byte" });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    let settings = backend.settings();
    let mut session = Session {
        when_done: WhenDone::Sleep,
        ..Session::default()
    };
    // Each read is followed by one look for the queue running dry, the
    // first included.
    let first = session.read(&backend, &settings);
    assert!(!session.ran_dry(&first.snapshot.rows));

    dm_core::lock(&backend.handed).push_back(crate::host::Request {
        url: format!("http://{addr}/file.bin"),
        ..Default::default()
    });
    let Route::Added(Ok(id)) =
        session
            .hands
            .next(&backend, &settings, &[], Asking::Nobody, Instant::now())
    else {
        panic!("not added");
    };

    let mut finished = false;
    let mut fired = None;
    let until = Instant::now() + Duration::from_secs(20);
    while fired.is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
        let read = session.read(&backend, &settings);
        finished |= read.detected.finished.contains(&id);
        if session.ran_dry(&read.snapshot.rows) {
            fired = Some(session.when_done_fires(true, Instant::now()));
        }
    }
    assert!(finished, "the download was not seen to finish");
    assert_eq!(
        fired,
        Some(Fired::Counting),
        "a sleep waits out its countdown"
    );
    assert_eq!(session.when_done, WhenDone::Nothing, "taken once");
    let due = Instant::now() + Duration::from_secs(super::super::power::COUNTDOWN_SECS);
    assert_eq!(
        session.countdown_due(&backend, due),
        Some(Ok(Carried::Asked))
    );

    let rows = backend.snapshot().rows;
    let listed =
        crate::app::control_api::answer(&backend, &rows, "queue.list", &serde_json::Value::Null)
            .unwrap()
            .unwrap();
    assert_eq!(listed[0]["id"], serde_json::json!(id));
    assert_eq!(listed[0]["state"], serde_json::json!({"kind": "completed"}));
}
