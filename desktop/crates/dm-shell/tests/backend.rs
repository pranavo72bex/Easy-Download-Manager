// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The backend's verbs on rows that cannot take them, and a running
//! download given a new address, against a real queue on a loopback server.
//! dm's home is a temporary folder (`DM_HOME`) for the whole binary, and a
//! platform that does nothing stands in for the system. Moved from the egui
//! window's tests.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::response::Response;
use dm_core::{AddOptions, Id, Queue, State};
use dm_shell::app::backend::Backend;
use dm_shell::app::platform::Platform;
use dm_shell::app::power::{Report, WhenDone};
use dm_shell::Settings;

/// One test at a time: they share one home and its settings file.
static ONE: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// dm's home for this binary, made and named in `DM_HOME` once.
fn home() -> &'static Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: set once (`OnceLock`), before any test reads it: they all
        // reach the home through this function, under `ONE`.
        unsafe {
            std::env::set_var("DM_HOME", dir.path());
            // A throw-away home leaves nothing in the person's own store.
            std::env::set_var("DM_NO_SECRET_STORE", "1");
        }
        dir
    })
    .path()
}

/// A system that does nothing and is asked nothing.
struct Quiet;

impl Platform for Quiet {
    fn set_login_item(&self, _wanted: bool) -> Result<(), String> {
        Ok(())
    }
    fn notify(&self, _title: &str, _body: &str) {}
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
    fn open(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
    fn chime(&self) {}
    fn power(&self, _action: WhenDone, _report: Report) -> Result<(), String> {
        Ok(())
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn open_with(&self, _path: &str, _app: &Path) -> Result<(), String> {
        Ok(())
    }
    fn choose_file(&self, _save: bool, _suggested: &str) -> Option<std::path::PathBuf> {
        None
    }
}

/// A loopback server with one file that arrives at once and one that never
/// does, and a backend over a queue of its own.
struct World {
    runtime: tokio::runtime::Runtime,
    dir: tempfile::TempDir,
    addr: std::net::SocketAddr,
}

fn world() -> World {
    home();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let addr = runtime.block_on(async {
        let app = axum::Router::new()
            .route(
                "/file.zip",
                axum::routing::get(|| async { Response::new(Body::from(vec![7u8; 64 * 1024])) }),
            )
            .route(
                "/stalled.bin",
                axum::routing::get(|| async {
                    let never = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
                    Response::builder()
                        .header("content-length", "1000000")
                        .body(Body::from_stream(never))
                        .unwrap()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    World {
        runtime,
        dir: tempfile::tempdir().unwrap(),
        addr,
    }
}

impl World {
    fn url(&self, name: &str) -> String {
        format!("http://{}/{name}", self.addr)
    }

    /// A backend over settings that keep its files in the test's folder.
    fn backend(&self) -> Backend {
        let settings = Settings {
            download_dir: self.dir.path().join("out"),
            connections: 1,
            categorize: false,
            start_on_login: false,
            ..Settings::default()
        };
        let queue = Queue::new(
            dm_core::client(),
            settings.options(self.dir.path().join("parts"), dm_core::Limiter::unlimited()),
            settings.concurrency,
        );
        Backend::new(
            self.runtime.handle().clone(),
            queue,
            settings,
            Vec::new(),
            Arc::new(Quiet),
        )
    }
}

/// Waits up to ten seconds for row `id` to be in `state`.
fn until_state(backend: &Backend, id: Id, state: State, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while backend.row(id).is_none_or(|r| r.state != state) {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn the_backends_verbs_on_rows_that_cannot_take_them_say_why() {
    let _one = one_at_a_time();
    let world = world();
    let backend = world.backend();
    // Not a torrent's files, and none ticked.
    let id = backend
        .add_with(
            &world.url("held.bin"),
            AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        backend.set_torrent_files(id, &[false]),
        Err("Tick at least one file.".into())
    );
    assert!(backend.set_torrent_files(id, &[true]).is_err());
    // An address that is no link.
    assert_eq!(
        backend.set_url(id, "not a link"),
        Err("it must be a link dm can download".into())
    );
    // Resuming a row that is gone, or one already running, does nothing.
    backend.resume(id + 1000);
    backend.flush();
    assert_eq!(backend.list().len(), 1);
    // The settings shared with a thread of its own are the ones in force.
    let shared = backend
        .settings_handle()
        .lock()
        .unwrap()
        .download_dir
        .clone();
    assert_eq!(shared, backend.settings().download_dir);
    // "Always save here without asking", from the dialog: kept alone.
    let now = backend.set_ask_caught_downloads(true).unwrap();
    assert!(now.ask_caught_downloads);
    assert!(Settings::load().0.ask_caught_downloads);
    backend.set_ask_caught_downloads(false).unwrap();
    assert!(!Settings::load().0.ask_caught_downloads);

    // A finished row's address cannot be changed.
    let done = backend
        .add_with(&world.url("file.zip"), AddOptions::default())
        .unwrap();
    until_state(&backend, done, State::Completed, "never finished");
    assert!(backend
        .set_url(done, &world.url("other.pdf"))
        .unwrap_err()
        .starts_with("a running or finished download's address cannot be changed"));
}

/// A running download given a new address is stopped, given it, and
/// started again.
#[test]
fn a_running_download_given_a_new_address_goes_on_from_it() {
    let _one = one_at_a_time();
    let world = world();
    let backend = world.backend();
    let id = backend
        .add_with(&world.url("stalled.bin"), AddOptions::default())
        .unwrap();
    until_state(&backend, id, State::Running, "never started");
    // Resumed while it runs: nothing changes.
    backend.resume(id);
    let fresh = format!("{}?fresh=1", world.url("stalled.bin"));
    backend.set_url(id, &fresh).unwrap();
    let row = backend.row(id).unwrap();
    assert_eq!(row.url, fresh);
    until_state(&backend, id, State::Running, "not started again");
    backend.pause(id);
}
