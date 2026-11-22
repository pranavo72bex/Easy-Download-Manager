// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where a download lands on the phone and where a finished one goes, decided on the host.

use super::*;

/// Downloads go to dm's own folder alone, and a file dm writes for the person to the shared
/// one where dm could write there, else to dm's own.
#[test]
fn new_downloads_go_to_dms_own_folder() {
    let own = Some(PathBuf::from("/e/files/Download/dm"));
    let shared = Some(PathBuf::from("/s/Download/dm"));
    let both = places(shared.clone(), own.clone());
    assert_eq!(both.first(), own.as_ref());
    assert_eq!(both.save, shared);
    // The shared folder not writable.
    let own_only = places(None, own.clone());
    assert_eq!(
        (own_only.first(), own_only.save.as_ref()),
        (own.as_ref(), own.as_ref())
    );
    assert_eq!(places(None, None), Places::default());
    assert_eq!(
        own_downloads(Path::new("/e/files")),
        PathBuf::from("/e/files/Download/dm")
    );
}

/// A file in dm's own folders goes to `Download/dm`, its sub-folders kept; one anywhere
/// else, or a path out of the folder, stays.
#[test]
fn a_file_in_dms_own_folder_has_its_place_in_download() {
    let private = private_folders(Some(Path::new("/e/files")), Path::new("/i/dm"));
    let place = shared_place(Path::new("/e/files/Download/dm/a.zip"), &private);
    assert_eq!(
        place,
        Some(Shared {
            sub: vec![],
            name: "a.zip".into()
        })
    );
    assert_eq!(
        place.map(|p| p.relative_path()).as_deref(),
        Some("Download/dm")
    );
    let video = shared_place(Path::new("/e/files/Download/dm/Video/b.mp4"), &private)
        .expect("in dm's folder");
    assert_eq!(video.relative_path(), "Download/dm/Video");
    assert_eq!(
        video.folder_in(Path::new("/s/Download")),
        PathBuf::from("/s/Download/dm/Video")
    );
    // The folder earlier starts chose, and dm's home's.
    assert_eq!(
        shared_place(Path::new("/e/files/Download/c.txt"), &private).map(|p| p.name),
        Some("c.txt".into())
    );
    assert!(shared_place(Path::new("/i/dm/Download/d.txt"), &private).is_some());
    // Elsewhere: left.
    assert_eq!(
        shared_place(Path::new("/s/Download/dm/a.zip"), &private),
        None
    );
    assert_eq!(
        shared_place(Path::new("/e/files/torrents/x"), &private),
        None
    );
    assert_eq!(
        shared_place(Path::new("/e/files/Download/dm"), &private),
        None
    );
    assert_eq!(
        shared_place(Path::new("/e/files/Download/dm/../../x"), &private),
        None
    );
}

/// The chunked copy carries every byte of a file larger than its buffer.
#[test]
fn the_copy_carries_every_byte() {
    let root = tempfile::tempdir().expect("a folder for the test");
    let from = root.path().join("from");
    let bytes: Vec<u8> = (0..2_500_000u32).map(|n| (n % 253) as u8).collect();
    std::fs::write(&from, &bytes).expect("the source");
    let to = root.path().join("to");
    let mut out = File::create(&to).expect("the copy");
    assert_eq!(stream(&from, &mut out).expect("copied"), bytes.len() as u64);
    drop(out);
    assert_eq!(std::fs::read(&to).expect("read back"), bytes);
}
