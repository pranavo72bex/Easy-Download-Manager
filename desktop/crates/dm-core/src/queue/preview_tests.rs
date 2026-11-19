// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Tests: the preview of a download being fetched, and the bytes staged.

use super::*;

#[test]
fn a_preview_is_what_is_fetched_under_the_files_own_ending() {
    let dir = tempfile::tempdir().unwrap();
    let parts = dir.path();
    assert!(preview_of(parts, "talk.mp4").is_err(), "nothing yet");

    std::fs::write(parts.join(crate::download::STAGED), b"first bytes").unwrap();
    let preview = preview_of(parts, "talk.mp4").unwrap();
    assert_eq!(preview, parts.join("preview.mp4"));
    assert_eq!(std::fs::read(&preview).unwrap(), b"first bytes");

    // A link grows with the download; asked again, it is made afresh.
    std::fs::write(parts.join(crate::download::STAGED), b"first bytes, more").unwrap();
    let again = preview_of(parts, "talk.mp4").unwrap();
    assert_eq!(std::fs::read(again).unwrap(), b"first bytes, more");
}

/// A download going to another disk has a file a range (gaps row 166): the first holds the
/// file's beginning, which is what a player needs; the others are not the file's start.
#[test]
fn the_preview_of_a_download_with_a_file_a_range_is_its_first_range() {
    let dir = tempfile::tempdir().unwrap();
    let parts = dir.path();
    std::fs::write(crate::download::range_path(parts, 1), b"the middle").unwrap();
    assert!(
        preview_of(parts, "talk.mp4").is_err(),
        "not the file's start"
    );
    std::fs::write(crate::download::range_path(parts, 0), b"the start").unwrap();
    let preview = preview_of(parts, "talk.mp4").unwrap();
    assert_eq!(std::fs::read(preview).unwrap(), b"the start");
}

/// What an earlier run left is counted once: by each range's ledger, not again by the length
/// of the range's own file.
#[tokio::test]
async fn a_range_file_is_counted_by_its_ledger_alone() {
    let dir = tempfile::tempdir().unwrap();
    let parts = dir.path();
    std::fs::write(crate::download::range_path(parts, 0), vec![1u8; 5000]).unwrap();
    std::fs::write(parts.join("done-0000"), format!("{:020}", 4096)).unwrap();
    std::fs::write(parts.join("plan.json"), b"{}").unwrap();
    assert_eq!(staged_bytes(parts).await, Some(4096));
}
