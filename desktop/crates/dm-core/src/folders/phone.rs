// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Android's folders (gaps row N2). A phone names no home folder and no Downloads folder to a
//! library: `dirs` finds neither there (no `$HOME`, no XDG user folders), and Android 10 and up
//! give an app no free access to the disk (row N9). So the app hands them in, its own files
//! folder and the folder new downloads go to, before the engine starts
//! (`dm-android`'s `Engine::start`), and [`super::home`], [`super::downloads`] and
//! [`super::local_data`] answer with them. Until then, or for a folder that is not a full path,
//! the answer is `None`, never a relative path.
//!
//! Built on the desktop for this module's own tests alone.

use std::path::PathBuf;
use std::sync::Mutex;

/// The two folders the app gave.
struct Given {
    home: PathBuf,
    downloads: PathBuf,
}

/// What the app gave, the last time it said.
static GIVEN: Mutex<Option<Given>> = Mutex::new(None);

/// The app's own files folder (`home`) and the folder new downloads go to until Options says
/// otherwise (`downloads`), for dm's folders on a phone. Said again at each start of the
/// engine: the newest is kept.
pub fn set_phone_folders(home: PathBuf, downloads: PathBuf) {
    *crate::lock(&GIVEN) = Some(Given { home, downloads });
}

/// One of the folders given, where it is a full path.
fn given(pick: impl Fn(&Given) -> PathBuf) -> Option<PathBuf> {
    crate::lock(&GIVEN)
        .as_ref()
        .map(pick)
        .filter(|path| path.is_absolute())
}

/// The app's own files folder.
pub(super) fn home() -> Option<PathBuf> {
    given(|given| given.home.clone())
}

/// The folder new downloads go to.
pub(super) fn downloads() -> Option<PathBuf> {
    given(|given| given.downloads.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Given full paths they are the answer; a relative one is none, never a folder taken
    /// against wherever the process happens to be.
    #[test]
    fn the_folders_given_are_answered_and_a_relative_one_is_none() {
        // Full paths on the system the test runs on (a drive's on Windows).
        let (files, download) = (
            std::env::temp_dir().join("files"),
            std::env::temp_dir().join("Download"),
        );
        set_phone_folders(files.clone(), download.clone());
        assert_eq!(home(), Some(files));
        assert_eq!(downloads(), Some(download.clone()));
        set_phone_folders("files".into(), download.clone());
        assert_eq!(home(), None);
        assert_eq!(downloads(), Some(download));
    }
}
