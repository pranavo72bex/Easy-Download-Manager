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

    /// What keeps dm busy (gaps row N6), each time the queue changes and once a second while
    /// anything does: `Some` while a row runs, a queued row is due or a finished torrent is
    /// shared, `None` once nothing is. The phone keeps its wake lock and its ongoing
    /// notification (saying [`Busy::words`]) for as long as it is `Some`. Only Android's
    /// `awake` watch calls it; by default it does nothing.
    fn busy(&self, summary: Option<Busy>) {
        let _ = summary;
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

/// Tells the phone what keeps dm busy, or that nothing does.
pub fn busy(summary: Option<Busy>) {
    if let Some(phone) = phone() {
        phone.busy(summary);
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
        nothing.busy(None);
        nothing.set_clipboard("x");
    }

    fn row(id: u64, state: dm_core::State, downloaded: u64, speed: f64) -> Arc<dm_core::Entry> {
        let row: dm_core::Entry = serde_json::from_value(serde_json::json!({
            "id": id,
            "url": "https://files.example/a.zip",
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": downloaded, "total": 100, "speed": speed, "eta": null},
            "filename": "a.zip",
        }))
        .unwrap();
        Arc::new(row)
    }

    /// Running, queued while due, or sharing keeps dm busy; queued rows the schedule holds,
    /// finished and stopped ones do not.
    #[test]
    fn what_keeps_a_phone_busy() {
        use dm_core::State;
        let rows = vec![
            row(1, State::Running, 10, 1000.0),
            row(2, State::Running, 100, 5.0),
            row(3, State::Queued, 0, 0.0),
            row(4, State::Completed, 100, 0.0),
            row(5, State::Paused, 3, 0.0),
        ];
        assert_eq!(
            Busy::of(&rows, true, false),
            Some(Busy {
                running: 1,
                finishing: 1,
                waiting: 1,
                seeding: false,
                bytes_per_second: 1000.0,
            })
        );
        assert_eq!(Busy::of(&rows, false, false).map(|b| b.waiting), Some(0));
        let queued = vec![row(3, State::Queued, 0, 0.0)];
        assert_eq!(Busy::of(&queued, false, false), None);
        assert_eq!(Busy::of(&queued, true, false).map(|b| b.waiting), Some(1));
        let done = vec![
            row(4, State::Completed, 100, 0.0),
            row(5, State::Paused, 3, 0.0),
        ];
        assert_eq!(Busy::of(&done, true, false), None);
        assert_eq!(
            Busy::of(&done, true, true),
            Some(Busy {
                seeding: true,
                ..Busy::default()
            })
        );
    }

    /// The notification's words are the status bar's, Exit's question's and the tray's.
    #[test]
    fn a_busy_phone_says_it_in_dms_words() {
        crate::i18n::set("en");
        let busy = |running, finishing, waiting, seeding| Busy {
            running,
            finishing,
            waiting,
            seeding,
            bytes_per_second: 2048.0,
        };
        let rate = crate::format::rate(2048.0);
        assert_eq!(
            busy(2, 0, 1, false).words(),
            (
                format!("2 downloading at {rate}"),
                Some("1 more is waiting to start.".into())
            )
        );
        assert_eq!(
            busy(1, 1, 0, true).words(),
            (format!("1 downloading at {rate}"), None)
        );
        assert_eq!(busy(0, 2, 0, false).words(), ("2 finishing".into(), None));
        assert_eq!(
            busy(0, 0, 3, false).words(),
            ("3 downloads are waiting to start.".into(), None)
        );
        assert_eq!(
            busy(0, 0, 0, true).words(),
            (crate::tray::words()[4].to_string(), None)
        );
    }
}
