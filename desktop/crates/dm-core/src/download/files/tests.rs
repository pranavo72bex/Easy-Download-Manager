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
