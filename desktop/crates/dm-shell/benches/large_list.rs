// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A read of a list of 10,000 rows, what a window does each time the queue changes: the
//! application's read (`Session::read`), each finished file's size on disk
//! (`SavedSizes::apply`), the places and their counts (`Places`) and the rows waiting under the
//! same name (`names::earlier_of_the_same_name`). The queue is a real one, as
//! `session/under_load_tests.rs` builds it: 10,000 rows added for later and four running
//! against a loopback server that never answers. dm's home is a temporary folder
//! (`DM_HOME`). Run by hand, never in CI (docs/testing.md, Benchmarks; gaps row 190):
//! `cargo bench -p dm-shell --bench large_list`.

use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use dm_core::{AddOptions, Category, Entry, State};
use dm_shell::app::backend::Backend;
use dm_shell::app::names::earlier_of_the_same_name;
use dm_shell::app::places::{Place, Places};
use dm_shell::app::platform::Platform;
use dm_shell::app::power::{Report, WhenDone};
use dm_shell::app::session::Session;
use dm_shell::app::snapshot::SavedSizes;
use dm_shell::Settings;

const ROWS: usize = 10_000;
const EXTENSIONS: [&str; 5] = ["zip", "mp4", "pdf", "bin", "mp3"];

/// A system that does nothing and is asked nothing (as `tests/backend.rs` has it: dm-shell's
/// own is its unit tests' alone).
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

/// The queue the benches read, and what it stands on.
struct World {
    // Dropped before the folders it writes into.
    backend: Backend,
    settings: Settings,
    _runtime: tokio::runtime::Runtime,
    _home: tempfile::TempDir,
    dir: tempfile::TempDir,
}

