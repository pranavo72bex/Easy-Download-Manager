//! One dm at a time.
//!
//! Every browser hand-off used to start a second copy of the app: the host
//! process spawned the desktop on its first request without asking whether
//! one was already running, and nothing stopped the second from opening a
//! second window, a second tray icon and a second queue writing the same
//! `queue.json` as the first. An advisory lock on one file under dm's home
//! is the whole mechanism: the running app holds it for its lifetime, and
//! anything that wants to know whether dm is running tries to take it.

use std::fs::File;
use std::path::Path;

/// The file whose lock says dm is running.
fn lock_path(home: &Path) -> std::path::PathBuf {
    home.join("instance.lock")
}

/// Takes the lock for this process, or says another has it.
///
/// The `File` is the lock: it is held for as long as the handle lives, and
/// released by the OS when the process ends however it ends — a crash
/// leaves nothing stale to clean up, which a pid file would.
pub fn claim(home: &Path) -> Option<File> {
    let _ = std::fs::create_dir_all(home);
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(home))
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Whether a dm is running: whether the lock is held by someone else.
///
/// Taking and immediately releasing the lock is a probe, not a claim — the
/// caller who wants dm to keep running should already be dm.
pub fn running(home: &Path) -> bool {
    claim(home).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_is_held_for_as_long_as_the_handle_lives() {
        let home = tempfile::tempdir().unwrap();
        assert!(!running(home.path()), "nothing holds a fresh lock");
        let held = claim(home.path()).expect("the first claim succeeds");
        assert!(running(home.path()), "a held lock says dm is running");
        drop(held);
        assert!(!running(home.path()), "a dropped lock is released");
    }
}
