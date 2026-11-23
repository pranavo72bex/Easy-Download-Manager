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

/// One file taken out of a ZIP on a server: its name, and where it was saved
/// or why it was not. `None` while under way.
pub type TakeSlot = Arc<Mutex<Option<(String, Result<PathBuf, String>)>>>;

/// How long taking one file out of an archive may take.
pub const TAKE_WITHIN: std::time::Duration = std::time::Duration::from_secs(300);

/// Takes `entry` out of the ZIP at `url` into `folder`, on the application's
/// client (`dm_core::zip_take_one`: range requests for that file's own bytes
/// alone), and wakes the window when it is done.
pub fn take_from_zip(
    backend: &crate::app::backend::Backend,
    url: String,
    referrer: Option<String>,
    sign_in: Option<dm_core::cookies::SignIn>,
    entry: dm_core::ZipEntry,
    folder: PathBuf,
    wake: impl FnOnce() + Send + 'static,
) -> TakeSlot {
    let held: TakeSlot = Arc::default();
    let into = Arc::clone(&held);
    let client = backend.client();
    backend.runtime.spawn(async move {
        let take = dm_core::zip_take_one(
            &client,
            &url,
            referrer.as_deref(),
            sign_in.as_ref(),
            &entry,
            &folder,
        );
        let done = tokio::time::timeout(TAKE_WITHIN, take)
            .await
            .unwrap_or_else(|_| Err("the server took too long".into()));
        let name = dm_core::sanitize_name(&entry.name);
        *dm_core::lock(&into) = Some((name, done));
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

/// Extracts the finished archive (ZIP, tar, 7z or RAR) at `path` (fetched from `url`, which it is
/// checked against) into a new folder beside it, or inside `into`, on a
/// thread of its own, once a turn at the computer's heavy work is free
/// (`dm_core::heavy`); nothing in it is opened or run. `done` is given what
/// was extracted, or why not, when it is over. Err if the thread could not
/// be started (then `done` is never called).
pub fn extract_later(
    path: PathBuf,
    url: String,
    into: Option<PathBuf>,
    done: impl FnOnce(Result<dm_core::extract::Extracted, String>) + Send + 'static,
) -> Result<(), crate::Error> {
    extract_later_with(path, url, into, None, move |answer| {
        done(answer.map_err(|e| e.to_string()))
    })
}

/// [`extract_later`] with the archive's `password` (gaps row F10): a ZIP's AES or older
/// cipher, a 7z's AES. `done` is given dm's own error, so that a window can tell an archive
/// that wants a password (`Error::Encrypted`, as `None` gives for one) from a password that
/// is not its own (`Error::WrongPassword`), and ask again. The password goes with the
/// extracting thread and is dropped with it, kept nowhere.
pub fn extract_later_with(
    path: PathBuf,
    url: String,
    into: Option<PathBuf>,
    password: Option<String>,
    done: impl FnOnce(Result<dm_core::extract::Extracted, dm_core::extract::Error>) + Send + 'static,
) -> Result<(), crate::Error> {
    std::thread::Builder::new()
        .name("dm-extract".into())
        .spawn(move || {
            // A turn at the computer's heavy work, as digests and joins take one, so ten ZIPs
            // extracted at once do not take every core (gaps row 185).
            let _turn = dm_core::heavy::turn();
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let origin = (!url.is_empty()).then_some(url.as_str());
            let limits = dm_core::extract::Limits::default();
            let answer = dm_core::extract::extract_in_with(
                &path,
                into.as_deref(),
                password.as_deref(),
                &limits,
                &cancel,
                origin,
                &|_, _| {},
            );
            done(answer);
        })
        .map(|_| ())
        .map_err(|e| crate::Error::from(e.to_string()))
}

/// [`extract_later_with`] for an archive whose password is not known: first with none, then,
/// where it has one, with each of `passwords` in turn until one opens it (gaps row C2). `done`
/// is given the answer and the password that opened it, if one did; an archive none of them
/// opens is the last answer, a password's (`Error::WrongPassword`) or none's (`Encrypted`).
pub fn extract_later_trying(
    path: PathBuf,
    url: String,
    into: Option<PathBuf>,
    passwords: Vec<String>,
    done: impl FnOnce(Result<dm_core::extract::Extracted, dm_core::extract::Error>, Option<String>)
        + Send
        + 'static,
) -> Result<(), crate::Error> {
    std::thread::Builder::new()
        .name("dm-extract".into())
        .spawn(move || {
            let _turn = dm_core::heavy::turn();
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let origin = (!url.is_empty()).then_some(url.as_str());
            let limits = dm_core::extract::Limits::default();
            let attempt = |password: Option<&str>| {
                dm_core::extract::extract_in_with(
                    &path,
                    into.as_deref(),
                    password,
                    &limits,
                    &cancel,
                    origin,
                    &|_, _| {},
                )
            };
            let mut answer = attempt(None);
            for password in &passwords {
                if !matches!(
                    answer,
                    Err(dm_core::extract::Error::Encrypted(_)
                        | dm_core::extract::Error::WrongPassword(_))
                ) {
                    break;
                }
                answer = attempt(Some(password));
                if answer.is_ok() {
                    done(answer, Some(password.clone()));
                    return;
                }
            }
            done(answer, None);
        })
        .map(|_| ())
        .map_err(|e| crate::Error::from(e.to_string()))
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

    #[test]
    fn a_zip_with_a_password_says_which_answer_it_wants() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked.zip");
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dm-core/tests/fixtures/archives/zip/aes256.zip");
        std::fs::copy(fixtures, &path).unwrap();
        let ask = |password: Option<&str>| {
            let (tx, rx) = std::sync::mpsc::channel();
            let pw = password.map(str::to_string);
            extract_later_with(path.clone(), String::new(), None, pw, move |answer| {
                tx.send(answer).unwrap();
            })
            .unwrap();
            rx.recv_timeout(std::time::Duration::from_secs(60)).unwrap()
        };
        use dm_core::extract::Error;
        assert!(matches!(ask(None), Err(Error::Encrypted(_))));
        assert!(matches!(ask(Some("no")), Err(Error::WrongPassword(_))));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let done = ask(Some("dm secret")).unwrap();
        let hello = std::fs::read(done.folder.join("docs/a.txt")).unwrap();
        assert_eq!(hello, b"hello, zip\n");
    }

    /// An archive with a password is opened by the one of those kept that is its own, tried in
    /// turn after none, and which one is said (gaps row C2); one none opens stays packed.
    #[test]
    fn a_zip_with_a_password_is_opened_by_the_one_kept_that_fits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked.zip");
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dm-core/tests/fixtures/archives/zip/aes256.zip");
        std::fs::copy(fixtures, &path).unwrap();
        let tried = |passwords: &[&str]| {
            let (tx, rx) = std::sync::mpsc::channel();
            let kept = passwords.iter().map(|p| p.to_string()).collect();
            extract_later_trying(
                path.clone(),
                String::new(),
                None,
                kept,
                move |answer, used| {
                    tx.send((answer, used)).unwrap();
                },
            )
            .unwrap();
            rx.recv_timeout(std::time::Duration::from_secs(60)).unwrap()
        };
        let (answer, used) = tried(&["no", "nor this"]);
        assert!(answer.is_err() && used.is_none());
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "nothing made"
        );
        let (answer, used) = tried(&["no", "dm secret", "after it"]);
        assert_eq!(used.as_deref(), Some("dm secret"));
        let done = answer.unwrap();
        assert_eq!(
            std::fs::read(done.folder.join("docs/a.txt")).unwrap(),
            b"hello, zip\n"
        );
    }
}
