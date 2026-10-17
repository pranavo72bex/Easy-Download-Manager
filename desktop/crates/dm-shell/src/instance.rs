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
use std::time::Duration;

/// The file whose lock says dm is running.
fn lock_path(home: &Path) -> std::path::PathBuf {
    home.join("instance.lock")
}

/// How long a claim keeps trying a lock held by someone else before it
/// believes a dm is running. The native host's probe holds it for the
/// moment between taking and letting go; a dm starting in that moment
/// used to exit, believing another was running, and none was.
const CLAIM_FOR: Duration = Duration::from_millis(500);

/// Takes the lock for this process: `Ok(Some)` with it, `Ok(None)` when
/// another dm has it, and an error when whether one has it cannot be told.
///
/// The `File` is the lock: it is held for as long as the handle lives, and
/// released by the OS when the process ends however it ends — a crash
/// leaves nothing stale to clean up, which a pid file would.
///
/// A lock file that cannot be opened or locked used to read as "another dm
/// has it", and dm then exited without a word, every time it was started.
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
/// list and read downloads in progress.
fn private(home: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(home, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
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
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn dms_home_is_this_accounts_alone() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let home = parent.path().join("dm");
        let _held = claim(&home).unwrap();
        let mode = std::fs::metadata(&home).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{mode:o}");
    }

    #[test]
    fn the_lock_is_held_for_as_long_as_the_handle_lives() {
        let home = tempfile::tempdir().unwrap();
        assert!(!running(home.path()).unwrap(), "nothing holds a fresh lock");
        let held = claim(home.path())
            .unwrap()
            .expect("the first claim succeeds");
        assert!(
            running(home.path()).unwrap(),
            "a held lock says dm is running"
        );
        drop(held);
        assert!(!running(home.path()).unwrap(), "a dropped lock is released");
    }

    #[test]
    fn a_lock_that_cannot_be_opened_is_an_error_not_a_running_dm() {
        let home = tempfile::tempdir().unwrap();
        // Something that is not a file where the lock goes.
        std::fs::create_dir_all(home.path().join("instance.lock")).unwrap();
        assert!(claim(home.path()).is_err(), "read as another dm running");
        assert!(running(home.path()).is_err(), "read as a dm running");
    }

    #[test]
    fn a_dm_starting_while_the_host_probes_waits_the_probe_out() {
        /* The host's probe takes the lock and lets it go. A claim landing
         * in between used to give up at once, and that dm exited believing
         * another was running. Held here for a little longer than a probe
         * would, which a claim must outlast. */
        let home = tempfile::tempdir().unwrap();
        let probe = claim(home.path()).unwrap().expect("taken");
        let released = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(probe);
        });
        assert!(
            claim(home.path()).unwrap().is_some(),
            "a passing probe was taken for a running dm"
        );
        released.join().unwrap();
    }
}
