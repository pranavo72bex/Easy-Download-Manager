// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::app::events::DownloadEvent;

/// Each operation's time, per run.
pub(super) struct Costs {
    pub shift: Duration,
    pub pause_resume: Duration,
    pub read: Duration,
}

fn each(runs: u32, mut f: impl FnMut()) -> Duration {
    let start = Instant::now();
    for _ in 0..runs {
        f();
    }
    start.elapsed() / runs
}

pub(super) fn exercise(size: usize, runs: u32) -> Costs {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    // A server that takes each request and never answers it: its
    // downloads stay running for as long as the test.
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
    let later = dm_core::AddOptions {
        later: true,
        ..Default::default()
    };
    let items = (0..size)
        .map(|i| (format!("http://127.0.0.1:9/{i}.bin"), later.clone()))
        .collect();
    assert!(backend.add_each(items).iter().all(Result::is_ok));
    let active: Vec<Id> = (0..4)
        .map(|i| {
            backend
                .add_with(
                    &format!("http://{addr}/active{i}.bin"),
                    dm_core::AddOptions::default(),
                )
                .unwrap()
        })
        .collect();
    let settings = backend.settings();
    let mut session = Session::default();
    let first = session.events.subscribe(None);
    let second = session.events.subscribe(None);
    session.read(&backend, &settings);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !backend
        .list()
        .iter()
        .any(|r| active.contains(&r.id) && r.state == State::Running)
    {
        assert!(Instant::now() < deadline, "no download ever ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    let ids: Vec<Id> = backend.list().iter().map(|r| r.id).collect();
    let middle = ids[ids.len() / 2];
    let mut by = -1;
    let shift = each(runs, || {
        backend.shift(middle, by);
        by = -by;
    });
    let running = active[0];
    let pause_resume = each(runs, || {
        backend.pause(running);
        backend.resume(running);
    });
    let read = each(runs, || {
        session.read(&backend, &settings);
    });
    // Nothing was lost or told differently to either subscriber, and the
    // list is whole.
    let (one, two) = (first.drain(), second.drain());
    assert_eq!(one, two, "the subscribers heard different things");
    assert!(
        one.iter().all(|e| !matches!(e, DownloadEvent::Removed(_))),
        "nothing was removed"
    );
    assert_eq!(backend.list().len(), size + active.len());
    for id in active {
        backend.pause(id);
    }
    Costs {
        shift,
        pause_resume,
        read,
    }
}

#[test]
fn a_hundred_rows_with_downloads_running_take_moves_and_pauses() {
    let costs = exercise(100, 20);
    // Generous: a loaded CI runner, in a debug build.
    assert!(
        costs.shift < Duration::from_millis(200),
        "{:?}",
        costs.shift
    );
    assert!(
        costs.pause_resume < Duration::from_millis(500),
        "{:?}",
        costs.pause_resume
    );
    assert!(costs.read < Duration::from_millis(200), "{:?}", costs.read);
}

#[test]
#[ignore = "a measure, run by hand in release"]
fn large_queue_under_load() {
    for size in [100usize, 1_000, 10_000] {
        let costs = exercise(size, 50);
        println!(
            "{size:>6} rows, 4 running: move {:>9.2?}  pause+resume {:>9.2?}  read (2 subscribers) {:>9.2?}",
            costs.shift, costs.pause_resume, costs.read
        );
        assert!(costs.read < Duration::from_millis(50), "{:?}", costs.read);
    }
}
