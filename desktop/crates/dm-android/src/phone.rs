// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's services as dm's window asks for them (`dm_desktop_base::phone::PhoneServices`,
//! gaps rows N3, N6, N17), made of the calls into Android that `android/java.rs` makes through
//! JNI ([`Java`]). What each service does with those calls is decided here, where the host's
//! tests reach it: a finished file indexed before its notification, the wake lock and the
//! ongoing notification together for as long as dm is busy, its words at most once a second,
//! a finished file published to the shared Download folder (gaps row N9), the Trash refused.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dm_desktop_base::phone::{Busy, PhoneNetwork, PhoneServices};
use dm_desktop_base::quiet::Note;

/// The calls into Android that dm's services are made of. Each comes from any of dm's threads
/// (the window's, the engine's); a failure is said in dm's words, never a panic.
pub trait Java: Send + Sync + 'static {
    /// A message in the shade: `open` is a finished file a tap opens; without one a tap brings
    /// dm to the front.
    fn post(&self, title: &str, body: &str, open: Option<&str>) -> Result<(), String>;
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
    /// Puts the finished file at `path` in the shared `Download/dm` where it is in one of dm's
    /// own folders (gaps row N9): where it now is, `None` where it is left, or why it could not
    /// be, the file then kept. Asked on one of the engine's blocking threads.
    fn publish(&self, path: &Path) -> Result<Option<PathBuf>, String>;
    /// The phone's connection and battery now, for its rules (gaps row N10); what a call could
    /// not read is unknown, which holds nothing.
    fn network(&self) -> PhoneNetwork;
}

/// The ongoing notification's second line where [`Busy::words`] has none.
pub const ONGOING_BODY: &str = "Tap to see the downloads";

/// How often the ongoing notification's words change at most: its speed moves many times a
/// second, and a phone redraws its shade for each change.
pub const ONGOING_EVERY: Duration = Duration::from_secs(1);

/// How long a finished file's notification waits for the media index to take the file.
const INDEX_WAIT: Duration = Duration::from_millis(1500);

/// dm's [`PhoneServices`] over the phone's [`Java`] side: what `android_main` hands to
/// `dm_desktop_base::phone::set_phone` before dm's window starts.
pub struct Phone<J> {
    java: Arc<J>,
    index_wait: Duration,
    places: Places,
    ongoing: Mutex<Ongoing>,
}

/// The wake lock and the ongoing notification as last set.
#[derive(Default)]
pub struct Ongoing {
    /// Whether the wake lock is held and the notification up.
    held: bool,
    /// The words last shown, and when.
    shown: Option<((String, String), Instant)>,
}

/// What the phone is to do as dm's busyness is told.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Take the wake lock and put the notification up with these words.
    Hold(String, String),
    /// Change the notification's words.
    Say(String, String),
    /// Take the notification down and let the wake lock go.
    Let,
    /// Nothing changes.
    Keep,
}

impl Ongoing {
    /// What to do at `now` when dm's busyness is `words` (`None`: nothing keeps it busy): the
    /// wake lock and the notification taken and let go at once, the words changed only where
    /// they differ and at most once each [`ONGOING_EVERY`].
    pub fn step(&mut self, words: Option<(String, String)>, now: Instant) -> Step {
        match words {
            None if self.held => {
                *self = Ongoing::default();
                Step::Let
            }
            None => Step::Keep,
            Some((title, body)) if !self.held => {
                self.held = true;
                self.shown = Some(((title.clone(), body.clone()), now));
                Step::Hold(title, body)
            }
            Some(words) => match &self.shown {
                Some((said, at)) if *said == words || now.duration_since(*at) < ONGOING_EVERY => {
                    Step::Keep
                }
                _ => {
                    self.shown = Some((words.clone(), now));
                    Step::Say(words.0, words.1)
                }
            },
        }
    }
}

