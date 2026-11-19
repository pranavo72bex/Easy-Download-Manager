// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The application's tick, written once for whatever drives dm (gaps row A7): the orders
//! another dm left in the inbox, what the reads detected told (notifications, sounds), the
//! schedules, each download's own "When this download is done", and When done as the queue
//! runs dry. The window and dm with no window (`headless`) each call it after a read and carry
//! out what it asks ([`Effect`]), each its own way: the window opens a dialog and says it in
//! the language in force; with no window, what a person would be asked is settled with no one
//! asked, and what is said is said in English, for the control port.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_core::{Entry, Id};
use dm_shell::app::backend::Backend;
use dm_shell::app::detect::Detected;
use dm_shell::app::names::name_of;
use dm_shell::app::power::WhenDone;
use dm_shell::app::session::{Fired, ThisDone};

use crate::i18n::{t, tf};

/// The application's memory between ticks, for whatever drives dm: the application's own
/// session (`dm_shell::app::session::Session`, which this one also reaches as itself), with
/// what the reads since the last tick detected and whether the queue ran dry at one of them.
#[derive(Debug, Default)]
pub struct Session {
    /// The application's session: the reads, the hand-offs, When done and its countdown.
    app: dm_shell::app::session::Session,
    /// What the reads since the last tick detected ([`Session::note_read`]).
    detected: Vec<Detected>,
    /// The queue ran dry at a read ([`Session::note_dry`], looked at once per read, as
    /// `ran_dry` asks): When done's for the next tick. Looked at in the tick alone, a read
    /// between (after a command, an add, Download details) took the change, and When done
    /// never fired.
    dry: bool,
    /// When the finished downloads were last looked at for those to forget, and the days
    /// Options › Downloads said then ([`Session::forget_old`]).
    forgot: Option<(Instant, u32)>,
}

/// How often the finished downloads are looked at for those older than Options › Downloads'
/// "Forget finished downloads after … days": at start, once a day, and when the days change.
const FORGET_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

impl Deref for Session {
    type Target = dm_shell::app::session::Session;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl DerefMut for Session {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}

/// Where whoever drives the tick differs: the window, or dm with no window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Driver {
    /// The inbox's orders (`--new-download`, `--when-done`) are taken. In the window's tests,
    /// only by a window whose test holds the inbox, the home being every test's.
    pub reads_inbox: bool,
    /// Nobody sees a countdown: Exit when done ends dm at once.
    pub nobody_sees: bool,
}

impl Driver {
    /// dm with no window: the inbox read, and Exit when done at once.
    pub const HEADLESS: Driver = Driver {
        reads_inbox: true,
        nobody_sees: true,
    };

    /// The window, which takes the inbox's orders where `reads_inbox`.
    pub fn window(reads_inbox: bool) -> Driver {
        Driver {
            reads_inbox,
            nobody_sees: false,
        }
    }
}

/// A sound a tick plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sound {
    /// The sound file the person chose.
    File(String),
    /// The system's sound for something done.
    Chime,
    /// The system's sound for something gone wrong.
    Alert,
}

/// What a tick asks of whoever drives it, in the order it came to. Each is the driver's to
/// carry out, as only it can (a dialog, a sentence in its language), or the same for every
/// driver (a notification, a sound, a file opened: [`Effect::on_platform`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Another dm asked for a new download (`dm --new-download`): Add URL, where there is a
    /// window to add in.
    NewDownload,
    /// Another dm asked for this When done (`dm --when-done`), one this computer can do.
    WhenDone(WhenDone),
    /// Another dm asked for a When done this computer has no way to do: said.
    CannotWhenDone,
    /// What the reads detected is about to be told.
    Telling {
        /// The first download whose signed link ran out part-way, to be asked for a fresh one.
        expired: Option<Id>,
    },
    /// A download failed: told as a finish is, with why in plain words.
    Failed {
        /// The download.
        id: Id,
        /// Its name.
        name: String,
        /// Why, in plain words.
        why: String,
    },
    /// The one download a read found started elsewhere (a browser's hand-off, a second
    /// launch), where Options asks for each download's progress window.
    StartedElsewhere(Id),
    /// A hand-off's address was already listed, and was added again: said.
    AddedAgain {
        /// The download's name.
        name: String,
    },
    /// The downloads that finished, in the order read.
    Finished(Vec<Id>),
    /// What was told, for a screen reader to hear too.
    Told {
        /// The names of the downloads that finished.
        finished: Vec<String>,
        /// The name of each download that failed, with why.
        failures: Vec<(String, String)>,
    },
    /// A notification.
    Notify {
        /// Its title.
        title: &'static str,
        /// What it says.
        body: String,
    },
    /// A notification of one finished file, which a click on it opens.
    NotifyFile {
        /// Its title.
        title: &'static str,
        /// The file's name.
        name: String,
        /// Where the file is.
        path: String,
    },
    /// A sound played.
    Play(Sound),
    /// A problem met, for the list of problems.
    Problem(String),
    /// The schedule armed this When done: said.
    Armed(WhenDone),
    /// The schedules were looked at.
    Scheduled {
        /// The scheduled downloads are over and a schedule asks for the dial-up connection to
        /// be hung up.
        hang_up: bool,
    },
    /// A download's own "When this download is done" opens its file.
    Open(String),
    /// A download's own "When this download is done" opens its folder, the file chosen there.
    Reveal(String),
    /// dm ends now; the tick asks nothing after it.
    Quit,
}

