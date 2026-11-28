// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Tests: a full disk said, a folder copied whole, and renames to no name or of case alone.

use super::*;

#[test]
fn a_full_disk_or_a_folder_that_cannot_be_reached_is_said_and_nothing_else_is() {
    let dir = tempfile::tempdir().unwrap();
    let here = dir.path();
    let full = Error::Disk(std::io::Error::from(std::io::ErrorKind::StorageFull));
    assert!(out_of_room(&full, [here, here]).is_some());
    let enospc = Error::Disk(std::io::Error::from_raw_os_error(28));
    assert!(out_of_room(&enospc, [here, here]).is_some());
    let said = Error::known(
        crate::FailKind::NoRoom,
        "the download folder cannot be written to",
    );
    assert_eq!(
        out_of_room(&said, [here, here]).as_deref(),
        Some("the download folder cannot be written to")
    );
    // Another failure, on folders that are there: not a matter of room.
    assert_eq!(
        out_of_room(&Error::Internal("the server said no".into()), [here, here]),
        None
    );
    let other = Error::Disk(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert_eq!(out_of_room(&other, [here, here]), None);
    // A folder under a file can never be made: its drive is said to be gone.
    let file = here.join("a file");
    std::fs::write(&file, b"x").unwrap();
    let under = file.join("downloads");
    let why = out_of_room(&other, [std::path::Path::new(""), &under]).unwrap();
    assert!(
        why.contains("cannot be reached") && why.contains("downloads"),
        "{why}"
    );
}

#[tokio::test]
async fn a_folder_is_copied_whole_with_what_is_inside_it() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("from");
    std::fs::create_dir_all(from.join("a/b")).unwrap();
    std::fs::write(from.join("one"), b"1").unwrap();
    std::fs::write(from.join("a/b/two"), b"2").unwrap();
    let to = dir.path().join("elsewhere/to");
    copy_dir(&from, &to).await.unwrap();
    assert_eq!(std::fs::read(to.join("one")).unwrap(), b"1");
    assert_eq!(std::fs::read(to.join("a/b/two")).unwrap(), b"2");
    assert!(from.join("one").exists(), "a copy leaves the original");
    // A move takes it away from where it was.
    let moved = dir.path().join("moved");
    move_dir(&from, &moved).await.unwrap();
    assert!(!from.exists());
    assert_eq!(std::fs::read(moved.join("a/b/two")).unwrap(), b"2");
    assert!(copy_dir(&dir.path().join("missing"), &dir.path().join("x"))
        .await
        .is_err());
}

#[tokio::test]
async fn a_rename_to_no_name_is_refused_and_one_of_case_only_is_done_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Report.pdf");
    std::fs::write(&file, b"r").unwrap();
    let why = rename_free(&file, "").await.unwrap_err();
    assert_eq!(why.kind(), std::io::ErrorKind::InvalidInput);
    // Only the case changes: the same file under the new spelling, never "report (1).pdf".
    let to = rename_free(&file, "report.pdf").await.unwrap();
    assert_eq!(to, dir.path().join("report.pdf"));
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["report.pdf"]);
    assert_eq!(std::fs::read(&to).unwrap(), b"r");
    // Another name entirely: not a case-only rename.
    assert_eq!(
        rename_case_only(&to, dir.path(), "other.pdf")
            .await
            .unwrap(),
        None
    );
}