/// The notification's title and second line for `busy`, in dm's words.
fn ongoing_words(busy: &Busy) -> (String, String) {
    let (title, more) = busy.words();
    (title, more.unwrap_or_else(|| ONGOING_BODY.to_owned()))
}

/// The folders of the phone's storage dm writes by their path (gaps row N9), worked out once as
/// dm starts (`android/storage.rs`, decided in `storage::places`): on Android 10 and up a
/// folder of the storage picker's is an address only that picker's permission opens, not a
/// path the engine can resume a segmented download in, so these are what Browse… offers and
/// where a file dm writes for the person goes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    /// The shared `Download/dm`, which the Files app shows, where dm downloads into it by path:
    /// on Android 8 and 9 once storage is allowed. On 10 and up dm downloads into its own folder
    /// and publishes each finished file here instead, so it is not offered there.
    pub shared: Option<PathBuf>,
    /// dm's own folder on the phone's storage (`Android/data/com.dm.android/files/Download/dm`),
    /// a finished file in which is published to `Download/dm`.
    pub own: Option<PathBuf>,
    /// Where a file dm writes for the person goes (Export, Back up dm): the shared
    /// `Download/dm` where dm can write a file there by path (Android 8 and 9 once storage is
    /// allowed, 11 and up), else dm's own folder.
    pub save: Option<PathBuf>,
}

impl Places {
    /// Where new downloads go by default: the shared folder where dm downloads into it, else
    /// dm's own.
    pub fn first(&self) -> Option<&PathBuf> {
        self.shared.as_ref().or(self.own.as_ref())
    }
}

impl<J: Java> Phone<J> {
    /// The services over `java`.
    pub fn new(java: J) -> Self {
        Phone {
            java: Arc::new(java),
            index_wait: INDEX_WAIT,
            places: Places::default(),
            ongoing: Mutex::default(),
        }
    }

    /// The services, with the folders dm can write by path.
    pub fn with_places(mut self, places: Places) -> Self {
        self.places = places;
        self
    }
}

impl<J: Java> PhoneServices for Phone<J> {
    fn notify(&self, note: Note) {
        // A Retry button on a failure needs a broadcast receiver, a Java class (docs/gaps.md,
        // N6); until then a failure's notification brings dm to the front, where the row's
        // Retry is.
        let Note {
            title, body, path, ..
        } = note;
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
        // Android's `awake` watch tells `busy`; on or off alone is busy with nothing counted.
        self.busy(on.then(Busy::default));
    }

    fn busy(&self, summary: Option<Busy>) {
        let words = summary.as_ref().map(ongoing_words);
        let step = match self.ongoing.lock() {
            Ok(mut ongoing) => ongoing.step(words, Instant::now()),
            Err(poisoned) => poisoned.into_inner().step(words, Instant::now()),
        };
        // A refusal (no permission to notify yet) leaves the wake lock to do its part.
        match step {
            Step::Hold(title, body) => {
                let _ = self.java.stay_awake(true);
                let _ = self.java.ongoing(&title, &body);
            }
            Step::Say(title, body) => {
                let _ = self.java.ongoing(&title, &body);
            }
            Step::Let => {
                let _ = self.java.end_ongoing();
                let _ = self.java.stay_awake(false);
            }
            Step::Keep => {}
        }
    }

    fn publish(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        self.java.publish(path)
    }

    fn network(&self) -> PhoneNetwork {
        self.java.network()
    }

    fn download_folders(&self) -> Result<Vec<PathBuf>, String> {
        let Places { shared, own, .. } = &self.places;
        Ok(shared.iter().chain(own).cloned().collect())
    }

    fn save_folder(&self) -> Result<PathBuf, String> {
        self.places
            .save
            .as_ref()
            .or_else(|| self.places.first())
            .cloned()
            .ok_or_else(|| "this phone gave dm no folder on its storage to save in".into())
    }

    // `trash`: the seam's own answer. A phone has no Trash dm can reach, so the file is kept
    // and that is said.
}

#[cfg(test)]
mod tests;
