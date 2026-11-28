// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finished file handed to the phone's shared storage (gaps row N9). Android 10 and up let
//! no app write the shared Download folder by path, so on a phone EDM downloads into the app's
//! own folder on the shared storage, where segmented writes, resume and every file the engine
//! handles work by path as on a desktop, and the app hands in a [`Publisher`] that puts each
//! finished file in the shared Download folder (through the system's media store) and says
//! where it now is. The queue asks it as a run finishes, before the row says Completed, so
//! the row, its notification and an open-when-done all name the published file
//! (`queue/runner/finish.rs`, on Android alone). A desktop never hands one in.
//!
//! Built on the desktop for this module's own tests alone.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Puts a finished file where the phone's other apps see it: `Ok(None)` where it is left
/// where it is (it is not in the app's own folder), `Ok(Some(path))` where it now is, and why
/// not where it could not be, the file then kept where it was.
pub type Publisher = dyn Fn(&Path) -> Result<Option<PathBuf>, String> + Send + Sync;

/// The app's publisher, once it has handed one in.
static PUBLISHER: Mutex<Option<Arc<Publisher>>> = Mutex::new(None);

/// Hands in the phone's publisher: called once by the app as it starts, before the engine.
/// A second call replaces the first.
pub fn set_publisher(publisher: Arc<Publisher>) {
    *crate::lock(&PUBLISHER) = Some(publisher);
}

/// The publisher handed in, if one was.
pub(crate) fn publisher() -> Option<Arc<Publisher>> {
    crate::lock(&PUBLISHER).clone()
}

/// Where the finished file at `path` is once `publisher` has been asked, and what the person
/// should hear where it could not be published: the file then stays where it is, whole.
pub(crate) fn published(path: &Path, publisher: &Publisher) -> (PathBuf, Option<String>) {
    match publisher(path) {
        Ok(Some(now)) => (now, None),
        Ok(None) => (path.to_path_buf(), None),
        Err(why) => (
            path.to_path_buf(),
            Some(format!(
                "{} could not be put in the phone's Download folder and stays in EDM's own \
                 folder: {why}",
                path.display()
            )),
        ),
    }
}

/// Where a finished folder (a torrent of several files) is once each file in it has been
/// handed to `publisher`, keeping the folders inside it: its new place, found from the first
/// file published; and what the person should hear where any file could not be (those stay,
/// whole, where they were). The folder left empty in EDM's own place is taken away.
pub(crate) fn published_folder(folder: &Path, publisher: &Publisher) -> (PathBuf, Option<String>) {
    let mut files = Vec::new();
    every_file(folder, &mut files);
    files.sort();
    let mut moved_to: Option<PathBuf> = None;
    let mut refused = Vec::new();
    for file in &files {
        match publisher(file) {
            Ok(Some(now)) => {
                if moved_to.is_none() {
                    // The folder's new place: the published file less its path inside it.
                    if let Ok(inside) = file.strip_prefix(folder) {
                        let mut place = now.clone();
                        for _ in inside.components() {
                            place.pop();
                        }
                        moved_to = Some(place);
                    }
                }
            }
            Ok(None) => return (folder.to_path_buf(), None),
            Err(why) => refused.push(format!("{}: {why}", file.display())),
        }
    }
    if refused.is_empty() {
        // Only empty folders are left: remove_dir takes none that still holds a file.
        remove_empty(folder);
    }
    let why = (!refused.is_empty()).then(|| {
        format!(
            "Some files could not be put in the phone's Download folder and stay in EDM's own \
             folder: {}",
            refused.join("; ")
        )
    });
    let place = if refused.is_empty() {
        moved_to.unwrap_or_else(|| folder.to_path_buf())
    } else {
        folder.to_path_buf()
    };
    (place, why)
}

/// Every file under `folder`, into `files`.
fn every_file(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => every_file(&path, files),
            Ok(kind) if kind.is_file() => files.push(path),
            _ => {}
        }
    }
}

/// `folder` and the folders in it taken away where they hold no file.
fn remove_empty(folder: &Path) {
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                remove_empty(&entry.path());
            }
        }
    }
    let _ = std::fs::remove_dir(folder);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A finished torrent's folder: each file published with the folders inside it kept, the
    /// row then naming the folder's new place, and the empty folder left behind taken away.
    #[test]
    fn a_finished_folder_is_published_file_by_file() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        let public = root.path().join("public");
        let folder = private.join("Sintel");
        std::fs::create_dir_all(folder.join("subs")).unwrap();
        std::fs::write(folder.join("Sintel.mp4"), b"film").unwrap();
        std::fs::write(folder.join("subs").join("en.srt"), b"words").unwrap();
        let (from, to) = (private.clone(), public.clone());
        let put = move |file: &Path| -> Result<Option<PathBuf>, String> {
            let now = to.join(file.strip_prefix(&from).unwrap());
            std::fs::create_dir_all(now.parent().unwrap()).unwrap();
            std::fs::rename(file, &now).unwrap();
            Ok(Some(now))
        };
        let (place, why) = published_folder(&folder, &put);
        assert_eq!(why, None);
        assert_eq!(place, public.join("Sintel"));
        assert!(place.join("Sintel.mp4").is_file());
        assert!(place.join("subs").join("en.srt").is_file());
        assert!(!folder.exists(), "the empty folder was left behind");
        // One refused: the folder stays where it was, and why is said.
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.bin"), b"a").unwrap();
        let refused = |_: &Path| -> Result<Option<PathBuf>, String> { Err("no room".into()) };
        let (place, why) = published_folder(&folder, &refused);
        assert_eq!(place, folder);
        assert!(why.is_some_and(|why| why.contains("no room")));
        assert!(folder.join("a.bin").is_file());
    }

    /// Published, the file is where the publisher says; left alone, where it was; refused,
    /// where it was, with the publisher's reason in the sentence the person is told.
    #[test]
    fn a_finished_file_is_where_the_publisher_put_it_or_kept_with_why() {
        let file = std::env::temp_dir().join("a.zip");
        let moved = std::env::temp_dir().join("Download").join("a.zip");
        let to = moved.clone();
        let put = move |_: &Path| -> Result<Option<PathBuf>, String> { Ok(Some(to.clone())) };
        assert_eq!(published(&file, &put), (moved, None));
        let left = |_: &Path| -> Result<Option<PathBuf>, String> { Ok(None) };
        assert_eq!(published(&file, &left), (file.clone(), None));
        let refused = |_: &Path| -> Result<Option<PathBuf>, String> { Err("no room".into()) };
        let (kept, why) = published(&file, &refused);
        assert_eq!(kept, file);
        assert!(why.is_some_and(|why| why.ends_with("no room") && why.contains("a.zip")));
    }

    /// The publisher handed in is the one asked; a second replaces the first.
    #[test]
    fn the_last_publisher_handed_in_is_kept() {
        set_publisher(Arc::new(|_: &Path| Ok(None)));
        set_publisher(Arc::new(|_: &Path| Err("second".into())));
        let publisher = publisher().expect("a publisher was handed in");
        assert_eq!(publisher(Path::new("x")), Err("second".to_string()));
    }
}
