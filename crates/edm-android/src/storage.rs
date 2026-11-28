// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where a download lands on the phone, and where it goes once finished (gaps row N9), in the
//! plain decisions `android/storage.rs` makes over JNI, tested on the host.
//!
//! Android 10 (API 29) and up let no app write the shared Download folder by path. So EDM
//! downloads into the app's own folder on the shared storage
//! (`getExternalFilesDir(DIRECTORY_DOWNLOADS)/EDM`), which it writes by path: segmented writes,
//! resume and every file the engine handles stay as they are. A file finished there is then
//! put in `Download/EDM` (its sub-folder kept) through the media store, and EDM's own copy
//! removed. EDM runs on Android 10 and up alone (the maintainer, 2026-11-22). A torrent's files stay where they are
//! (`edm_core`'s queue never hands them over): its seeding reads them there.
//!
//! The folders EDM writes by path ([`places`]): only EDM's own is offered for downloads (the shared one is reached through publishing), while a file EDM writes for the
//! person (Export, Back up EDM) goes to the shared `Download/EDM` where EDM can write a file there
//! (Android 11 and up let an app make its own files in it by path), else to EDM's own.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::phone::Places;

/// The folder, under the shared storage, that EDM's finished files are put in.
pub(crate) const SHARED_FOLDER: &str = "Download/EDM";

/// EDM's own download folder on the shared storage: `getExternalFilesDir(DIRECTORY_DOWNLOADS)`
/// (`external` is `getExternalFilesDir(null)`) and `EDM` in it.
pub(crate) fn own_downloads(external: &Path) -> PathBuf {
    external.join("Download").join("EDM")
}

/// The folders EDM downloads into that only EDM sees, most particular first: the one above, the
/// app's folder that earlier starts chose on Android 10 (`files/Download`), and the one in EDM's
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

/// The folders EDM writes by path, of `shared` (the shared `Download/EDM`, where EDM could write a
/// file in it) and `own` (EDM's own folder, where it could): downloads go only to EDM's own folder
/// (each finished file is published to the shared one); a file EDM writes for the person goes to
/// the shared one where EDM could write there, else to EDM's own.
pub(crate) fn places(shared: Option<PathBuf>, own: Option<PathBuf>) -> Places {
    let save = shared.or_else(|| own.clone());
    Places { own, save }
}

/// Where in the shared storage a finished file goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shared {
    /// The folders under EDM's own download folder it was in (a category's, say), kept.
    pub(crate) sub: Vec<String>,
    /// Its name.
    pub(crate) name: String,
}

impl Shared {
    /// The media store's `RELATIVE_PATH`: `Download/EDM`, and its sub-folders.
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
            .fold(public.join("EDM"), |folder, sub| folder.join(sub))
    }
}

/// Where the finished file at `file` goes in the shared storage, if it is in one of EDM's
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
            // `..` or a root: not a path EDM made, never followed out of the folder.
            _ => return None,
        }
    }
    let name = parts.pop().filter(|name| !name.is_empty())?;
    Some(Shared { sub: parts, name })
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

#[cfg(test)]
mod tests;
