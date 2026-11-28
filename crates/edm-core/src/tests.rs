// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#[cfg(unix)]
#[test]
fn dms_files_are_readable_by_this_account_alone() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("edm-mode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("queue.json");
    super::write_atomic(&path, b"[]").unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(mode, 0o600, "{mode:o}");
}

#[test]
fn a_windows_long_path_reads_as_a_plain_one() {
    use std::path::PathBuf;
    assert_eq!(
        super::plain_path(PathBuf::from(r"\\?\C:\Program Files\EDM\EDM.exe")),
        PathBuf::from(r"C:\Program Files\EDM\EDM.exe")
    );
    assert_eq!(
        super::plain_path(PathBuf::from(r"\\?\UNC\server\share\EDM.exe")),
        PathBuf::from(r"\\?\UNC\server\share\EDM.exe"),
        "a share keeps its form"
    );
    assert_eq!(
        super::plain_path(PathBuf::from("/usr/bin/EDM")),
        PathBuf::from("/usr/bin/EDM")
    );
}

#[test]
fn writes_to_one_file_at_once_all_succeed() {
    let dir = std::env::temp_dir().join(format!("edm-atomic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("queue.json");
    let writers: Vec<_> = (0..8)
        .map(|i| {
            let path = path.clone();
            std::thread::spawn(move || {
                for _ in 0..200 {
                    super::write_atomic(&path, format!("[{i}]").as_bytes()).unwrap();
                }
            })
        })
        .collect();
    for w in writers {
        w.join().expect("a write failed");
    }
    let left = std::fs::read_dir(&dir).unwrap().count();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(left, 1, "temporary files were left beside the file");
}
