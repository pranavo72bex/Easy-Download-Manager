// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's services as dm's window asks for them (`dm_desktop_base::phone::PhoneServices`,
//! gaps rows N3, N6, N12, N17), made of the calls into Android that `android/java.rs` makes
//! through JNI ([`Java`]). What each service does with those calls is decided here, where the
//! host's tests reach it: a finished file indexed before its notification, a failure's with
//! Retry, the wake lock and the ongoing notification together, the schedule's alarm set only
//! when its time changes, the Trash refused.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dm_desktop_base::phone::PhoneServices;
use dm_desktop_base::quiet::Note;

/// The calls into Android that dm's services are made of. Each comes from any of dm's threads
/// (the window's, the engine's); a failure is said in dm's words, never a panic.
pub trait Java: Send + Sync + 'static {
    /// A message in the shade: `open` is a finished file a tap opens; without one a tap brings
    /// dm to the front.
    fn post(&self, title: &str, body: &str, open: Option<&str>) -> Result<(), String>;
    /// A failed download's message, with a Retry button that tries row `row` again; a tap
    /// brings dm to the front.
    fn post_failed(&self, title: &str, body: &str, row: u64) -> Result<(), String>;
    /// Adds the finished file at `path` to the phone's media index, so a tap lends it to
    /// another app by a `content://` address.
    fn index(&self, path: &str) -> Result<(), String>;
    /// Opens the file at `path` with the app the phone opens it with.
    fn open(&self, path: &str) -> Result<(), String>;
    /// Shows where `path` is: the phone's Downloads.
    fn reveal(&self, path: &str) -> Result<(), String>;
    /// The clipboard's text, where there is any.
    fn clipboard(&self) -> Option<String>;
    /// Puts `text` on the clipboard.
    fn set_clipboard(&self, text: &str);
    /// The ongoing notification, put up while anything downloads.
    fn ongoing(&self, title: &str, body: &str) -> Result<(), String>;
    /// Takes the ongoing notification down.
    fn end_ongoing(&self) -> Result<(), String>;
    /// Holds the processor awake while `on` (a partial wake lock), and lets it sleep after.
    fn stay_awake(&self, on: bool) -> Result<(), String>;
    /// Sets the schedule's alarm for `at` (milliseconds since 1970), or clears it.
    fn alarm(&self, at: Option<i64>) -> Result<(), String>;
    /// The time now, in milliseconds since 1970.
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
            })
    }
}

/// The ongoing notification's words while a download runs (gaps row N6). The count and the
/// speed it said under the placeholder's engine wait for the seam to carry them (docs/gaps.md,
/// N6).
pub const ONGOING_TITLE: &str = "dm is downloading";
/// The ongoing notification's second line.
pub const ONGOING_BODY: &str = "Tap to see the downloads";

/// How long a finished file's notification waits for the media index to take the file.
const INDEX_WAIT: Duration = Duration::from_millis(1500);

/// dm's [`PhoneServices`] over the phone's [`Java`] side: what `android_main` hands to
/// `dm_desktop_base::phone::set_phone` before dm's window starts.
pub struct Phone<J> {
    java: Arc<J>,
    index_wait: Duration,
    /// The schedule's alarm as last set (`None` before the first).
    armed: Mutex<Option<Option<i64>>>,
}

impl<J: Java> Phone<J> {
    /// The services over `java`.
    pub fn new(java: J) -> Self {
        Phone {
            java: Arc::new(java),
            index_wait: INDEX_WAIT,
            armed: Mutex::new(None),
        }
    }
}

impl<J: Java> PhoneServices for Phone<J> {
    fn notify(&self, note: Note) {
        let Note {
            title,
            body,
            path,
            retry,
        } = note;
        // A failure: Retry on it, which tries the row again as the window's Retry does.
        if let (None, Some(row)) = (&path, retry) {
            let _ = self.java.post_failed(&title, &body, row);
            return;
        }
        let Some(path) = path else {
            // Without the permission to notify, the window still shows it.
            let _ = self.java.post(&title, &body, None);
            return;
        };
        // A finished file is added to the media index first, so the tap lends it by a
        // `content://` address; the index takes a moment, so the notification waits for it
        // off the caller's thread.
        let _ = self.java.index(&path);
        let java = Arc::clone(&self.java);
        let wait = self.index_wait;
        let posted = std::thread::Builder::new()
            .name("dm-notify".into())
            .spawn(move || {
                std::thread::sleep(wait);
                let _ = java.post(&title, &body, Some(&path));
            });
        drop(posted);
    }

    fn open(&self, path: &str) -> Result<(), String> {
        self.java.open(path)
    }

    fn reveal(&self, path: &str) -> Result<(), String> {
        self.java.reveal(path)
    }

    fn clipboard(&self) -> Option<String> {
        self.java.clipboard().filter(|text| !text.is_empty())
    }

    fn set_clipboard(&self, text: &str) {
        self.java.set_clipboard(text);
    }

    fn keep_awake(&self, on: bool) {
        // A refusal (no permission to notify yet) leaves the wake lock to do its part.
        if on {
            let _ = self.java.stay_awake(true);
            let _ = self.java.ongoing(ONGOING_TITLE, ONGOING_BODY);
        } else {
            let _ = self.java.end_ongoing();
            let _ = self.java.stay_awake(false);
        }
    }

    fn next_start(&self, minutes: Option<u32>) {
        let at = minutes.map(|minutes| crate::wakeup::alarm_at(self.java.now(), minutes));
        let mut armed = dm_core::lock(&self.armed);
        if *armed == Some(at) {
            return;
        }
        // Kept only once set, so a refusal is tried again at the next look.
        if self.java.alarm(at).is_ok() {
            *armed = Some(at);
        }
    }

    // `trash`: the seam's own answer. A phone has no Trash dm can reach, so the file is kept
    // and that is said.
}

#[cfg(test)]
mod tests;
