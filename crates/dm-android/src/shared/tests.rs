// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn shared_text_and_opened_addresses_are_told_apart() {
    assert_eq!(
        handed(
            Some(SEND),
            Some("  A film https://example.com/a.mp4 "),
            None
        ),
        Handed::Text("A film https://example.com/a.mp4".into())
    );
    assert_eq!(handed(Some(SEND), Some("  "), None), Handed::Nothing);
    assert_eq!(
        handed(Some(VIEW), None, Some("magnet:?xt=urn:btih:abc")),
        Handed::Text("magnet:?xt=urn:btih:abc".into())
    );
    assert_eq!(
        handed(Some(VIEW), None, Some("content://downloads/1")),
        Handed::File("content://downloads/1".into())
    );
    // Not a `file://` address, nor a provider of dm's own (gaps row H16).
    for refused in [
        "FILE:///sdcard/a.torrent",
        "file:///data/data/com.dm.android/files/settings.json",
        "content://com.dm.android.files/home/settings.json",
        "content://COM.DM.ANDROID/x.torrent",
        "content://evil@com.dm.android.provider/x.torrent",
    ] {
        assert_eq!(
            handed(Some(VIEW), None, Some(refused)),
            Handed::Nothing,
            "{refused}"
        );
    }
    assert_eq!(
        handed(
            Some(VIEW),
            None,
            Some("content://com.dm.androidx.files/a.torrent")
        ),
        Handed::File("content://com.dm.androidx.files/a.torrent".into())
    );
    assert_eq!(handed(Some(VIEW), None, Some("tel:123")), Handed::Nothing);
    assert_eq!(
        handed(Some("android.intent.action.MAIN"), None, None),
        Handed::Nothing
    );
    assert_eq!(handed(None, None, None), Handed::Nothing);
}

#[test]
fn a_torrent_is_kept_under_a_safe_name() {
    assert_eq!(
        torrent_name(Some("Big Buck Bunny.torrent"), None),
        "Big Buck Bunny.torrent"
    );
    assert_eq!(
        torrent_name(None, Some("primary:Download/linux.iso.TORRENT")),
        "linux.iso.TORRENT"
    );
    assert_eq!(torrent_name(Some("a/b\\c?d"), None), "c_d.torrent");
    assert_eq!(torrent_name(Some(" .. "), None), "shared.torrent");
    assert_eq!(torrent_name(None, None), "shared.torrent");
    assert!(torrent_name(Some(&"x".repeat(500)), None).len() <= 128);
}

#[test]
fn a_shared_torrent_never_replaces_one_already_kept() {
    let dir = tempfile::tempdir().unwrap();
    let first = keep_torrent(dir.path(), "ubuntu.torrent", b"the person's").unwrap();
    let second = keep_torrent(dir.path(), "ubuntu.torrent", b"another app's").unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read(&first).unwrap(), b"the person's");
    assert_eq!(std::fs::read(&second).unwrap(), b"another app's");
    assert_eq!(second.file_name().unwrap(), "ubuntu (1).torrent");
}

/// A copy that never ends is given up after its wait, and the next share, waiting behind the
/// same lock, is then taken (gaps row H37a): a sharing app whose stream never ended held the
/// share lock for good.
#[test]
fn a_copy_that_never_ends_is_given_up_and_the_next_share_is_taken() {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    let lock = Arc::new(Mutex::new(()));
    let wait = Duration::from_millis(200);
    let began = Instant::now();
    let first = {
        let _held = lock.lock().unwrap();
        within(wait, "the copy did not finish", || {
            std::thread::sleep(Duration::from_secs(60));
            Ok(())
        })
    };
    assert_eq!(first, Err("the copy did not finish".to_string()));
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "{:?}",
        began.elapsed()
    );
    let second = {
        let _held = lock.lock().unwrap();
        within(wait, "the copy did not finish", || Ok(7))
    };
    assert_eq!(second, Ok(7));
}
