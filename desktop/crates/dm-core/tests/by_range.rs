// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Each range in a plain file of its own, appended into the finished file in file order: the
//! layout dm stages a download in where its download folder is on another disk than its parts
//! folder (gaps row 166). A test cannot count on two disks, so each test here runs again in a
//! process of its own with `DM_STAGE_BY_RANGE=1`, which takes that layout on one.
//!
//! Each serves its own file from a loopback server and compares what was saved with what was
//! served, byte for byte.

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, download};
use futures_util::StreamExt;

use common::{options, serve, tempdir};

/// Runs the test named again in a process of its own with a file a range, and fails as it fails
/// there: `true` in that process, where the test goes on.
fn by_range(test: &str) -> bool {
    if std::env::var("DM_STAGE_BY_RANGE").as_deref() == Ok("1") {
        return true;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("DM_STAGE_BY_RANGE", "1")
        .status()
        .unwrap();
    assert!(status.success(), "{test} failed with a file a range");
    false
}

fn body(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// The range a request asks for, if it asks for one.
fn asked(headers: &HeaderMap, total: usize) -> Option<(usize, usize)> {
    let (a, b) = headers
        .get("range")?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?
        .split_once('-')?;
    let a: usize = a.parse().ok()?;
    Some((a, b.parse::<usize>().unwrap_or(total - 1).min(total - 1)))
}

/// What a server was asked for: the first byte of each range that names its end, in the order
/// asked. The probe's (`bytes=0-`, or the file's first part on several connections) is the first
/// asked; dropped after its headers on a resume, it says nothing of what was fetched again.
#[derive(Default)]
struct Asked(Mutex<Vec<usize>>);

/// A server of `data` that honours ranges under one ETag, sending 16 KiB at a time with the
/// pause `slow` gives a request (by the byte it starts at) before each.
async fn server(
    data: Arc<Vec<u8>>,
    slow: impl Fn(usize) -> Duration + Clone + Send + Sync + 'static,
    asked_for: Arc<Asked>,
) -> String {
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let (data, asked_for, slow) = (data.clone(), asked_for.clone(), slow.clone());
            async move {
                let total = data.len();
                let (status, from, to) = match asked(&h, total) {
                    Some((a, b)) => (206, a, b),
                    None => (200, 0, total - 1),
                };
                let pause = slow(from);
                let ended = h
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| !v.ends_with('-'));
                if ended {
                    asked_for.0.lock().unwrap().push(from);
                }
                let chunks: Vec<bytes::Bytes> = data[from..=to]
                    .chunks(16 * 1024)
                    .map(bytes::Bytes::copy_from_slice)
                    .collect();
                let stream = futures_util::stream::iter(chunks).then(move |c| async move {
                    if !pause.is_zero() {
                        tokio::time::sleep(pause).await;
                    }
                    Ok::<_, std::io::Error>(c)
                });
                let mut answer = Response::builder()
                    .status(status)
                    .header("etag", "\"v1\"")
                    .header("accept-ranges", "bytes")
                    .header("content-length", to + 1 - from);
                if status == 206 {
                    answer = answer.header("content-range", format!("bytes {from}-{to}/{total}"));
                }
                answer.body(Body::from_stream(stream)).unwrap()
            }
        }),
    );
    format!("http://{}/f.bin", serve(app).await)
}

