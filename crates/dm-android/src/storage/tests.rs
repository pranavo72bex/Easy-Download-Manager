// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where a download lands on the phone and where a finished one goes, decided on the host.

use super::*;

/// Android 10 and up go through the media store; 8 and 9 by path.
#[test]
fn the_way_is_the_phones() {
    assert_eq!(way(26), Way::ByPath);
    assert_eq!(way(28), Way::ByPath);
    assert_eq!(way(29), Way::MediaStore);
    assert_eq!(way(35), Way::MediaStore);
}

/// On Android 10 and up downloads go to dm's own folder alone, and a file dm writes to the
/// shared one where dm could write there; on 8 and 9 the shared one first.
#[test]
fn new_downloads_go_to_dms_own_folder_from_android_10() {
    let own = Some(PathBuf::from("/e/files/Download/dm"));
    let shared = Some(PathBuf::from("/s/Download/dm"));
    let ten = places(29, shared.clone(), own.clone());
    assert_eq!((ten.shared.as_ref(), ten.first()), (None, own.as_ref()));
    assert_eq!(ten.save, shared);
    let nine = places(28, shared.clone(), own.clone());
    assert_eq!(nine.first(), shared.as_ref());
    assert_eq!(nine.save, shared);
    // Android 9 before storage is allowed, and 13 with the shared folder not writable.
    assert_eq!(places(28, None, own.clone()).first(), own.as_ref());
    let thirteen = places(33, None, own.clone());
    assert_eq!(
        (thirteen.first(), thirteen.save.as_ref()),
        (own.as_ref(), own.as_ref())
    );
    assert_eq!(places(33, None, None), Places::default());
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

/// A second file of a name is numbered before its extension, as the system does.
#[test]
fn a_taken_name_is_numbered() {
    assert_eq!(numbered("a.zip", 1), "a (1).zip");
    assert_eq!(numbered("a.tar.gz", 2), "a.tar (2).gz");
    assert_eq!(numbered("README", 1), "README (1)");
    assert_eq!(numbered(".hidden", 1), ".hidden (1)");
}

/// Moved by path: the bytes whole in the shared folder, under a free name, dm's copy gone,
/// and a file already there untouched.
#[test]
fn a_file_is_moved_whole_and_never_over_another() {
    let root = tempfile::tempdir().expect("a folder for the test");
    let own = root.path().join("own");
    std::fs::create_dir_all(&own).expect("dm's folder");
    let shared = root.path().join("Download").join("dm");
    std::fs::create_dir_all(&shared).expect("the shared folder");
    std::fs::write(shared.join("a.bin"), b"already").expect("a file already there");
    let file = own.join("a.bin");
    let bytes: Vec<u8> = (0..3_000_000u32).map(|n| (n % 251) as u8).collect();
    std::fs::write(&file, &bytes).expect("the finished file");
    let now = move_into(&file, &shared, "a.bin").expect("moved");
    assert_eq!(now, shared.join("a (1).bin"));
    assert_eq!(std::fs::read(&now).expect("read back"), bytes);
    assert_eq!(
        std::fs::read(shared.join("a.bin")).expect("kept"),
        b"already"
    );
    assert!(!file.exists());
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
