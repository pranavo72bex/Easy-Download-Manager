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

    /// The schedule's next start, `minutes` from now (`None`: none), so the phone wakes dm for
    /// it (an alarm; gaps row N12). Told every minute by the wake watch (`wake`).
    fn next_start(&self, minutes: Option<u32>) {
        let _ = minutes;
    }

    /// Takes the file at `path` away where it can be put back; a phone has no Trash dm can
    /// reach, so by default it is refused and the file kept.
    fn trash(&self, path: &str) -> Result<(), String> {
        let _ = path;
        Err(NO_TRASH.into())
    }
}

/// What is said where the phone's own way is not handed in.
const NOT_YET: &str = "this phone cannot do that for dm yet";

/// What is said when a file is to go to the Trash on a phone.
const NO_TRASH: &str = "a phone has no Trash: the file is kept where it is";

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

/// Tells the phone the schedule's next start, `minutes` from now.
pub fn next_start(minutes: Option<u32>) {
    if let Some(phone) = phone() {
        phone.next_start(minutes);
    }
}

/// `path` to the phone's Trash, or refused in words.
pub fn trash(path: &str) -> Result<(), String> {
    phone().map_or_else(|| Err(NO_TRASH.into()), |phone| phone.trash(path))
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
    }
}
