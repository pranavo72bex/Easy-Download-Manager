// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where dm finds the JavaScript engine for YouTube's player challenges
//! (`videopage::quickjs`, through `tools::ensure`): the program named
//! outright, the `PATH`, the places package managers put it, and the copy
//! dm fetched itself.
//!
//! One test here changes `PATH` and the `DM_` variables, which are the
//! process's, and turns fetching off so nothing is fetched from the
//! network; the other reads no environment.

#![cfg(unix)]

use std::path::Path;

use dm_core::tools::{self, Tool};

fn put(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"#!/bin/sh\n").unwrap();
}

#[tokio::test]
async fn the_engine_is_found_where_it_was_put() {
    let dir = tempfile::tempdir().unwrap();
    let client = dm_core::client();
    let fetched = dir.path().join("tools/qjs");
    tools::set_folder(dir.path().join("tools"));
    tools::allow_fetching(false);
    std::env::remove_var("DM_ONLY_FETCHED_TOOLS");

    // Named outright: that file, or none where it is not one.
    let named = dir.path().join("named/qjs");
    put(&named);
    std::env::set_var("DM_QJS", &named);
    assert_eq!(tools::ensure(&client, Tool::QuickJs).await.unwrap(), named);
    std::env::set_var("DM_QJS", dir.path().join("named/missing"));
    let why = tools::ensure(&client, Tool::QuickJs).await.unwrap_err();
    assert_eq!(why.to_string(), tools::NOT_FETCHED, "fetching is off");
    std::env::remove_var("DM_QJS");

    // On the `PATH`, before anywhere else.
    let on_path = dir.path().join("bin/qjs");
    put(&on_path);
    let path = std::env::var_os("PATH");
    std::env::set_var(
        "PATH",
        format!(
            "{}:{}",
            dir.path().join("empty").display(),
            dir.path().join("bin").display()
        ),
    );
    let found = tools::ensure(&client, Tool::QuickJs).await;
    std::fs::remove_file(&on_path).unwrap();
    assert_eq!(found.unwrap(), on_path);

    // Not on the `PATH`: the copy dm fetched, where no other is installed.
    put(&fetched);
    let found = tools::ensure(&client, Tool::QuickJs).await.unwrap();
    assert!(
        found == fetched || !found.starts_with(dir.path()),
        "an installed one, or dm's own: {found:?}"
    );

    // As a computer with nothing installed: dm's own copy alone, and with
    // none, nothing (and fetching is off).
    std::env::set_var("DM_ONLY_FETCHED_TOOLS", "1");
    std::fs::write(dir.path().join("bin/qjs"), b"#!/bin/sh\n").unwrap();
    assert_eq!(
        tools::ensure(&client, Tool::QuickJs).await.unwrap(),
        fetched
    );
    std::fs::remove_file(&fetched).unwrap();
    let why = tools::ensure(&client, Tool::QuickJs).await.unwrap_err();
    assert_eq!(why.to_string(), tools::NOT_FETCHED);

    std::env::remove_var("DM_ONLY_FETCHED_TOOLS");
    match path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    tools::allow_fetching(true);
}

/// A YouTube watch page is one whose own data lists its qualities; another
/// site's page is not.
#[test]
fn a_watch_page_is_told_from_another_page() {
    assert!(dm_core::videopage::is_watch_page(
        "https://www.youtube.com/watch?v=jNQXAC9IVRw"
    ));
    assert!(!dm_core::videopage::is_watch_page(
        "https://example.com/watch?v=jNQXAC9IVRw"
    ));
}
