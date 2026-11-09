// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm does with a finished file, apart from the system's own openers
//! (those are `Platform`'s): a ZIP extracted off the presentation's thread,
//! and a ZIP's files listed, from its address before it is fetched or from
//! the finished file. Only text: nothing in an archive is opened or followed.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The files of a ZIP, as a background reading leaves them: `None` while it
/// is under way, an empty list where there was nothing to list.
pub type ZipSlot = Arc<Mutex<Option<Vec<dm_core::ZipEntry>>>>;

/// How long the reading of an address may take before the list is left out.
pub const ZIP_WITHIN: std::time::Duration = std::time::Duration::from_secs(30);

/// Starts reading the files of the ZIP at `url` in the background, with the
/// application's client (`dm_core::zip_contents`: two or three small range
/// requests, never the archive), and gives the slot it fills; `wake` is
/// called once it is filled. Nothing found, or no answer in [`ZIP_WITHIN`],
/// is an empty list.
pub fn list_zip_at(
    backend: &crate::app::backend::Backend,
    url: String,
    referrer: Option<String>,
    sign_in: Option<dm_core::cookies::SignIn>,
    wake: impl FnOnce() + Send + 'static,
) -> ZipSlot {
    let held: ZipSlot = Arc::default();
    let into = Arc::clone(&held);
    let client = backend.client();
    backend.runtime.spawn(async move {
        let read = dm_core::zip_contents(&client, &url, referrer.as_deref(), sign_in.as_ref());
        let found = tokio::time::timeout(ZIP_WITHIN, read)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        *dm_core::lock(&into) = Some(found);
        wake();
    });
    held
}

/// Starts reading the files of the finished ZIP at `path` in the background,
/// and gives the slot it fills; `wake` is called once it is filled.
pub fn list_zip_file(
    backend: &crate::app::backend::Backend,
    path: PathBuf,
    wake: impl FnOnce() + Send + 'static,
) -> ZipSlot {
    let held: ZipSlot = Arc::default();
    let into = Arc::clone(&held);
    backend.runtime.spawn_blocking(move || {
        let found = dm_core::zip_contents_of_file(&path).unwrap_or_default();
        *dm_core::lock(&into) = Some(found);
        wake();
    });
    held
}

/// What the hashing of a finished file has found, by algorithm name: `None`
/// while it works, then the digest or why there is none.
pub type Sums = Arc<Mutex<std::collections::HashMap<&'static str, Option<Result<String, String>>>>>;

/// Starts hashing the finished file at `path` with `algorithm` in the
/// background, unless that is already under way or done: a large file takes
/// a while, and the presentation must not.
pub fn hash_later(
    backend: &crate::app::backend::Backend,
    sums: &Sums,
    path: &str,
    algorithm: dm_core::Algorithm,
) {
    let name = algorithm.name();
    {
        let mut held = dm_core::lock(sums);
        if held.contains_key(name) {
            return;
        }
        held.insert(name, None);
    }
    let (sums, path) = (Arc::clone(sums), PathBuf::from(path));
    backend.runtime.spawn(async move {
        let found = dm_core::digest(&path, algorithm)
            .await
            .map_err(|e| e.to_string());
        dm_core::lock(&sums).insert(name, Some(found));
    });
}

/// What has been found for `algorithm`: still working (`None`), the digest,
/// or why not.
pub fn sum_of(sums: &Sums, algorithm: dm_core::Algorithm) -> Option<Result<String, String>> {
    dm_core::lock(sums).get(algorithm.name()).cloned().flatten()
}

/// Extracts the finished ZIP at `path` (fetched from `url`, which it is
/// checked against) into a new folder beside it, or inside `into`, on a
/// thread of its own; nothing in it is opened or run. `done` is given what
/// was extracted, or why not, when it is over. Err if the thread could not
/// be started (then `done` is never called).
pub fn extract_later(
    path: PathBuf,
    url: String,
    into: Option<PathBuf>,
    done: impl FnOnce(Result<dm_core::extract::Extracted, String>) + Send + 'static,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("dm-extract".into())
        .spawn(move || {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let origin = (!url.is_empty()).then_some(url.as_str());
            let limits = dm_core::extract::Limits::default();
            let answer = dm_core::extract::extract_in(
                &path,
                into.as_deref(),
                &limits,
                &cancel,
                origin,
                &|_, _| {},
            )
            .map_err(|e| e.to_string());
            done(answer);
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_not_a_zip_is_said_and_nothing_is_made() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not.zip");
        std::fs::write(&path, b"not a zip").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        extract_later(path, String::new(), None, move |answer| {
            tx.send(answer).unwrap();
        })
        .unwrap();
        let answer = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(answer.is_err(), "{answer:?}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
