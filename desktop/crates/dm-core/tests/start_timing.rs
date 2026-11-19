// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How long reading back a saved list of 10,000 rows takes at the start (gaps row S16). By
//! hand: `cargo test --release -p dm-core --test start_timing -- --ignored --nocapture`.

mod common;

use dm_core::{AddOptions, DownloadList, Options};

#[tokio::test]
#[ignore]
async fn reading_back_ten_thousand_rows() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("queue.json");
    let make = || {
        DownloadList::new(
            dm_core::client(),
            Options {
                ..common::options(dir.path())
            },
            1,
        )
    };
    let first = make();
    first.persist_to(store.clone()).await;
    let items: Vec<(String, AddOptions)> = (0..10_000)
        .map(|i| {
            (
                format!("http://127.0.0.1:9/files/file-{i}.iso"),
                AddOptions {
                    later: true,
                    ..AddOptions::default()
                },
            )
        })
        .collect();
    first.add_each(items).await;
    first.flush().await;
    drop(first);
    if let Some(out) = dm_core::env::get_os("DM_SEED_OUT") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::copy(&store, std::path::Path::new(&out).join("queue.json")).unwrap();
    }
    println!(
        "queue.json {} bytes",
        std::fs::metadata(&store).unwrap().len()
    );
    let bytes = std::fs::read(&store).unwrap();
    let started = std::time::Instant::now();
    let rows: Vec<dm_core::Entry> = serde_json::from_slice(&bytes).unwrap();
    println!("parse alone: {:?} ({} rows)", started.elapsed(), rows.len());
    let started = std::time::Instant::now();
    let again = serde_json::to_vec(&rows).unwrap();
    println!(
        "write out alone: {:?} ({} bytes)",
        started.elapsed(),
        again.len()
    );
    for _ in 0..3 {
        let second = make();
        let started = std::time::Instant::now();
        second.persist_to(store.clone()).await;
        println!("persist_to: {:?}", started.elapsed());
    }
}
