// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's engine on the host: started in a home of its own, handed a link as a share
//! would hand it, and the file downloaded, paused and resumed through the same calls the
//! phone's screens make (gaps row N3).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;

/// The phone's side, recording what the engine asked of it.
#[derive(Default)]
struct Recorded {
    changes: AtomicUsize,
    notified: Mutex<Vec<String>>,
}

impl AndroidSystem for Recorded {
    fn notify(&self, title: String, _: String, _: Option<String>, _: Option<u64>) {
        self.notified.lock().unwrap().push(title);
    }
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn set_clipboard(&self, _: String) {}
    fn open(&self, _: String) -> Option<String> {
        None
    }
    fn reveal(&self, _: String) -> Option<String> {
        None
    }
    fn sound(&self, _: bool) {}
    fn changed(&self) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }
}

/// A server of `body` at `/file.bin`, on a runtime of its own; its address.
fn serve(body: Vec<u8>) -> (tokio::runtime::Runtime, String) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let address = listener.local_addr().unwrap();
    let body = Arc::new(body);
    runtime.spawn(async move {
        let app = axum::Router::new().route(
            "/file.bin",
            axum::routing::get(move || {
                let body = Arc::clone(&body);
                async move { (*body).clone() }
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });
    (runtime, format!("http://{address}/file.bin"))
}

/// Waits up to `limit` for `row` to say `true`.
fn until(engine: &Engine, id: u64, limit: Duration, done: impl Fn(&Row) -> bool) -> Row {
    let start = Instant::now();
    loop {
        let row = engine.row(id).expect("the row is in the list");
        if done(&row) || start.elapsed() > limit {
            return row;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// One test, as the engine's home is the process's (`DM_HOME`) and its change listeners last
// for the process.
#[test]
fn a_shared_link_is_downloaded_into_the_phones_folder_and_its_settings_round_trip() {
    let home = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let body: Vec<u8> = (0..256 * 1024u32).map(|i| (i % 251) as u8).collect();
    let (_server, url) = serve(body.clone());
    let system = Arc::new(Recorded::default());

    // A relative home is refused before anything starts.
    let refused = Engine::start(
        "relative".into(),
        downloads.path().display().to_string(),
        system.clone(),
    );
    assert!(matches!(refused, Err(EngineError::Start(_))));

    let engine = Engine::start(
        home.path().display().to_string(),
        downloads.path().display().to_string(),
        system.clone(),
    )
    .unwrap();

    // The first start puts new downloads in the phone's folder, not the desktop's.
    let settings: serde_json::Value =
        serde_json::from_str(&engine.settings_json().unwrap()).unwrap();
    assert_eq!(
        settings["download_dir"].as_str().unwrap(),
        downloads.path().display().to_string()
    );

    // Shared text with a link in it, as Android's share sheet hands it over.
    let added = engine
        .add(format!("Look at this: {url}"))
        .expect("the link is taken");
    assert_eq!(added.ids.len(), 1);
    assert!(added.skipped.is_empty(), "skipped: {:?}", added.skipped);
    let id = added.ids[0];
    assert!(engine.busy() || engine.row(id).unwrap().state == RowState::Completed);

    let row = until(&engine, id, Duration::from_secs(30), |row| {
        row.state == RowState::Completed || row.state == RowState::Failed
    });
    assert_eq!(row.state, RowState::Completed, "failure: {:?}", row.failure);
    assert_eq!(row.name, "file.bin");
    assert!(!row.torrent);
    assert_eq!(std::fs::read(&row.path).unwrap(), body);
    assert!(
        std::path::Path::new(&row.path).starts_with(downloads.path()),
        "{} is not under the phone's folder",
        row.path
    );
    assert!(!engine.busy());
    assert!(
        system.changes.load(Ordering::Relaxed) > 0,
        "the phone was never told"
    );

    // Settings go back in whole: a change is kept, and a broken one is refused in words.
    let mut changed = settings.clone();
    changed["watch_clipboard"] = serde_json::Value::Bool(true);
    let now: serde_json::Value =
        serde_json::from_str(&engine.set_settings_json(changed.to_string()).unwrap()).unwrap();
    assert_eq!(now["watch_clipboard"], true);
    assert!(matches!(
        engine.set_settings_json("{not json".into()),
        Err(EngineError::Refused(_))
    ));

    assert_eq!(engine.clear_finished(), 1);
    assert!(engine.list().is_empty());
    assert!(matches!(
        engine.add("no link here".into()),
        Err(EngineError::Refused(_))
    ));
}
