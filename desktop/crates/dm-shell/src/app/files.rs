// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm does with a finished file, apart from the system's own openers
//! (those are `Platform`'s): a ZIP extracted off the presentation's thread.

use std::path::PathBuf;

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
