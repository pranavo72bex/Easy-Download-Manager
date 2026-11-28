// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::sync::Arc;

use super::*;
use crate::download::dash::tests::{options, serve_files};

/// What a server said of `name`, `size` bytes, with `etag`.
fn probed(name: &str, size: u64, etag: Option<&str>) -> Probe {
    Probe {
        size: Some(size),
        resumable: true,
        filename: name.into(),
        validator: etag.map(Into::into),
        final_url: format!("http://mirror.example/{name}"),
        content_type: None,
        modified: None,
        digest: None,
    }
}

/// A finished file of `size` bytes at `dir/done/name`, its server's word `etag`.
fn finished(dir: &Path, name: &str, size: u64, etag: Option<&str>) -> SameFile {
    let path = dir.join("done").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, vec![7u8; size as usize]).unwrap();
    SameFile {
        name: name.into(),
        size,
        validator: etag.map(Into::into),
        digest: None,
        path,
    }
}

/// Options that compare with `finished`.
fn asking(dir: &Path, finished: Vec<SameFile>) -> Options {
    Options {
        same_files: Some(Arc::new(SameFiles {
            finished,
            passed_over: Vec::new(),
        })),
        ..options(dir)
    }
}

fn kind(outcome: Result<(), Error>) -> Option<FailKind> {
    outcome.err().map(|e| e.fail_kind())
}

/// The same name, size and entity tag as a finished download: stopped, to be asked about.
#[tokio::test]
async fn the_same_name_size_and_etag_as_a_finished_file_is_asked_about() {
    let dir = tempfile::tempdir().unwrap();
    let done = finished(dir.path(), "setup.exe", 4096, Some("\"v1\""));
    let options = asking(dir.path(), vec![done]);
    let info = probed("setup.exe", 4096, Some("\"v1\""));
    assert_eq!(
        kind(already_here(&info, &options).await),
        Some(FailKind::SameFile)
    );
    // Where only one side has an entity tag, the name and size decide.
    let untagged = probed("setup.exe", 4096, None);
    assert_eq!(
        kind(already_here(&untagged, &options).await),
        Some(FailKind::SameFile)
    );
}

/// Another size, another entity tag, another name, a finished file gone from the disk, or
/// nobody to ask: fetched as before.
#[tokio::test]
async fn another_size_or_etag_is_not_asked_about() {
    let dir = tempfile::tempdir().unwrap();
    let done = finished(dir.path(), "setup.exe", 4096, Some("\"v1\""));
    let options = asking(dir.path(), vec![done.clone()]);
    for info in [
        probed("setup.exe", 4097, Some("\"v1\"")),
        probed("setup.exe", 4096, Some("\"v2\"")),
        probed("setup2.exe", 4096, Some("\"v1\"")),
    ] {
        assert_eq!(kind(already_here(&info, &options).await), None, "{info:?}");
    }
    let unasked = Options {
        same_files: None,
        ..options.clone()
    };
    let info = probed("setup.exe", 4096, Some("\"v1\""));
    assert_eq!(kind(already_here(&info, &unasked).await), None);
    std::fs::remove_file(&done.path).unwrap();
    assert_eq!(kind(already_here(&info, &options).await), None);
}

/// A file of the name and size in the folder it would be saved in, from before: asked about;
/// one a person already chose to fetch again by its address is not asked about twice.
#[tokio::test]
async fn a_file_already_in_the_folder_is_asked_about() {
    let dir = tempfile::tempdir().unwrap();
    let options = asking(dir.path(), Vec::new());
    std::fs::create_dir_all(&options.dest_dir).unwrap();
    let there = options.dest_dir.join("photos.zip");
    std::fs::write(&there, vec![1u8; 300]).unwrap();
    let info = probed("photos.zip", 300, Some("\"x\""));
    assert_eq!(
        kind(already_here(&info, &options).await),
        Some(FailKind::SameFile)
    );
    let bigger = probed("photos.zip", 301, None);
    assert_eq!(kind(already_here(&bigger, &options).await), None);
    let passed = Options {
        same_files: Some(Arc::new(SameFiles {
            finished: Vec::new(),
            passed_over: vec![there],
        })),
        ..options.clone()
    };
    assert_eq!(kind(already_here(&info, &passed).await), None);
}

/// A second address of a finished file, with the same name and size: held before any of its
/// body is fetched (nothing staged, nothing saved), and fetched whole once nobody is asked.
#[tokio::test]
async fn a_second_address_of_a_finished_file_is_held_before_its_body() {
    let dir = tempfile::tempdir().unwrap();
    let body = vec![9u8; 200_000];
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let base = serve_files(vec![("/mirror/data.bin".to_string(), body.clone())], asked).await;
    let url = format!("{base}/mirror/data.bin");
    let done = finished(dir.path(), "data.bin", body.len() as u64, None);
    let options = asking(dir.path(), vec![done]);
    let client = crate::reqwest_defaults();
    let held = crate::download::download(&client, &url, &options).await;
    let error = held.expect_err("the same file is asked about");
    assert_eq!(error.fail_kind(), FailKind::SameFile);
    assert!(!options.dest_dir.join("data.bin").exists());
    assert!(
        !options.parts_dir.exists(),
        "nothing of its body was staged"
    );

    // Another size: not the same file, fetched as before.
    let other = finished(dir.path(), "data.bin", 10, None);
    let fetched = crate::download::download(&client, &url, &asking(dir.path(), vec![other]))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&fetched.path).unwrap(), body);
}
