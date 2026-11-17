// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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

#[cfg(windows)]
#[test]
fn dms_home_is_this_accounts_alone_on_windows_too() {
    use owner_only::by_hand::{access_of, open_to};
    // A parent every account may read (`BU`: users), as a second drive's root is.
    let parent = tempfile::tempdir().unwrap();
    let me = owner_only::account().unwrap();
    open_to(
        parent.path(),
        &format!("D:(A;OICI;FA;;;{me})(A;OICI;0x1200a9;;;BU)"),
    )
    .unwrap();
    let home = parent.path().join("dm");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("queue.json"), "[]").unwrap();
    let before = access_of(&home).unwrap();
    assert!(
        before.contains(";BU)"),
        "the test's parent did not let users in: {before}"
    );
    let _held = claim(&home).unwrap();
    let made_after = home.join("control.token");
    std::fs::write(&made_after, "x").unwrap();
    for path in [home.clone(), home.join("queue.json"), made_after] {
        let access = access_of(&path).unwrap();
        assert!(
            access.contains("D:P") || path != home,
            "{} still takes its parent's list: {access}",
            path.display()
        );
        for other in [";BU)", ";AU)", ";WD)"] {
            assert!(
                !access.contains(other),
                "{} lets {other} in: {access}",
                path.display()
            );
        }
    }
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
    // A process another test starts at that moment is forked holding
    // the lock's file until it runs its program, which lets go of it:
    // released within moments, not at once.
    let released = (0..50).any(|_| {
        let free = !running(home.path()).unwrap();
        if !free {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        free
    });
    assert!(released, "a dropped lock is released");
}

/// A lock file left by a dm run as another account, readable but not
/// writable here, still locks; one not even readable is said, with its
/// path, rather than taken for a running dm.
#[cfg(unix)]
#[test]
fn a_lock_file_only_readable_here_still_locks_and_an_unreadable_one_is_said() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let lock = lock_path(home.path());
    std::fs::write(&lock, b"").unwrap();
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o444)).unwrap();
    assert!(!running(home.path()).unwrap());
    let held = claim(home.path()).unwrap().expect("claimed read-only");
    assert!(running(home.path()).unwrap());
    drop(held);

    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&lock).is_err() {
        // Not an account that reads past permissions.
        let why = running(home.path()).unwrap_err();
        assert_eq!(why.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(why.to_string().contains("could not be opened"), "{why}");
    }
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
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
