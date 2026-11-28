// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download into a folder whose full path is longer than Windows' old 260 characters
//! (`MAX_PATH`), and names in Chinese, Japanese, Korean and emoji (gaps row 191): the file lands
//! whole, its parts staged in a long path too, and its row says where it is. Rust's `std::fs`
//! reaches such a path on Windows by itself; this holds the engine to it on every system, the
//! Windows CI job's included.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use edm_core::{client, AddOptions, DownloadList, State as DownloadState};

use common::tempdir;

/// The file's size: four of the planner's smallest pieces and three bytes, so it is fetched over
/// several connections.
const SIZE: usize = 4 * 1024 * 1024 + 3;

/// The fewest characters the folders' full paths have: past Windows' `MAX_PATH`, 260.
const PATH_CHARS: usize = 300;

/// The name the first file is given by its address.
const FIRST: &str = "下载 ダウンロード 다운로드 🎬📦.bin";

/// A folder's name with every script the row names in it.
const NAMED_FOLDER: &str = "保存 ほぞん 저장 📁✨";

/// The name the second file is given by its address.
const SECOND: &str = "歌 うた 노래 🎵🎶.bin";

/// Serves `data` at every address, whole or in the range asked, so a download takes it over
/// several connections.
async fn ranged(State(data): State<Arc<Vec<u8>>>, headers: HeaderMap) -> Response {
    let size = data.len();
    let asked = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split_once('-'))
        .map(|(a, b)| {
            (
                a.parse::<usize>().unwrap_or(0),
                b.parse::<usize>().unwrap_or(size - 1).min(size - 1),
            )
        });
    let Some((a, b)) = asked else {
        return Response::builder()
            .header("accept-ranges", "bytes")
            .header("content-length", size)
            .body(Body::from(data.to_vec()))
            .unwrap();
    };
    if a > b {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .body(Body::empty())
            .unwrap();
    }
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("accept-ranges", "bytes")
        .header("content-length", b - a + 1)
        .header("content-range", format!("bytes {a}-{b}/{size}"))
        .body(Body::from(data[a..=b].to_vec()))
        .unwrap()
}

/// A folder under `root` whose full path has at least [`PATH_CHARS`] characters, in parts of
/// under 40 (each well within any file system's 255).
fn long_folder(root: &Path) -> PathBuf {
    let mut folder = root.to_path_buf();
    let mut n = 0;
    while folder.to_string_lossy().chars().count() < PATH_CHARS {
        folder.push(format!("{n:02} a folder deep in a long path"));
        n += 1;
    }
    folder
}

/// `name`'s address on the server at `addr`, its name percent-encoded as a browser sends it.
fn address(addr: std::net::SocketAddr, name: &str) -> String {
    let mut url = url::Url::parse(&format!("http://{addr}/")).unwrap();
    url.set_path(&format!("/files/{name}"));
    url.to_string()
}

#[tokio::test]
async fn a_file_lands_whole_past_260_characters_with_cjk_and_emoji_names() {
    let data: Vec<u8> = (0..SIZE).map(|i| (i % 251) as u8).collect();
    let app = axum::Router::new()
        .fallback(ranged)
        .with_state(Arc::new(data.clone()));
    let addr = common::serve(app).await;
    let dir = tempdir();
    let long = long_folder(dir.path());
    let named = long.join(NAMED_FOLDER);
    // The parts are staged in a long path too, by every connection.
    let q = DownloadList::new(
        client(),
        edm_core::Options {
            parts_dir: long.join("parts"),
            ..common::options(dir.path())
        },
        2,
    );
    let mut rows = Vec::new();
    for (folder, name) in [(&long, FIRST), (&named, SECOND)] {
        let id = q
            .add_with(
                &address(addr, name),
                AddOptions {
                    save_dir: Some(folder.to_string_lossy().into_owned()),
                    ..AddOptions::default()
                },
            )
            .await
            .unwrap();
        rows.push((id, folder.join(name), name));
    }
    q.run().await;

    for (id, want, name) in rows {
        let row = q.get(id).await.unwrap();
        assert_eq!(
            row.state,
            DownloadState::Completed,
            "{name}: {:?}",
            row.state
        );
        assert_eq!(row.filename, name);
        // Where it is, as it was asked for: not shortened, nor in Windows' `\\?\` form.
        assert_eq!(Path::new(&row.path), want);
        assert!(
            row.path.chars().count() > PATH_CHARS,
            "{} characters: {}",
            row.path.chars().count(),
            row.path
        );
        let landed = std::fs::read(&row.path).unwrap();
        assert!(
            landed == data,
            "{name}: {} bytes landed of {SIZE}, or others",
            landed.len()
        );
    }
}
