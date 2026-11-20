// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finished file handed to the phone's shared storage (gaps row N9). Android 10 and up let
//! no app write the shared Download folder by path, so on a phone dm downloads into the app's
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
                "{} could not be put in the phone's Download folder and stays in dm's own \
                 folder: {why}",
                path.display()
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
