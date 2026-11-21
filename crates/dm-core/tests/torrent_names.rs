// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A torrent whose file is named with a drive (`C:x`) on Windows: the torrent
//! library joined the name onto the download folder with `PathBuf::push`, which
//! starts again on that drive, and made the empty file there before dm's own
//! check refused the torrent (gaps row 159 b). Nothing may be made outside the
//! folder.

#![cfg(windows)]

mod common;
use common::{options, tempdir};
use std::time::Duration;

/// A bencoded torrent of one file in a folder, named `file`, five bytes long.
fn torrent_naming(file: &str) -> Vec<u8> {
    let mut info = Vec::new();
    info.extend_from_slice(b"d5:filesld6:lengthi5e4:pathl");
    info.extend_from_slice(format!("{}:{}", file.len(), file).as_bytes());
    info.extend_from_slice(b"eee4:name4:evil12:piece lengthi16384e6:pieces20:");
    info.extend_from_slice(&[7u8; 20]);
    info.push(b'e');
    let mut whole = b"d4:info".to_vec();
    whole.extend_from_slice(&info);
    whole.push(b'e');
    whole
}

#[tokio::test]
async fn a_torrent_naming_a_drive_makes_no_file_on_it() {
    // Named on the current directory's own drive, so a run anywhere has the drive: `C:x` is
    // `x` in drive C:'s current directory, which for the current drive is this process's.
    let here = std::env::current_dir().unwrap();
    let drive = here.to_string_lossy().chars().next().unwrap();
    let name = format!("dm_escape_{}.txt", std::process::id());
    let escaped = here.join(&name);
    let _ = std::fs::remove_file(&escaped);

    let dir = tempdir();
    let torrent = dir.path().join("evil.torrent");
    std::fs::write(&torrent, torrent_naming(&format!("{drive}:{name}"))).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let url = dm_core::file_url(&torrent).unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        dm_core::download_torrent(&dm_core::client(), &url, &options(&out), None),
    )
    .await;
    let made_outside = escaped.exists();
    let _ = std::fs::remove_file(&escaped);
    assert!(
        !made_outside,
        "the torrent made {} outside its folder ({outcome:?})",
        escaped.display()
    );
    assert!(
        matches!(outcome, Ok(Err(_))),
        "a torrent naming another drive was not refused: {outcome:?}"
    );
}
