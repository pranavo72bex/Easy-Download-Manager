// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What only the phone can do for dm's window, handed in by the Android app as it starts
//! (gaps row N17): a notification, opening a file and showing its folder, the clipboard,
//! keeping the phone awake while a download runs, and the link a share hands over. A library
//! reaches none of them: each is a call into Android's Java side, which `dm-android`'s JNI
//! modules make. So `dm-android` gives its own [`PhoneServices`] to [`set_phone`] before dm's
//! window starts, and the Android arms of the system's services (`desktop`, `awake`, and the
//! window's clipboard and notifier) ask through the free functions here.
//!
//! Built on every system, so `dm-android` and its tests build on a laptop too; only Android's
//! arms call it. Until a phone is handed in, or for what its services leave out, each says in
//! dm's words that the phone cannot, or does nothing where nothing is the answer (a
//! notification, the clipboard, keeping awake).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::drops::Dropped;
use crate::quiet::Note;

/// What the phone does for dm, through the app. Each has an answer of its own, so the app
/// implements only what it has.
pub trait PhoneServices: Send + Sync {
    /// Posts `note` in the phone's notification shade: a click on one that names a file
    /// opens it; one with a retry offers Retry.
    fn notify(&self, note: Note) {
        let _ = note;
    }

    /// Opens the file at `path` in the app the phone picks for it.
    fn open(&self, path: &str) -> Result<(), String> {
        let _ = path;
        Err(NOT_YET.into())
    }

    /// Shows the folder `path` is in, in the phone's file manager.
    fn reveal(&self, path: &str) -> Result<(), String> {
        let _ = path;
        Err(NOT_YET.into())
    }

    /// The clipboard's text, if it holds any.
    fn clipboard(&self) -> Option<String> {
        None
    }

    /// Puts `text` on the clipboard.
    fn set_clipboard(&self, text: &str) {
        let _ = text;
    }

    /// Holds the phone awake (a wake lock) while `on`, and lets it sleep again when not.
    fn keep_awake(&self, on: bool) {
        let _ = on;
    }

    /// Takes the file at `path` away where it can be put back; a phone has no Trash dm can
    /// reach, so by default it is refused and the file kept.
    fn trash(&self, path: &str) -> Result<(), String> {
        let _ = path;
        Err(NO_TRASH.into())
    }

    /// The folders dm can download into by their path on this phone (gaps row N9): on Android
    /// 10 and up only the app's own folders and, where the phone lets dm write there, the
    /// shared `Download/dm`; a folder of the storage picker's is an address, not a path. What
    /// Browse… offers on a phone.
    fn download_folders(&self) -> Result<Vec<PathBuf>, String> {
        Err(NO_FOLDERS.into())
    }

    /// The folder a file dm writes for the person goes to (Export, Back up dm, Save settings
    /// to a file): the phone has no save panel dm's window can ask (GPUI's answers none), so
    /// the file is written here and the window says where.
    fn save_folder(&self) -> Result<PathBuf, String> {
        Err(NO_SAVE.into())
    }
}

/// What is said where the phone's own way is not handed in.
const NOT_YET: &str = "this phone cannot do that for dm yet";

/// What is said when a file is to go to the Trash on a phone.
const NO_TRASH: &str = "a phone has no Trash: the file is kept where it is";

/// What is said where the phone names no folder dm can download into.
const NO_FOLDERS: &str = "this phone names no folder dm can download into";

/// What is said where the phone names no folder for a file dm writes.
const NO_SAVE: &str = "this phone names no folder dm can save a file in";

/// The phone's services, once the app has handed them in.
static PHONE: RwLock<Option<Arc<dyn PhoneServices>>> = RwLock::new(None);

/// Hands in the phone's services: called once by `dm-android` as it starts, before dm's
/// window. A second call replaces the first.
pub fn set_phone(services: Box<dyn PhoneServices>) {
    let services: Arc<dyn PhoneServices> = Arc::from(services);
    match PHONE.write() {
        Ok(mut phone) => *phone = Some(services),
        Err(poisoned) => *poisoned.into_inner() = Some(services),
    }
}

