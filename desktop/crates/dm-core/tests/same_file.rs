// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The same file from another address (gaps row W18): where a window asks, a download whose
//! server gives the name, size and entity tag of a finished row's file stops before its body,
//! failed as `FailKind::SameFile`, for the person to be asked; Download again lets it through.
//! Where nobody is asked (headless, the control port), it is fetched as before.

mod common;

use std::path::Path;

use axum::body::Body;
use axum::response::Response;
use dm_core::{client, AddOptions, DownloadList, FailKind, State};

use common::tempdir;

const BODY: &[u8] = b"the same bytes, from either address";

/// Serves [`BODY`] at every address, with one entity tag.
async fn serve() -> std::net::SocketAddr {
    async fn handler() -> Response {
        Response::builder()
            .header("content-length", BODY.len())
            .header("etag", "\"same-v1\"")
            .body(Body::from(BODY))
            .unwrap()
    }
    common::serve(axum::Router::new().fallback(handler)).await
}

fn queue(dir: &Path) -> DownloadList {
    DownloadList::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir)
        },
        2,
    )
}

/// The files in the download folder.
fn saved(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join("out"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `address` added and served until the queue is idle: its row.
async fn fetched(q: &DownloadList, address: String, with: AddOptions) -> dm_core::Entry {
    let id = q.add_with(&address, with).await.unwrap();
    q.run().await;
    q.get(id).await.unwrap()
}

#[tokio::test]
async fn a_second_address_of_a_finished_file_waits_to_be_asked_about() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    q.ask_about_same_files(true);
    let first = fetched(
        &q,
        format!("http://{addr}/a/same.bin"),
        AddOptions::default(),
    )
    .await;
    assert_eq!(first.state, State::Completed);

    let second = fetched(
        &q,
        format!("http://{addr}/b/same.bin"),
        AddOptions::default(),
    )
    .await;
    assert_eq!(
        second.failed_as(),
        Some(FailKind::SameFile),
        "{:?}",
        second.state
    );
    assert_eq!(second.filename, "same.bin", "named for the question");
    assert_eq!(
        saved(dir.path()),
        ["same.bin"],
        "nothing fetched a second time"
    );

    // Download again: let through, and saved beside the first.
    assert!(q.accept_same(second.id).await);
    assert!(q.retry(second.id).await);
    q.run().await;
    assert_eq!(q.get(second.id).await.unwrap().state, State::Completed);
    assert_eq!(saved(dir.path()), ["same (1).bin", "same.bin"]);
}

#[tokio::test]
async fn nobody_to_ask_or_the_control_port_fetches_it_as_before() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let first = fetched(
        &q,
        format!("http://{addr}/a/same.bin"),
        AddOptions::default(),
    )
    .await;
    assert_eq!(first.state, State::Completed);
    // No window has turned the question on: fetched again.
    let headless = fetched(
        &q,
        format!("http://{addr}/b/same.bin"),
        AddOptions::default(),
    )
    .await;
    assert_eq!(headless.state, State::Completed);
    // With a window, an add from the control port is not asked about either.
    q.ask_about_same_files(true);
    let port = AddOptions {
        accept_same: true,
        ..AddOptions::default()
    };
    let scripted = fetched(&q, format!("http://{addr}/c/same.bin"), port).await;
    assert_eq!(scripted.state, State::Completed);
    assert_eq!(
        saved(dir.path()),
        ["same (1).bin", "same (2).bin", "same.bin"]
    );
}

#[tokio::test]
async fn a_file_already_in_the_folder_is_asked_about() {
    let addr = serve().await;
    let dir = tempdir();
    std::fs::create_dir_all(dir.path().join("out")).unwrap();
    std::fs::write(dir.path().join("out").join("same.bin"), BODY).unwrap();
    let q = queue(dir.path());
    q.ask_about_same_files(true);
    let row = fetched(
        &q,
        format!("http://{addr}/a/same.bin"),
        AddOptions::default(),
    )
    .await;
    assert_eq!(row.failed_as(), Some(FailKind::SameFile), "{:?}", row.state);
    // Another size in the folder is another file.
    std::fs::write(dir.path().join("out").join("same.bin"), b"shorter").unwrap();
    assert!(q.retry(row.id).await);
    q.run().await;
    assert_eq!(q.get(row.id).await.unwrap().state, State::Completed);
}