impl Effect {
    /// Carries out on `backend`'s platform what is the same whoever drives dm: a notification,
    /// a sound, a file opened or shown in its folder. `None` for any other effect, which is the
    /// driver's own; a file that would not open is `Some(Err)`, a problem.
    pub fn on_platform(&self, backend: &Backend) -> Option<Result<(), String>> {
        let platform = &backend.platform;
        match self {
            Effect::Notify { title, body } => platform.notify(title, body),
            Effect::NotifyFile { title, name, path } => platform.notify_file(title, name, path),
            Effect::Play(Sound::File(path)) => platform.play_file(path),
            Effect::Play(Sound::Chime) => platform.chime(),
            Effect::Play(Sound::Alert) => platform.alert(),
            Effect::Open(path) => return Some(platform.open(path)),
            Effect::Reveal(path) => return Some(platform.reveal(path)),
            _ => return None,
        }
        Some(Ok(()))
    }
}

impl Session {
    /// What a read detected (`Read::detected`), kept for the next tick to tell.
    pub fn note_read(&mut self, detected: Detected) {
        self.detected.push(detected);
    }

    /// Whether the queue has just run dry, looked at once per read, after it, with its `rows`
    /// (`ran_dry`): kept for the next tick's When done. Every driver calls it at each of its
    /// reads, the window's and headless's alike (gaps row A7): looked at in the tick alone, a
    /// read between (a command over the control port, a hand-off) took the change.
    pub fn note_dry(&mut self, rows: &[Arc<Entry>]) {
        if self.app.ran_dry(rows) {
            self.dry = true;
        }
    }

    /// The orders another dm left in the inbox, each taken once, where the driver
    /// [`Driver::reads_inbox`]: a new download asked for, and a When done, said where this
    /// computer has no way to do it. Before [`Session::tick`], so that a When done asked for
    /// is in force when the queue running dry takes it.
    pub fn take_orders(&mut self, driver: Driver) -> Vec<Effect> {
        let home = dm_shell::settings::home();
        let mut effects = Vec::new();
        if !driver.reads_inbox {
            return effects;
        }
        if dm_shell::app::session::new_download_asked(&home) {
            effects.push(Effect::NewDownload);
        }
        if let Some(wanted) = dm_shell::app::session::when_done_asked(&home) {
            // As the menu greys them: only what this computer can do.
            effects.push(if wanted.possible(crate::power::available) {
                Effect::WhenDone(wanted)
            } else {
                Effect::CannotWhenDone
            });
        }
        effects
    }

