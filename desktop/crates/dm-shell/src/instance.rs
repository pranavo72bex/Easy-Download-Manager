// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! One dm at a time.
//!
//! A browser hand-off must not start a second copy of the app, with a second
//! window, a second tray icon and a second queue writing the same
//! `queue.json` as the first. An advisory lock on one file under dm's home
//! is the whole mechanism: the running app holds it for its lifetime, and
//! anything that wants to know whether dm is running tries to take it.

use std::fs::File;
use std::path::Path;
use std::time::Duration;

#[cfg(windows)]
mod owner_only;

/// The file whose lock says dm is running.
fn lock_path(home: &Path) -> std::path::PathBuf {
    home.join("instance.lock")
}

/// How long a claim keeps trying a lock held by someone else before it
/// believes a dm is running. The native host's probe holds it for the
/// moment between taking and letting go; a dm starting in that moment
/// would otherwise exit, believing another was running when none was.
const CLAIM_FOR: Duration = Duration::from_millis(500);

/// Takes the lock for this process: `Ok(Some)` with it, `Ok(None)` when
/// another dm has it, and an error when whether one has it cannot be told.
///
/// The `File` is the lock: it is held for as long as the handle lives, and
/// released by the OS when the process ends however it ends — a crash
/// leaves nothing stale to clean up, which a pid file would.
///
/// A lock file that cannot be opened or locked is an error, not "another dm
/// has it", which would have dm exit without a word every time it started.
pub fn claim(home: &Path) -> std::io::Result<Option<File>> {
    let deadline = std::time::Instant::now() + CLAIM_FOR;
    loop {
        match try_claim(home)? {
            // dm itself, starting: its home is made its account's alone.
            // Not on the browser's "is it running?" probe, which should
            // change nothing.
            Some(file) => {
                private(home);
                return Ok(Some(file));
            }
            None if std::time::Instant::now() >= deadline => return Ok(None),
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

/// One attempt: held by someone else is `Ok(None)`, anything else that
/// stops it is the error.
fn try_claim(home: &Path) -> std::io::Result<Option<File>> {
    let file = open(home)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(std::io::Error::new(
            e.kind(),
            format!("{} could not be locked: {e}", lock_path(home).display()),
        )),
    }
}

/// dm's home readable by this account alone. Settings, the queue and the
/// token are written 0600 already, but unfinished parts are not, and a home
/// made with the default umask (0755 on Linux) let any user on the computer
/// list and read downloads in progress. On Windows the folder gets an access
/// list of its own (`owner_only.rs`): a home on a second drive took the drive's,
/// which lets every signed-in account read and change it.
fn private(home: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(home, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(windows)]
    let _ = owner_only::make(home);
    #[cfg(not(any(unix, windows)))]
    let _ = home;
}

fn open(home: &Path) -> std::io::Result<File> {
    let path = lock_path(home);
    let _ = std::fs::create_dir_all(home);
    let written = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path);
    match written {
        Ok(file) => Ok(file),
        // One left by a dm run as another user (`sudo`) still locks when
        // opened to read: the lock is advisory, and nothing is written.
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            File::open(&path).map_err(|_| {
                std::io::Error::new(
                    e.kind(),
                    format!("{} could not be opened: {e}", path.display()),
                )
            })
        }
        Err(e) => Err(std::io::Error::new(
            e.kind(),
            format!("{} could not be opened: {e}", path.display()),
        )),
    }
}

/// Whether a dm is running: whether the lock is held by someone else, and
/// an error when that cannot be told — which is not "running": a caller
/// that believed it would hand a download to nobody.
///
/// Taking and immediately releasing the lock is a probe, not a claim — the
/// caller who wants dm to keep running should already be dm, and a dm
/// starting in the moment it is held waits it out (`CLAIM_FOR`).
pub fn running(home: &Path) -> std::io::Result<bool> {
    try_claim(home).map(|claimed| claimed.is_none())
}

#[cfg(test)]
mod tests;
