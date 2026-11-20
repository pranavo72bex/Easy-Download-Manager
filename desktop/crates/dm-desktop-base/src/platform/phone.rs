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

use std::path::{Path, PathBuf};
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

    /// What keeps dm busy (gaps row N6), each time the queue changes and once a second while
    /// anything does: `Some` while a row runs, a queued row is due or a finished torrent is
    /// shared, `None` once nothing is. The phone keeps its wake lock and its ongoing
    /// notification (saying [`Busy::words`]) for as long as it is `Some`. Only Android's
    /// `awake` watch calls it; by default it does nothing.
    fn busy(&self, summary: Option<Busy>) {
        let _ = summary;
    }

    /// Takes the file at `path` away where it can be put back; a phone has no Trash dm can
    /// reach, so by default it is refused and the file kept.
    fn trash(&self, path: &str) -> Result<(), String> {
        let _ = path;
        Err(NO_TRASH.into())
    }

    /// Puts the file a download has just finished at `path` where the phone's other apps see
    /// it (gaps row N9: on Android 10 and up dm downloads into its own folder and the phone
    /// hands each finished file to the shared Download folder): where it now is, `None` where
    /// it is left where it is, or why it could not be, the file then kept. Asked by the engine
    /// as a run finishes, before the row says Completed, on one of its blocking threads
    /// (`dm_core::folders::set_publisher`, handed in by [`set_phone`] on Android alone); by
    /// default every file is left where it is.
    fn publish(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        let _ = path;
        Ok(None)
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

/// What keeps dm busy on a phone, for its wake lock and its ongoing notification (gaps row N6).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Busy {
    /// Rows running and still fetching.
    pub running: usize,
    /// Rows running with every byte, putting their file together.
    pub finishing: usize,
    /// Rows queued while the schedule and the download limit let them start.
    pub waiting: usize,
    /// Whether a finished torrent is being shared.
    pub seeding: bool,
    /// The fetching rows' speeds together.
    pub bytes_per_second: f64,
}

impl Busy {
    /// What `rows` keep busy: the status bar's counts (`rules::tally`), the queued rows when
    /// `due` (neither the schedule nor the download limit holds the queue), and `seeding`
    /// (`dm_core::sharing_any`). `None` when none of them is anything.
    #[must_use]
    pub fn of(rows: &[Arc<dm_core::Entry>], due: bool, seeding: bool) -> Option<Busy> {
        let tally = dm_shell::app::rules::tally(rows);
        let waiting = if due {
            rows.iter()
                .filter(|row| row.state == dm_core::State::Queued)
                .count()
        } else {
            0
        };
        let busy = Busy {
            running: tally.running,
            finishing: tally.finishing,
            waiting,
            seeding,
            bytes_per_second: tally.rate,
        };
        (busy.running + busy.finishing + busy.waiting > 0 || seeding).then_some(busy)
    }

    /// The ongoing notification's title and second line, in the language in force on this
    /// thread and in words dm already says: the status bar's (how many download and how
    /// fast, else how many finish), Exit's question's (how many wait to start), else the
    /// tray's for dm at work in the background. The second line says how many more wait
    /// beside the running ones, where any do.
    #[must_use]
    pub fn words(&self) -> (String, Option<String>) {
        use crate::i18n::tn;
        let title = if self.running > 0 {
            tn(
                "{} downloading at {}||{} downloading at {}",
                self.running as u64,
                &[&self.running, &crate::format::rate(self.bytes_per_second)],
            )
        } else if self.finishing > 0 {
            tn(
                "{} finishing||{} finishing",
                self.finishing as u64,
                &[&self.finishing],
            )
        } else if self.waiting > 0 {
            let waiting = tn(
                "{} download is waiting to start.||{} downloads are waiting to start.",
                self.waiting as u64,
                &[&self.waiting],
            );
            return (waiting, None);
        } else {
            return (crate::tray::words()[4].to_string(), None);
        };
        let more = (self.waiting > 0).then(|| {
            tn(
                "{} more is waiting to start.||{} more are waiting to start.",
                self.waiting as u64,
                &[&self.waiting],
            )
        });
        (title, more)
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
    // The engine asks the phone where each finished file goes (gaps row N9).
    #[cfg(target_os = "android")]
    dm_core::folders::set_publisher(std::sync::Arc::new(publish));
}

/// The file a download finished at `path`, published through the phone; left where it is
/// where none is handed in.
pub fn publish(path: &Path) -> Result<Option<PathBuf>, String> {
    phone().map_or(Ok(None), |phone| phone.publish(path))
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

/// Tells the phone what keeps dm busy, or that nothing does.
pub fn busy(summary: Option<Busy>) {
    if let Some(phone) = phone() {
        phone.busy(summary);
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
fn save_place_in(folder: &Path, name: &str) -> Result<PathBuf, String> {
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
pub fn shared_file(path: PathBuf) {
    crate::drops::arrived(Dropped::File(path));
}

#[cfg(test)]
mod tests;