fn world() -> World {
    let home = tempfile::tempdir().unwrap();
    // Set before anything reads it, on the one thread there is yet. The edition is 2021, where
    // this is a safe call.
    std::env::set_var("DM_HOME", home.path());
    // A throw-away home leaves nothing in the person's own store.
    std::env::set_var("DM_NO_SECRET_STORE", "1");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        general: dm_shell::settings::GeneralSettings {
            start_on_login: false,
            ..Default::default()
        },
        folders: dm_shell::settings::FolderSettings {
            download_dir: dir.path().join("out"),
            ..Default::default()
        },
        ..Settings::default()
    };
    let queue = dm_core::DownloadList::new(
        dm_core::client(),
        settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
        settings.downloads.concurrency,
    );
    let backend = Backend::new(
        runtime.handle().clone(),
        queue,
        settings,
        Vec::new(),
        Arc::new(Quiet),
    );
    // A server that takes each request and never answers it: its downloads stay running.
    let addr = runtime.block_on(async {
        let app = axum::Router::new().fallback(|| async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            "never"
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    let later = AddOptions {
        later: true,
        ..Default::default()
    };
    // Five kinds of file, and each name four times over (different addresses, one folder).
    let items = (0..ROWS)
        .map(|i| {
            let ext = EXTENSIONS[i % EXTENSIONS.len()];
            (
                format!("http://127.0.0.1:9/{i}/file{}.{ext}", i % (ROWS / 4)),
                later.clone(),
            )
        })
        .collect();
    assert!(backend.add_each(items).iter().all(Result::is_ok));
    let active: Vec<_> = (0..4)
        .map(|i| {
            backend
                .add_with(
                    &format!("http://{addr}/active{i}.bin"),
                    AddOptions::default(),
                )
                .unwrap()
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !backend
        .list()
        .iter()
        .any(|r| active.contains(&r.id) && r.state == State::Running)
    {
        assert!(Instant::now() < deadline, "no download ever ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    let settings = backend.settings();
    World {
        backend,
        settings,
        _runtime: runtime,
        _home: home,
        dir,
    }
}

/// The rows as finished: each saved to one of a hundred files of 1 KiB, its counted size the
/// file's, except every tenth (a stream put into an MP4 is not the size the engine counted).
fn finished(world: &World, rows: &[Arc<Entry>]) -> Vec<Arc<Entry>> {
    let saved = world.dir.path().join("saved");
    std::fs::create_dir_all(&saved).unwrap();
    let files: Vec<String> = (0..100)
        .map(|i| {
            let path = saved.join(format!("saved{i}.bin"));
            std::fs::write(&path, [0u8; 1024]).unwrap();
            path.display().to_string()
        })
        .collect();
    rows.iter()
        .enumerate()
        .map(|(i, row)| {
            let mut row = Arc::clone(row);
            let copy = Arc::make_mut(&mut row);
            copy.state = State::Completed;
            copy.path.clone_from(&files[i % files.len()]);
            copy.progress.downloaded = if i % 10 == 0 { 900 } else { 1024 };
            copy.progress.total = Some(copy.progress.downloaded);
            row
        })
        .collect()
}

fn large_list(c: &mut Criterion) {
    let world = world();
    let (backend, settings) = (&world.backend, &world.settings);

    let mut group = c.benchmark_group("session_read_10k");
    // The window's read: nothing subscribed to the events (gaps row 181).
    let mut session = Session::default();
    session.read(backend, settings);
    group.bench_function("window", |b| {
        b.iter(|| black_box(session.read(backend, settings)));
    });
    // With two subscribers to the events, as `under_load_tests.rs` reads.
    let mut session = Session::default();
    let (one, two) = (
        session.events.subscribe(None),
        session.events.subscribe(None),
    );
    session.read(backend, settings);
    group.bench_function("two-subscribers", |b| {
        b.iter(|| {
            black_box(session.read(backend, settings));
            black_box((one.drain(), two.drain()));
        });
    });
    group.finish();

    let rows = backend.snapshot().rows;
    let done = finished(&world, &rows);

    let mut group = c.benchmark_group("saved_sizes_10k");
    // The first read: every finished file's size asked of the disk.
    group.bench_function("first-read", |b| {
        b.iter_batched(
            || (SavedSizes::default(), done.clone()),
            |(mut sizes, mut rows)| {
                sizes.apply(&mut rows);
                (sizes, rows)
            },
            BatchSize::LargeInput,
        );
    });
    // A read that changed nothing: the queue's rows again, each given its copy kept.
    let mut sizes = SavedSizes::default();
    sizes.apply(&mut done.clone());
    group.bench_function("unchanged", |b| {
        b.iter_batched(
            || done.clone(),
            |mut rows| {
                sizes.apply(&mut rows);
                rows
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();

    let categories = settings.categories();
    let mut every_place = vec![Place::All, Place::Unfinished, Place::Finished];
    every_place.extend(Category::ALL.into_iter().map(Place::Kind));
    let mut group = c.benchmark_group("places_10k");
    group.bench_function("place-first-read", |b| {
        b.iter_batched(
            Places::default,
            |mut places| {
                places.place_rows(&rows, &categories);
                places
            },
            BatchSize::LargeInput,
        );
    });
    let mut places = Places::default();
    places.place_rows(&rows, &categories);
    group.bench_function("place-unchanged", |b| {
        b.iter(|| places.place_rows(black_box(&rows), &categories));
    });
    // A read's counts: each place counted once, as the sidebar asks after each read.
    group.bench_function("place-and-count-every-place", |b| {
        b.iter(|| {
            places.place_rows(&rows, &categories);
            for place in &every_place {
                black_box(places.count(place, &rows, &categories));
            }
        });
    });
    // The filter typed over the list, two words, over every row.
    group.bench_function("filter", |b| {
        b.iter(|| {
            rows.iter()
                .filter(|r| places.matches_filter(black_box("file12 zip"), r))
                .count()
        });
    });
    group.finish();

    let mut group = c.benchmark_group("names_10k");
    group.bench_function("earlier_of_the_same_name", |b| {
        b.iter(|| earlier_of_the_same_name(black_box(&rows)));
    });
    group.finish();
}

criterion_group!(benches, large_list);
criterion_main!(benches);