/// The names in the parts folder of a test's home.
fn names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir.join("parts"))
        .map(|it| {
            it.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// What range `index`'s ledger says is on disk.
fn ledger(dir: &Path, index: usize) -> u64 {
    std::fs::read_to_string(dir.join("parts").join(format!("done-{index:04}")))
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0)
}

/// Bytes on disk across every range, as the ledgers say.
fn staged(dir: &Path) -> u64 {
    names(dir)
        .iter()
        .filter_map(|n| n.strip_prefix("done-")?.parse::<usize>().ok())
        .map(|i| ledger(dir, i))
        .sum()
}

/// Every name the parts folder holds, looked at every 5 ms until the task is aborted.
fn watch(dir: &Path) -> (tokio::task::JoinHandle<()>, Arc<Mutex<BTreeSet<String>>>) {
    let seen = Arc::new(Mutex::new(BTreeSet::new()));
    let (dir, into) = (dir.to_path_buf(), Arc::clone(&seen));
    let watching = tokio::spawn(async move {
        loop {
            into.lock().unwrap().extend(names(&dir));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    (watching, seen)
}

/// The first run of a download paused once `at_least` bytes of it are on disk.
async fn paused_at(url: &str, dir: &Path, at_least: u64) {
    let o = options(dir);
    let control = o.control.clone();
    let home = dir.to_path_buf();
    tokio::spawn(async move {
        while staged(&home) < at_least {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        control.pause();
    });
    let paused = download(&client(), url, &o).await;
    assert!(paused.is_err(), "the first run was meant to be paused");
    let names = names(dir);
    assert!(
        names.iter().any(|n| n.starts_with("range-")) && !names.iter().any(|n| n == "staged"),
        "not a file a range: {names:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_range_is_a_file_of_its_own_and_the_file_comes_out_whole() {
    if !by_range("each_range_is_a_file_of_its_own_and_the_file_comes_out_whole") {
        return;
    }
    let data = Arc::new(body(4 * 1024 * 1024, 7));
    let url = server(data.clone(), |_| Duration::from_millis(10), Arc::default()).await;
    let dir = tempdir();
    let (watching, seen) = watch(dir.path());
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("the download failed");
    watching.abort();
    assert!(
        std::fs::read(&done.path).unwrap() == *data,
        "the saved file is not the file that was served"
    );
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.iter().any(|n| n.starts_with("range-")),
        "no range had a file of its own: {seen:?}"
    );
    assert!(!seen.contains("staged"), "a staged file was made: {seen:?}");
    assert_eq!(
        names(dir.path()),
        Vec::<String>::new(),
        "the parts folder is left as it was found"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_download_goes_on_from_its_range_files() {
    if !by_range("a_paused_download_goes_on_from_its_range_files") {
        return;
    }
    let data = Arc::new(body(4 * 1024 * 1024, 11));
    let slow = server(data.clone(), |_| Duration::from_millis(20), Arc::default()).await;
    let dir = tempdir();
    paused_at(&slow, dir.path(), 1024 * 1024).await;
    // What each range had on disk, short of the end a resume checks again (64 KiB).
    let plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("parts").join("plan.json")).unwrap())
            .unwrap();
    let kept: Vec<(usize, usize)> = plan["ranges"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(i, range)| {
            let start = range[0].as_u64()? as usize;
            let had = ledger(dir.path(), i) as usize;
            (had > 64 * 1024).then_some((start, start + had - 64 * 1024))
        })
        .collect();
    assert!(!kept.is_empty(), "nothing on disk to go on from");

    let asked_for = Arc::new(Asked::default());
    let fast = server(data.clone(), |_| Duration::ZERO, Arc::clone(&asked_for)).await;
    let done = download(&client(), &fast, &options(dir.path()))
        .await
        .expect("the resumed download failed");
    assert!(
        std::fs::read(&done.path).unwrap() == *data,
        "the saved file is not the file that was served"
    );
    let asked_for = asked_for.0.lock().unwrap().clone();
    // After the probe, which is asked before any range is.
    for from in asked_for.iter().skip(1) {
        assert!(
            !kept.iter().any(|&(start, end)| (start..end).contains(from)),
            "asked again from {from}, inside what a range had: {kept:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_range_file_a_power_cut_tore_is_fetched_again_where_it_was_torn() {
    if !by_range("a_range_file_a_power_cut_tore_is_fetched_again_where_it_was_torn") {
        return;
    }
    let data = Arc::new(body(4 * 1024 * 1024, 23));
    let slow = server(data.clone(), |_| Duration::from_millis(20), Arc::default()).await;
    let dir = tempdir();
    paused_at(&slow, dir.path(), 2 * 1024 * 1024).await;
    let parts = dir.path().join("parts");
    // Torn as a power cut tears: one range's file shorter than its ledger says, and another's
    // last whole block under its ledger never written (zeros). A run a power cut ended wrote no
    // note that its bytes were synced, as the pause that stopped this one did (gaps row 186):
    // taken away, so the ledgers are all there is to go by.
    for entry in std::fs::read_dir(&parts).unwrap().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".synced") {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    let (short, zeroed) = (1, 2);
    let said = ledger(dir.path(), short);
    assert!(
        said > 64 * 1024,
        "too little of range {short} to tear: {said}"
    );
    std::fs::OpenOptions::new()
        .write(true)
        .open(parts.join(format!("range-{short:04}")))
        .unwrap()
        .set_len(said - 10_000)
        .unwrap();
    let said = ledger(dir.path(), zeroed);
    assert!(
        said > 64 * 1024,
        "too little of range {zeroed} to tear: {said}"
    );
    let block = said / 4096 * 4096 - 4096;
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(parts.join(format!("range-{zeroed:04}")))
            .unwrap();
        file.seek(SeekFrom::Start(block)).unwrap();
        file.write_all(&[0u8; 4096]).unwrap();
    }

    let fast = server(data.clone(), |_| Duration::ZERO, Arc::default()).await;
    let done = download(&client(), &fast, &options(dir.path()))
        .await
        .expect("the resumed download failed");
    assert!(
        std::fs::read(&done.path).unwrap() == *data,
        "the torn ranges were not repaired"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_range_halved_under_its_connection_comes_out_whole() {
    if !by_range("a_range_halved_under_its_connection_comes_out_whole") {
        return;
    }
    // The file's first range served slowly, the others at once: the connections done with theirs
    // halve it under its own, which stops at the new end it is given.
    let data = Arc::new(body(4 * 1024 * 1024, 31));
    let url = server(
        data.clone(),
        |from| {
            if from == 0 {
                Duration::from_millis(30)
            } else {
                Duration::ZERO
            }
        },
        Arc::default(),
    )
    .await;
    let dir = tempdir();
    let (watching, seen) = watch(dir.path());
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("the download failed");
    watching.abort();
    assert!(
        std::fs::read(&done.path).unwrap() == *data,
        "the saved file is not the file that was served"
    );
    let seen = seen.lock().unwrap().clone();
    let most = seen
        .iter()
        .filter_map(|n| n.strip_prefix("range-")?.parse::<usize>().ok())
        .max();
    // A range halved is one past those planned: four of a megabyte for these 4 MiB.
    let planned = dm_core::plan(Some(data.len() as u64), 8, true).len();
    assert!(
        most.is_some_and(|i| i >= planned),
        "no range was halved into a file of its own: {seen:?}"
    );
}
