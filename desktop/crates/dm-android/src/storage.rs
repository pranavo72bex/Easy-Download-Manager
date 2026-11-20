// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where a download lands on the phone, and where it goes once finished (gaps row N9), in the
//! plain decisions `android/storage.rs` makes over JNI, tested on the host.
//!
//! Android 10 (API 29) and up let no app write the shared Download folder by path. So dm
//! downloads into the app's own folder on the shared storage
//! (`getExternalFilesDir(DIRECTORY_DOWNLOADS)/dm`), which it writes by path: segmented writes,
//! resume and every file the engine handles stay as they are. A file finished there is then
//! put in `Download/dm` (its sub-folder kept) through the media store, and dm's own copy
//! removed. On Android 8 and 9 (API 26 to 28) the shared folder is written by path, with the
//! storage permission, as before; a file that was finished in dm's own folder there (before the
//! permission was granted) is moved across by path. A torrent's files stay where they are
//! (`dm_core`'s queue never hands them over): its seeding reads them there.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

/// The folder, under the shared storage, that dm's finished files are put in.
pub(crate) const SHARED_FOLDER: &str = "Download/dm";

/// The first Android (API level) with the media store's Downloads (Android 10), and with no
/// writing to the shared Download folder by path.
pub(crate) const MEDIA_STORE_FROM: i32 = 29;

/// How a finished file reaches the shared Download folder on a phone of this API level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Way {
    /// `MediaStore.Downloads`: inserted pending, written through a file descriptor, then
    /// shown (Android 10 and up).
    MediaStore,
    /// Moved by path into the shared folder, with the storage permission (Android 8 and 9).
    ByPath,
}

/// The way for a phone of API level `sdk`.
pub(crate) fn way(sdk: i32) -> Way {
    if sdk >= MEDIA_STORE_FROM {
        Way::MediaStore
    } else {
        Way::ByPath
    }
}

/// dm's own download folder on the shared storage: `getExternalFilesDir(DIRECTORY_DOWNLOADS)`
/// (`external` is `getExternalFilesDir(null)`) and `dm` in it.
pub(crate) fn own_downloads(external: &Path) -> PathBuf {
    external.join("Download").join("dm")
}

/// The folders dm downloads into that only dm sees, most particular first: the one above, the
/// app's folder that earlier starts chose on Android 10 (`files/Download`), and the one in dm's
/// `home` where the phone has no shared storage. A file finished in one of them is published.
pub(crate) fn private_folders(external: Option<&Path>, home: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(external) = external {
        folders.push(own_downloads(external));
        folders.push(external.join("Download"));
    }
    folders.push(home.join("Download"));
    folders
}

/// Where new downloads go on the first start: on Android 10 and up dm's own folder
/// (`own`, where it could be made and written), as the shared one cannot be written by path
/// there; on 8 and 9, or without one, the folder the app found (`found`: the shared
/// `Download/dm` where the permission lets dm write it).
pub(crate) fn download_folder(sdk: i32, own: Option<PathBuf>, found: PathBuf) -> PathBuf {
    match (way(sdk), own) {
        (Way::MediaStore, Some(own)) => own,
        _ => found,
    }
}

/// Where in the shared storage a finished file goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shared {
    /// The folders under dm's own download folder it was in (a category's, say), kept.
    pub(crate) sub: Vec<String>,
    /// Its name.
    pub(crate) name: String,
}

impl Shared {
    /// The media store's `RELATIVE_PATH`: `Download/dm`, and its sub-folders.
    pub(crate) fn relative_path(&self) -> String {
        std::iter::once(SHARED_FOLDER)
            .chain(self.sub.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("/")
    }

    /// The folder it goes in under the shared Download folder `public` (by path).
    pub(crate) fn folder_in(&self, public: &Path) -> PathBuf {
        self.sub
            .iter()
            .fold(public.join("dm"), |folder, sub| folder.join(sub))
    }
}

/// Where the finished file at `file` goes in the shared storage, if it is in one of dm's
/// `private` folders; `None` for a file anywhere else (the shared folder itself, a folder of the
/// person's own), which is left where it is.
pub(crate) fn shared_place(file: &Path, private: &[PathBuf]) -> Option<Shared> {
    let rest = private
        .iter()
        .find_map(|folder| file.strip_prefix(folder).ok())?;
    let mut parts = Vec::new();
    for part in rest.components() {
        match part {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            // `..` or a root: not a path dm made, never followed out of the folder.
            _ => return None,
        }
    }
    let name = parts.pop().filter(|name| !name.is_empty())?;
    Some(Shared { sub: parts, name })
}

/// `name` numbered as the system numbers a second file of the same name: `a (1).zip`.
fn numbered(name: &str, n: u32) -> String {
    match name.rfind('.').filter(|&dot| dot > 0) {
        Some(dot) => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        None => format!("{name} ({n})"),
    }
}

/// Copies every byte of `from` into `to` in chunks, so a file of any size is copied with a
/// buffer's memory, and puts them on the disk.
pub(crate) fn stream(from: &Path, to: &mut File) -> io::Result<u64> {
    const CHUNK: usize = 1024 * 1024;
    let mut source = File::open(from)?;
    let mut buffer = vec![0u8; CHUNK];
    let mut copied = 0u64;
    loop {
        let read = match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        to.write_all(&buffer[..read])?;
        copied += read as u64;
    }
    to.sync_all()?;
    Ok(copied)
}

/// Moves the finished `file` into `folder` (made where missing) under `name`, or a numbered
/// one where that is taken (never over a file already there): renamed where both are on one
/// disk, else copied and dm's copy removed. Where it now is.
pub(crate) fn move_into(file: &Path, folder: &Path, name: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let mut n = 0;
    let (target, mut reserved) = loop {
        let candidate = folder.join(if n == 0 {
            name.to_owned()
        } else {
            numbered(name, n)
        });
        match File::options()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(reserved) => break (candidate, reserved),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 10_000 => n += 1,
            Err(e) => return Err(e),
        }
    };
    // The name is held by the empty file made above; a rename takes its place.
    if std::fs::rename(file, &target).is_ok() {
        return Ok(target);
    }
    if let Err(e) = stream(file, &mut reserved) {
        drop(reserved);
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    drop(reserved);
    // Published whole; dm's copy is a spare one now, and one it could not remove is left.
    let _ = std::fs::remove_file(file);
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Android 10 and up go through the media store; 8 and 9 by path.
    #[test]
    fn the_way_is_the_phones() {
        assert_eq!(way(26), Way::ByPath);
        assert_eq!(way(28), Way::ByPath);
        assert_eq!(way(29), Way::MediaStore);
        assert_eq!(way(35), Way::MediaStore);
    }

    /// On Android 10 and up dm's own folder, where there is one; else what the app found.
    #[test]
    fn new_downloads_go_to_dms_own_folder_from_android_10() {
        let own = PathBuf::from("/e/files/Download/dm");
        let found = PathBuf::from("/s/Download/dm");
        assert_eq!(download_folder(29, Some(own.clone()), found.clone()), own);
        assert_eq!(download_folder(28, Some(own), found.clone()), found);
        assert_eq!(download_folder(33, None, found.clone()), found);
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
}