    /// One tick's shared work, after a read and [`Session::take_orders`], over the last read's
    /// `rows`: what the reads detected told, the schedules, each download's own "When this
    /// download is done", and the queue run dry. What it comes to, in order, for the driver to
    /// carry out; [`Effect::Quit`] last where dm is to end now.
    pub fn tick(&mut self, backend: &Backend, rows: &[Arc<Entry>], driver: Driver) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.told(backend, rows, &mut effects);
        self.schedules(backend, rows, &mut effects);
        self.this_done(rows, &mut effects);
        self.forget_old(backend);
        if self.queue_dry(backend, driver, &mut effects) {
            effects.push(Effect::Quit);
        }
        effects
    }

    /// What the reads since the last tick detected: a notification for each download that
    /// failed, one for what finished (a batch is one sentence; a single file keeps its Open),
    /// the sounds chosen, and a hand-off's address already listed said.
    fn told(&mut self, backend: &Backend, rows: &[Arc<Entry>], effects: &mut Vec<Effect>) {
        let detected = std::mem::take(&mut self.detected);
        let settings = backend.settings();
        let expired = detected
            .iter()
            .find_map(|read| read.expired.first().copied());
        effects.push(Effect::Telling { expired });
        let mut finished: Vec<(String, String)> = Vec::new();
        let mut finished_ids: Vec<Id> = Vec::new();
        // Each failure's name and why, said aloud too (gaps row U23).
        let mut failures: Vec<(String, String)> = Vec::new();
        for read in detected {
            for row in rows {
                if read.failed.contains(&row.id) {
                    let (name, why) = (name_of(row), crate::rows::status_of(row));
                    effects.push(Effect::Failed {
                        id: row.id,
                        name: name.clone(),
                        why: why.clone(),
                    });
                    failures.push((name, why));
                }
                if read.finished.contains(&row.id) {
                    finished.push((name_of(row), row.path.clone()));
                    finished_ids.push(row.id);
                }
            }
            // One window for one download, not a stack for a batch.
            if settings.general.show_progress {
                if let [only] = read.started_elsewhere.as_slice() {
                    effects.push(Effect::StartedElsewhere(*only));
                }
            }
            if let Some(row) = read
                .added_again
                .and_then(|id| rows.iter().find(|r| r.id == id))
            {
                effects.push(Effect::AddedAgain { name: name_of(row) });
            }
        }
        if !finished_ids.is_empty() {
            effects.push(Effect::Finished(finished_ids));
        }
        let failed = !failures.is_empty();
        effects.push(Effect::Told {
            finished: finished.iter().map(|(name, _)| name.clone()).collect(),
            failures,
        });
        match finished.as_slice() {
            [] => {}
            [(name, path)] => effects.push(Effect::NotifyFile {
                title: t("Download finished"),
                name: name.clone(),
                path: path.clone(),
            }),
            many => effects.push(Effect::Notify {
                title: t("Download finished"),
                body: tf("{} downloads finished.", &[&many.len()]),
            }),
        }
        if settings.general.sound_on_complete && !finished.is_empty() {
            let sound = chosen_sound(&settings, "complete").map_or(Sound::Chime, Sound::File);
            effects.push(Effect::Play(sound));
        }
        if settings.general.sound_on_failure && failed {
            let sound = chosen_sound(&settings, "failed").map_or(Sound::Alert, Sound::File);
            effects.push(Effect::Play(sound));
        }
    }

    /// The schedules, after every read (`schedule_tick`): a once-only date that has passed is
    /// put away, and what the schedule armed for When done is said.
    fn schedules(&mut self, backend: &Backend, rows: &[Arc<Entry>], effects: &mut Vec<Effect>) {
        let settings = backend.settings();
        if let Some(wanted) = crate::held::spent_once_now(&settings) {
            if let Err(why) = backend.set_settings(wanted) {
                effects.push(Effect::Problem(why));
            }
        }
        let settings = backend.settings();
        let tick = self
            .app
            .schedule_tick(backend, &settings, rows, crate::power::available);
        if let Some(wanted) = tick.armed {
            effects.push(Effect::Armed(wanted));
        }
        effects.extend(tick.problems.into_iter().map(Effect::Problem));
        effects.push(Effect::Scheduled {
            hang_up: tick.hang_up,
        });
    }

    /// Each download whose own "When this download is done" is due (gaps row U36): its file or
    /// folder opened, or exit, sleep, hibernate or turn off through When done's own countdown,
    /// which can be cancelled.
    fn this_done(&mut self, rows: &[Arc<Entry>], effects: &mut Vec<Effect>) {
        for (row, choice) in self.app.this_done_due(rows) {
            match choice {
                ThisDone::Nothing => {}
                ThisDone::Open => effects.push(Effect::Open(row.path)),
                ThisDone::OpenFolder => effects.push(Effect::Reveal(row.path)),
                ThisDone::Then(action) => {
                    let at =
                        Instant::now() + Duration::from_secs(dm_shell::app::power::COUNTDOWN_SECS);
                    self.app.countdown = Some((action, at));
                }
            }
        }
    }

    /// The finished downloads older than Options › Downloads' "Forget finished downloads after
    /// … days" taken off the list, their files left (gaps row W17): at the first tick, once a
    /// day after it, and at once when the days are changed. 0 days forgets nothing.
    fn forget_old(&mut self, backend: &Backend) {
        let days = backend.settings().downloads.forget_after_days;
        let now = Instant::now();
        let due = self
            .forgot
            .is_none_or(|(at, then)| then != days || now.duration_since(at) >= FORGET_EVERY);
        if !due {
            return;
        }
        self.forgot = Some((now, days));
        if days > 0 {
            backend.forget_finished(days);
        }
    }

    /// A queue that has run dry has finished: its own sound, where the person chose a file,
    /// and When done (`when_done_fires`). Whether dm is to end now.
    fn queue_dry(&mut self, backend: &Backend, driver: Driver, effects: &mut Vec<Effect>) -> bool {
        // Noted at each read (`note_dry`), the last before this tick included.
        if !std::mem::take(&mut self.dry) {
            return false;
        }
        if let Some(path) = chosen_sound(&backend.settings(), "queue_done") {
            effects.push(Effect::Play(Sound::File(path)));
        }
        self.app.when_done_fires(driver.nobody_sees, Instant::now()) == Fired::QuitNow
    }
}

/// The sound file the person chose for `event`, if one was.
fn chosen_sound(settings: &dm_shell::Settings, event: &str) -> Option<String> {
    settings
        .sounds
        .get(event)
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

#[cfg(test)]
mod tests;
