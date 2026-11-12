// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#[cfg(windows)]
#[test]
fn a_staged_file_takes_its_room_only_as_bytes_land() {
    use std::io::{Seek, SeekFrom, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("staged");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    super::sparse(&file);
    // As `preallocate` makes it: a GiB long, then written at its far end.
    let long = 1u64 << 30;
    file.set_len(long).unwrap();
    let mut writing = &file;
    writing.seek(SeekFrom::Start(long - 65_536)).unwrap();
    writing.write_all(&[7u8; 65_536]).unwrap();
    file.sync_all().unwrap();
    const SPARSE: u32 = 0x200; // FILE_ATTRIBUTE_SPARSE_FILE
    let attributes = std::fs::metadata(&path).unwrap().file_attributes();
    assert!(
        attributes & SPARSE != 0,
        "not sparse: attributes {attributes:#x}"
    );
    // What it takes on disk, not how long it says it is.
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut high = 0u32;
    // SAFETY: `wide` ends in NUL; `high` receives the size's upper half.
    let low = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetCompressedFileSizeW(wide.as_ptr(), &mut high)
    };
    let taken = (u64::from(high) << 32) | u64::from(low);
    assert!(
        taken < 16 << 20,
        "a GiB staged file with 64 KB written takes {taken} bytes on disk"
    );
    drop(file);
}

/// A resume goes on in the layout its bytes are in, whatever the disks: an earlier run's staged
/// file or part files keep the one staged file, and its range files keep a file a range (gaps row
/// 166).
#[test]
fn a_resume_keeps_the_layout_its_bytes_are_in() {
    let dir = tempfile::tempdir().unwrap();
    let (parts, dest) = (dir.path().join("parts"), dir.path().join("out"));
    std::fs::create_dir_all(&parts).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(super::range_path(&parts, 0), b"bytes").unwrap();
    assert!(
        super::stage_by_range(&parts, &dest, true),
        "its range files"
    );
    assert!(
        !super::stage_by_range(&parts, &dest, false),
        "not for a download that is not fetched by ranges"
    );
    for earlier in [super::STAGED, "part-0000"] {
        std::fs::write(parts.join(earlier), b"bytes").unwrap();
        assert!(
            !super::stage_by_range(&parts, &dest, true),
            "{earlier} beside them keeps the staged file"
        );
        std::fs::remove_file(parts.join(earlier)).unwrap();
    }
}

/// With nothing staged, a file a range only where the download folder is on another disk.
#[test]
fn a_file_a_range_only_where_the_folder_is_on_another_disk() {
    if std::env::var_os("DM_STAGE_BY_RANGE").is_some() {
        return; // the switch decides instead, as it is meant to
    }
    let dir = tempfile::tempdir().unwrap();
    let (parts, dest) = (dir.path().join("parts"), dir.path().join("out"));
    std::fs::create_dir_all(&parts).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    assert!(!super::stage_by_range(&parts, &dest, true), "one disk");
    #[cfg(windows)]
    {
        // Another drive letter than the temporary folder's, as `same_disk` tells disks apart.
        let here = dir.path().to_string_lossy().to_ascii_uppercase();
        let other = if here.starts_with('Q') {
            r"R:\out"
        } else {
            r"Q:\out"
        };
        let other = std::path::Path::new(other);
        assert!(super::stage_by_range(&parts, other, true), "another disk");
        assert!(
            !super::stage_by_range(&parts, other, false),
            "another disk, but no ranges to stage"
        );
    }
}

/// A range's file name gives its index, as a part file's and a ledger's do: `planned_segments`
/// removes what is past the plan's last range by it.
#[test]
fn a_range_files_name_gives_its_index() {
    assert_eq!(super::staged_index("range-0012"), Some(12));
    assert_eq!(super::staged_index("done-0012"), Some(12));
    assert_eq!(super::staged_index("done-0012.synced"), Some(12));
    assert_eq!(super::staged_index("part-0012"), Some(12));
    assert_eq!(super::staged_index("staged"), None);
    assert_eq!(
        super::range_path(std::path::Path::new("p"), 3),
        std::path::Path::new("p").join("range-0003")
    );
}

/// A stopped connection's sync goes on without it; the download waits for it, and its mark, before
/// returning, so nothing comes into the parts after (`Stopping`).
#[tokio::test]
async fn a_stop_s_sync_is_waited_for_and_writes_its_mark() {
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let file = tokio::fs::File::create(dir.path().join("staged"))
        .await
        .unwrap();
    let stopping = Arc::new(super::Stopping::default());
    // A sync still on its thread when the download gets to its end.
    stopping
        .count
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    let held = super::Syncing(stopping.clone());
    let late = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(held);
    });
    let began = std::time::Instant::now();
    stopping.settled().await;
    assert!(
        began.elapsed() >= std::time::Duration::from_millis(190),
        "the download returned with a sync still running"
    );
    late.join().unwrap();
    // One begun and waited for leaves its mark and no count behind.
    super::sync_and_mark(&stopping, dir.path(), &file, 3, 4096).await;
    stopping.settled().await;
    assert_eq!(super::read_synced(dir.path(), 3), Some(4096));
}