/// The phone's services, where they were handed in.
pub fn phone() -> Option<Arc<dyn PhoneServices>> {
    match PHONE.read() {
        Ok(phone) => phone.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Posts `note` through the phone; nothing where none is handed in.
pub fn notify(note: Note) {
    if let Some(phone) = phone() {
        phone.notify(note);
    }
}

/// Opens `path` through the phone, or says it cannot.
pub fn open(path: &str) -> Result<(), String> {
    phone().map_or_else(|| Err(NOT_YET.into()), |phone| phone.open(path))
}

/// Shows `path`'s folder through the phone, or says it cannot.
pub fn reveal(path: &str) -> Result<(), String> {
    phone().map_or_else(|| Err(NOT_YET.into()), |phone| phone.reveal(path))
}

/// The phone's clipboard text.
pub fn clipboard() -> Option<String> {
    phone()?.clipboard()
}

/// Puts `text` on the phone's clipboard.
pub fn set_clipboard(text: &str) {
    if let Some(phone) = phone() {
        phone.set_clipboard(text);
    }
}

/// Holds the phone awake while `on`.
pub fn keep_awake(on: bool) {
    if let Some(phone) = phone() {
        phone.keep_awake(on);
    }
}

/// `path` to the phone's Trash, or refused in words.
pub fn trash(path: &str) -> Result<(), String> {
    phone().map_or_else(|| Err(NO_TRASH.into()), |phone| phone.trash(path))
}

/// The folders dm can download into on the phone, or why there are none.
pub fn download_folders() -> Result<Vec<PathBuf>, String> {
    match phone().map_or_else(|| Err(NO_FOLDERS.into()), |phone| phone.download_folders()) {
        Ok(folders) if folders.is_empty() => Err(NO_FOLDERS.into()),
        answer => answer,
    }
}

/// Where a file dm writes for the person, offered as `name`, goes on the phone: the save
/// folder, made where it is not there yet, under `name` or, where something has that name,
/// the first `name (n)` free, so nothing of the person's is written over.
pub fn save_place(name: &str) -> Result<PathBuf, String> {
    let folder = phone().map_or_else(|| Err(NO_SAVE.into()), |phone| phone.save_folder())?;
    save_place_in(&folder, name)
}

/// [`save_place`] in `folder`.
fn save_place_in(folder: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("{} could not be made: {e}", folder.display()))?;
    let name = if name.is_empty() { "dm" } else { name };
    Ok(folder.join(dm_core::free_name(folder, name)))
}

/// A link (or lines of links) the phone's share sheet handed dm: added as a link dropped on
/// the window is, the window woken to read it (`drops`).
pub fn shared(text: String) {
    crate::drops::arrived(Dropped::Text(text));
}

/// A file the phone handed dm (a `.torrent` opened with dm, copied into dm's home first):
/// added as a file dropped on the window is, the `.torrent` itself the download.
pub fn shared_file(path: std::path::PathBuf) {
    crate::drops::arrived(Dropped::File(path));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no phone handed in, or one that leaves a service out, dm says so in words and
    /// never panics.
    #[test]
    fn what_the_phone_leaves_out_is_said_in_words() {
        struct Nothing;
        impl PhoneServices for Nothing {}
        let nothing = Nothing;
        assert_eq!(nothing.open("/a"), Err(NOT_YET.to_string()));
        assert_eq!(nothing.reveal("/a"), Err(NOT_YET.to_string()));
        assert_eq!(nothing.trash("/a"), Err(NO_TRASH.to_string()));
        assert_eq!(nothing.clipboard(), None);
        nothing.keep_awake(true);
        nothing.set_clipboard("x");
        assert_eq!(nothing.download_folders(), Err(NO_FOLDERS.to_string()));
        assert_eq!(nothing.save_folder(), Err(NO_SAVE.to_string()));
    }

    /// A file dm writes on the phone goes into the save folder, made where it is missing,
    /// and never over a file already there.
    #[test]
    fn a_saved_file_takes_a_free_name() {
        let dir = std::env::temp_dir().join(format!("dm-phone-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = dir.join("Download").join("dm");
        let first = save_place_in(&folder, "downloads.dmlist").unwrap();
        assert_eq!(first, folder.join("downloads.dmlist"));
        std::fs::write(&first, b"x").unwrap();
        let second = save_place_in(&folder, "downloads.dmlist").unwrap();
        assert_eq!(second, folder.join("downloads (1).dmlist"));
        assert_eq!(save_place_in(&folder, "").unwrap(), folder.join("dm"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
