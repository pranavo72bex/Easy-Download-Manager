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
//!
//! The folders dm writes by path ([`places`]): on Android 10 and up only dm's own is offered for
//! downloads (the shared one is reached through publishing), while a file dm writes for the
//! person (Export, Back up dm) goes to the shared `Download/dm` where dm can write a file there
//! (Android 11 and up let an app make its own files in it by path), else to dm's own.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::phone::Places;

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

/// The folders dm writes by path on a phone of API level `sdk`, of `shared` (the shared
/// `Download/dm`, where dm could write a file in it) and `own` (dm's own folder, where it
/// could): on Android 10 and up downloads go only to dm's own folder (each finished file is
/// published to the shared one), so the shared one is not offered for them; a file dm writes
/// for the person goes to the shared one where dm could write there, else to dm's own. On 8
/// and 9 both are offered, the shared one first (where the storage permission lets dm write
/// it).
pub(crate) fn places(sdk: i32, shared: Option<PathBuf>, own: Option<PathBuf>) -> Places {
    let save = shared.clone().or_else(|| own.clone());
    let shared = shared.filter(|_| way(sdk) == Way::ByPath);
    Places { shared, own, save }
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
mod tests;
