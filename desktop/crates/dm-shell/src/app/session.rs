// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The application's work over time, apart from any window: what is due at a
//! tick (files whose "Delete file after" has come, and, as they move here, the
//! other timers), done by the application and reported, so each presentation
//! only reacts (forgets a row's dialogs, says what could not be done).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dm_core::{Entry, Id, State};

use super::backend::Backend;
use super::detect::{Detected, Detector, DryWatch};
use super::power::WhenDone;
use super::schedule::{looked_after, work_left, AtEnd, Step, Watch};
use super::snapshot::{Changes, Snapshot};
use super::watch::Watched;
use crate::Settings;

/// The application's memory between ticks, for whichever presentation drives
/// it (the window, or a loop with no window): what was told, and the watched
/// folder's. It does not own the `Backend`, which the presentation holds; its
/// methods are handed it.
#[derive(Debug, Default)]
pub struct Session {
    detector: Detector,
    dry: DryWatch,
    /// The rows of the last read, held so that what the next read changed
    /// is found by their pointers (`Changes`): held, none of them is changed
    /// in place by the queue (gaps row 181).
    rows: Vec<Arc<Entry>>,
    watched: Watched,
    /// Downloads the browser handed over: a moment ago, and a row waiting
    /// for a fresh link (`handoff::Hands`).
    pub hands: super::handoff::Hands,
    /// The speed limit last put in force (bytes a second; `None` before the
    /// first tick, or once a presentation asks for it to be put in again).
    limit_in_force: Option<u64>,
    /// The schedule for every queue: its window, its rounds, what it armed.
    schedule_watch: Watch,
    /// Each named queue with a schedule of its own.
    queue_watches: BTreeMap<String, Watch>,
    /// What dm does when the queue runs dry (Downloads › When done), for
    /// this session: the person's choice, or the schedule's armed for them.
    pub when_done: WhenDone,
    /// An action waiting out its countdown: what, and when it is due.
    pub countdown: Option<(WhenDone, Instant)>,
    /// What dm does when each of these downloads is done, chosen in its progress window
    /// (gaps row U36).
    pub this_done: HashMap<Id, ThisDone>,
    /// The typed events of each read, for any number of subscribers
    /// (`events::Events::subscribe`).
    pub events: super::events::Events,
    /// Rows being added off the presentation's thread (`Self::adding`).
    adds: Arc<Mutex<Adds>>,
    /// What is running, written for the browser's toolbar popup (`going.json`, gaps row U18).
    going: super::going::Publisher,
}

/// What dm does when one download is done, chosen in its own progress window (gaps row U36):
/// nothing, open the file or its folder, or what Downloads › When done does for every download.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThisDone {
    /// Nothing.
    #[default]
    Nothing,
    /// The file opened.
    Open,
    /// Its folder opened, the file chosen there.
    OpenFolder,
    /// Exit, sleep, hibernate or turn off, through When done's own countdown.
    Then(WhenDone),
}

impl ThisDone {
    /// Every choice, in the order the progress window lists them.
    pub const ALL: [ThisDone; 7] = [
        ThisDone::Nothing,
        ThisDone::Open,
        ThisDone::OpenFolder,
        ThisDone::Then(WhenDone::Quit),
        ThisDone::Then(WhenDone::Sleep),
        ThisDone::Then(WhenDone::Hibernate),
        ThisDone::Then(WhenDone::ShutDown),
    ];

    /// Its words in English, as the menus name the same things.
    pub fn english(self) -> &'static str {
        match self {
            ThisDone::Nothing | ThisDone::Then(WhenDone::Nothing) => "Do nothing",
            ThisDone::Open => "Open",
            ThisDone::OpenFolder => "Open folder",
            ThisDone::Then(WhenDone::Quit) => "Exit dm",
            ThisDone::Then(WhenDone::Sleep) => "Sleep",
            ThisDone::Then(WhenDone::Hibernate) => "Hibernate",
            ThisDone::Then(WhenDone::ShutDown) => "Turn off computer",
        }
    }
}

impl Session {
    /// The downloads whose own "When this download is done" is due: each finished one, with its
    /// choice, taken once. A choice for a download that failed, was cancelled or left the list
    /// is forgotten.
    pub fn this_done_due(&mut self, rows: &[Arc<Entry>]) -> Vec<(Entry, ThisDone)> {
        let mut due = Vec::new();
        self.this_done
            .retain(|id, choice| match rows.iter().find(|r| r.id == *id) {
                Some(row) if row.state == State::Completed => {
                    due.push(((**row).clone(), *choice));
                    false
                }
                Some(row) => !matches!(row.state, State::Failed(_) | State::Cancelled),
                None => false,
            });
        due
    }
}

#[cfg(test)]
mod this_done_tests {
    use super::*;

    fn row(id: Id, state: State) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": id, "url": "https://e.com/f.bin",
                "state": serde_json::to_value(&state).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "f.bin",
            }))
            .unwrap(),
        )
    }

    /// A download's own choice is due once, when it finishes; a failed or gone one's is
    /// forgotten, a running one's kept (gaps row U36).
    #[test]
    fn a_downloads_own_choice_is_due_once_when_it_finishes() {
        let mut session = Session::default();
        session.this_done.insert(1, ThisDone::Open);
        session.this_done.insert(2, ThisDone::Then(WhenDone::Sleep));
        session.this_done.insert(3, ThisDone::OpenFolder);
        session.this_done.insert(4, ThisDone::Open);
        let rows = vec![
            row(1, State::Running),
            row(2, State::Failed("x".into())),
            row(3, State::Completed),
        ];
        let due = session.this_done_due(&rows);
        assert_eq!(due.len(), 1);
        assert_eq!((due[0].0.id, due[0].1), (3, ThisDone::OpenFolder));
        assert_eq!(
            session.this_done.keys().copied().collect::<Vec<_>>(),
            vec![1]
        );
        let rows = vec![row(1, State::Completed)];
        assert_eq!(session.this_done_due(&rows).len(), 1);
        assert!(session.this_done_due(&rows).is_empty(), "once");
    }
}

/// How many addings run off the presentation's thread, and the ids of rows
/// added by those done, not yet marked as the presentation's own.
#[derive(Debug, Default)]
struct Adds {
    running: usize,
    added: Vec<Id>,
}

/// Rows being added off the presentation's thread (Import's errand), begun
/// with [`Session::adding`] on that thread. Until [`Adding::added`] says
/// their ids, the new rows a read finds are held unjudged rather than taken
/// for rows started elsewhere (a browser's); dropped without saying, its
/// rows are then judged as any new row.
#[derive(Debug)]
pub struct Adding {
    adds: Arc<Mutex<Adds>>,
    open: bool,
}

impl Adding {
    /// The rows this adding added: the presentation's own from the next
    /// read on.
    pub fn added(mut self, ids: &[Id]) {
        let mut adds = dm_core::lock(&self.adds);
        // In one step with the end of the adding: a read sees either both
        // or neither.
        adds.added.extend_from_slice(ids);
        adds.running = adds.running.saturating_sub(1);
        self.open = false;
    }
}

impl Drop for Adding {
    fn drop(&mut self) {
        if self.open {
            let mut adds = dm_core::lock(&self.adds);
            adds.running = adds.running.saturating_sub(1);
        }
    }
}

/// What the queue running dry did with When done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fired {
    /// Nothing was chosen.
    Nothing,
    /// Quit at once: nobody is there to see a countdown.
    QuitNow,
    /// The action waits out its countdown (`Session::countdown`).
    Counting,
}

/// What a tick of the schedules came to, for the presentation to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleTick {
    /// The schedule's choice, armed in this tick: to be said.
    pub armed: Option<WhenDone>,
    /// The scheduled downloads are over and a schedule asks the dial-up
    /// connection to be hung up (Windows).
    pub hang_up: bool,
    /// Problems met (a program to run at the end that could not start).
    pub problems: Vec<String>,
}

/// One read of the queue: what to draw from, what happened since the last
/// read, and problems met doing what follows from it.
#[derive(Clone, Default)]
pub struct Read {
    /// The rows and what each line says beside its state.
    pub snapshot: Snapshot,
    /// What happened since the last read.
    pub detected: Detected,
    /// Problems met (a command run after a download that could not start).
    pub problems: Vec<String>,
    /// Which rows are not the entries the last read had: a presentation
    /// that keeps the last read's rows looks at only those (gaps row 181).
    pub changes: Changes,
}

impl Session {
    /// Reads the queue and does what follows from it that is the
    /// application's: the command a person set to run on each finished file
    /// (Options › Downloads). Call `changes::take` before, never after, so a
    /// change made during the read is still owed one.
    pub fn read(&mut self, backend: &Backend, settings: &Settings) -> Read {
        // Asked before the queue is read: an adding begins only on this
        // thread (`Self::adding` takes the session), so none begins between,
        // and a row of one in the read is either held or among the ids it
        // said. Asked after, an adding that ended between could have said
        // ids the read has not got yet.
        let hold_new = {
            let mut adds = dm_core::lock(&self.adds);
            for id in adds.added.drain(..) {
                self.detector.own(id);
            }
            adds.running > 0
        };
        self.read_from(backend.snapshot(), hold_new, settings)
    }

    /// [`Self::read`] of `snapshot`, the queue read with new rows held
    /// unjudged where `hold_new`.
    fn read_from(&mut self, snapshot: Snapshot, hold_new: bool, settings: &Settings) -> Read {
        // What changed, found once by pointer and followed by each of what
        // comes next: at 10,000 rows with a few downloads running, each of
        // them had looked every row up at every read (gaps row 181).
        let changes = Changes::between(&self.rows, &snapshot.rows);
        self.rows.clone_from(&snapshot.rows);
        let detected = self
            .detector
            .read_changes(&snapshot.rows, hold_new, &changes);
        self.dry.follow(&snapshot.rows, &changes);
        self.events.publish_changes(&snapshot.rows, &changes);
        let home = crate::settings::home();
        self.going
            .publish(&home, &snapshot.rows, &changes, &detected.failed, || {
                crate::host::browser_seen(&home)
            });
        let mut problems = Vec::new();
        let command = settings.on_complete.trim();
        if !command.is_empty() && !detected.finished.is_empty() {
            for row in snapshot
                .rows
                .iter()
                .filter(|r| detected.finished.contains(&r.id) && !r.path.is_empty())
            {
                if let Err(why) = crate::shell::run_after(command, std::path::Path::new(&row.path))
                {
                    problems.push(why);
                }
            }
        }
        Read {
            snapshot,
            detected,
            problems,
            changes,
        }
    }

    /// A row the presentation added itself: not "started elsewhere".
    pub fn own(&mut self, id: Id) {
        self.detector.own(id);
    }

    /// Rows about to be added off this thread (Import, on the window's
    /// errand): the reads made meanwhile hold the new rows they find until
    /// the adding says which are its own ([`Adding::added`]), so none of
    /// them is taken for a row started elsewhere (gaps row 183).
    pub fn adding(&mut self) -> Adding {
        dm_core::lock(&self.adds).running += 1;
        Adding {
            adds: Arc::clone(&self.adds),
            open: true,
        }
    }

    /// Adds each address with its options (`Backend::add_each`, the queue
    /// written once), each row added marked as this presentation's own.
    pub fn add(&mut self, backend: &Backend, items: Vec<(String, dm_core::AddOptions)>) -> Added {
        let asked: Vec<(String, bool)> = items.iter().map(|(u, w)| (u.clone(), w.later)).collect();
        let mut done = Added::default();
        for ((url, later), outcome) in asked.into_iter().zip(backend.add_each(items)) {
            match outcome {
                Ok(id) => {
                    self.own(id);
                    done.rows.push((id, url, later));
                }
                Err(e) => {
                    done.failed += 1;
                    done.error = Some(e);
                }
            }
        }
        done
    }

    /// The row `id`'s state at the read before the latest, none where it
    /// was not listed then (When done's dry-queue watch compares the latest
    /// rows with it).
    pub fn before(&self, id: Id) -> Option<&State> {
        self.detector.before(id)
    }

    /// Whether the queue has just run dry since the read before (When done:
    /// the sound and the action it arms are the presentation's to start).
    /// Call once per read, after it, with its rows (as read, or as shown):
    /// worked out from the rows whose state the read changed, not from
    /// every row (gaps row 181).
    pub fn ran_dry(&mut self, rows: &[Arc<Entry>]) -> bool {
        if rows.len() != self.rows.len() {
            // Not the read's rows: compared with the read before, every one.
            return self.dry.look(rows, &self.detector.before_all());
        }
        self.dry.look_moved(self.detector.moved())
    }

    /// The speed limit last put in force, in bytes a second.
    pub fn limit_in_force(&self) -> Option<u64> {
        self.limit_in_force
    }

    /// Puts the speed limit in again at the next tick (the schedule's slow
    /// hours were changed).
    pub fn forget_limit(&mut self) {
        self.limit_in_force = None;
    }

    /// One tick of the schedules, after a read: the slow hours' limit in or
    /// out; the schedule's window opening (arming its When done, only for
    /// something waiting to run and never over a choice the person made this
    /// session) or closing (putting an unused choice away); and, for the
    /// schedule for every queue and each queue's own, the end of its window:
    /// its failed downloads tried again for as many rounds as allowed, then
    /// its program run and, where asked, the dial-up connection hung up.
    /// It acts on the session's When done; `can_do` says whether
    /// this computer can carry out an action (the presentation knows).
    pub fn schedule_tick(
        &mut self,
        backend: &Backend,
        settings: &Settings,
        rows: &[Arc<Entry>],
        can_do: impl Fn(WhenDone) -> bool,
    ) -> ScheduleTick {
        let mut tick = ScheduleTick {
            armed: None,
            hang_up: false,
            problems: Vec::new(),
        };
        // The slow hours' limit in, or out, as they begin or end.
        let limit = settings.speed_limit_bytes_now();
        if self.limit_in_force != Some(limit) {
            backend.set_speed_limit(limit);
            self.limit_in_force = Some(limit);
        }
        let schedule = settings.schedule.clone();
        let open = schedule.enabled && schedule.open();
        match self.schedule_watch.step(open) {
            Step::Opened => {
                let wanted = WhenDone::from_word(&schedule.when_done);
                let can = wanted == WhenDone::Nothing || wanted == WhenDone::Quit || can_do(wanted);
                if wanted != WhenDone::Nothing
                    && can
                    && self.when_done == WhenDone::Nothing
                    && work_left(rows)
                {
                    self.when_done = wanted;
                    tick.armed = Some(wanted);
                    self.schedule_watch.arm(wanted);
                }
            }
            Step::Closed => {
                // Unused: put away, unless it was changed in the menu since.
                if let Some(armed) = self.schedule_watch.disarm() {
                    if self.when_done == armed {
                        self.when_done = WhenDone::Nothing;
                    }
                }
            }
            Step::Nothing => {}
        }
        // Fired (the choice was taken as the queue ran dry): nothing to put away.
        if self.when_done == WhenDone::Nothing {
            self.schedule_watch.disarm();
        }
        // The rows the schedule for every queue looks after: those of a queue
        // with no schedule of its own (all of them, unless one has).
        let own = settings.queue_schedules.clone();
        let shared = looked_after(rows, &own, None);
        end_of_window(
            backend,
            &schedule,
            open,
            &shared,
            &mut self.schedule_watch,
            &mut tick,
        );
        // Each named queue with a schedule of its own: its program at the end
        // and its tries again, over its own rows. (When done stays the one for
        // every queue: it acts on the computer.)
        self.queue_watches.retain(|name, _| own.contains_key(name));
        for (name, schedule) in &own {
            let open = schedule.enabled && schedule.open();
            let watch = self.queue_watches.entry(name.clone()).or_default();
            watch.step(open);
            let mine = looked_after(rows, &own, Some(name));
            end_of_window(backend, schedule, open, &mine, watch, &mut tick);
        }
        tick
    }

    /// The queue has run dry (`ran_dry`): the session's When done is taken,
    /// once only. Quit is at once where `nobody_sees` a countdown (no
    /// window); anything else, Quit included, waits out its countdown,
    /// [`COUNTDOWN_SECS`](super::power::COUNTDOWN_SECS) from `now`.
    pub fn when_done_fires(&mut self, nobody_sees: bool, now: Instant) -> Fired {
        match std::mem::take(&mut self.when_done) {
            WhenDone::Nothing => Fired::Nothing,
            WhenDone::Quit if nobody_sees => Fired::QuitNow,
            action => {
                let at = now + Duration::from_secs(super::power::COUNTDOWN_SECS);
                self.countdown = Some((action, at));
                Fired::Counting
            }
        }
    }

    /// A countdown whose time is up by `now` is ended and its action carried
    /// out (`carry_out`): what came of it, or `None` while nothing is due.
    pub fn countdown_due(
        &mut self,
        backend: &Backend,
        now: Instant,
    ) -> Option<Result<Carried, String>> {
        let (action, at) = self.countdown?;
        if now < at {
            return None;
        }
        self.countdown = None;
        Some(carry_out(backend, action))
    }

    /// The watched folder's look (`watch::Watched::look`).
    pub fn look_at_watched(
        &mut self,
        folder: &str,
        rows: &[Arc<Entry>],
        backend: &Backend,
    ) -> Vec<Vec<String>> {
        self.watched.look(folder, rows, backend)
    }
}

/// What a tick of "Delete file after" did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeletedDue {
    /// Rows removed: their file moved to the Trash, or already gone.
    pub gone: Vec<Id>,
    /// Files that could not be moved, each with why; their rows stay, and are
    /// not tried again.
    pub failed: Vec<String>,
}

/// "Delete file after": each finished row whose time has come at `now`
/// (seconds since the epoch) has its file moved to the Trash and is removed.
/// Only its own file: one moved away, or since written by another row, is not
/// this row's to delete. A finished torrent still shared lets go of its file
/// first. `None` when nothing was due.
pub fn delete_due(backend: &Backend, rows: &[Arc<Entry>], now: u64) -> Option<DeletedDue> {
    let due: Vec<&Arc<Entry>> = rows
        .iter()
        .filter(|r| r.delete_due_in(now) == Some(0))
        .collect();
    if due.is_empty() {
        return None;
    }
    let superseded = super::rules::superseded(rows);
    let mut done = DeletedDue::default();
    for row in due {
        let present = !row.path.is_empty() && !super::rules::file_gone(row, &superseded);
        match present.then(|| {
            dm_core::stop_sharing(std::path::Path::new(&row.path));
            backend.platform.trash(&row.path)
        }) {
            Some(Err(e)) => {
                done.failed.push(format!("{}: {e}", row.path));
                backend.set_delete_after(row.id, None);
            }
            _ => done.gone.push(row.id),
        }
    }
    if !done.gone.is_empty() {
        backend.remove_many(&done.gone);
    }
    Some(done)
}

/// The When done another dm asked for (`dm --when-done <word>`), left in
/// the inbox under `home`: taken, so it is acted on once. A word dm does not
/// know is dropped.
pub fn when_done_asked(home: &std::path::Path) -> Option<WhenDone> {
    let file = crate::host::inbox(home).join(crate::shell::WHEN_DONE_FILE);
    let word = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    WhenDone::ALL.into_iter().find(|w| w.word() == word.trim())
}

/// Whether another dm asked for a new download (`dm --new-download`, the
/// jump list's Add URL), left in the inbox under `home`: taken, so Add URL
/// opens once.
pub fn new_download_asked(home: &std::path::Path) -> bool {
    std::fs::remove_file(crate::host::inbox(home).join(crate::shell::NEW_DOWNLOAD_FILE)).is_ok()
}

/// What [`Session::add`] came to.
#[derive(Debug, Default)]
pub struct Added {
    /// Each row added: its id, the address and whether it was added for
    /// later, in the order asked.
    pub rows: Vec<(Id, String, bool)>,
    /// How many were not added.
    pub failed: usize,
    /// Why the last of those was not.
    pub error: Option<String>,
}

impl Added {
    /// How many were added; the error when none was and one was refused.
    /// Some added and some not is still how many: the presentation says the
    /// rest ([`Self::partly`]).
    pub fn outcome(&self) -> Result<usize, String> {
        match (&self.error, self.rows.len()) {
            (Some(e), 0) => Err(e.clone()),
            (_, n) => Ok(n),
        }
    }

    /// How many were refused, and why the last was, when others went in.
    pub fn partly(&self) -> Option<(usize, &str)> {
        match &self.error {
            Some(e) if !self.rows.is_empty() => Some((self.failed, e.as_str())),
            _ => None,
        }
    }
}

/// What carrying out a When done action that came due leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carried {
    /// dm closes: the presentation does it, as only it can close itself.
    Quit,
    /// The computer was asked (to sleep, hibernate or shut down).
    Asked,
}

/// Carries out `action` once its countdown is over. Quit is the
/// presentation's to do; anything else is asked of the computer through the
/// platform, and a refusal that comes back later goes on `backend`'s problems,
/// as anything running apart from the window does: whoever set it is likely
/// away. An `Err` is a refusal said at once.
pub fn carry_out(backend: &Backend, action: super::power::WhenDone) -> Result<Carried, String> {
    if action == super::power::WhenDone::Quit {
        return Ok(Carried::Quit);
    }
    let notices = backend.notices();
    let report: super::power::Report = Box::new(move |why| dm_core::lock(&notices).push(why));
    backend
        .platform
        .power(action, report)
        .map(|()| Carried::Asked)
}

/// The end of one schedule's window over its `rows`: its failed downloads tried
/// again while rounds are left, then its program, and the hang-up it asks for.
fn end_of_window(
    backend: &Backend,
    schedule: &dm_core::Schedule,
    open: bool,
    rows: &[Arc<Entry>],
    watch: &mut Watch,
    tick: &mut ScheduleTick,
) {
    let failed: Vec<Id> = rows
        .iter()
        .filter(|r| matches!(r.state, State::Failed(_)))
        .map(|r| r.id)
        .collect();
    match watch.at_end(open, work_left(rows), !failed.is_empty(), schedule.retries) {
        AtEnd::Retry => {
            for id in failed {
                backend.retry(id);
            }
        }
        AtEnd::Done => {
            let command = schedule.run_when_done.trim();
            if !command.is_empty() {
                if let Err(why) = crate::shell::run_program(command) {
                    tick.problems.push(why);
                }
            }
            if schedule.hang_up {
                tick.hang_up = true;
            }
        }
        AtEnd::Nothing => {}
    }
}

#[cfg(test)]
mod when_done_tests {
    use super::*;

    #[test]
    fn the_queue_running_dry_takes_when_done_once() {
        let now = Instant::now();
        let mut session = Session::default();
        assert_eq!(session.when_done_fires(false, now), Fired::Nothing);
        assert!(session.countdown.is_none());

        // Exit with nobody to see a countdown: at once, and taken.
        session.when_done = WhenDone::Quit;
        assert_eq!(session.when_done_fires(true, now), Fired::QuitNow);
        assert_eq!(session.when_done, WhenDone::Nothing);
        assert!(session.countdown.is_none());

        // With a window, Exit waits out the countdown as the others do.
        for action in [WhenDone::Quit, WhenDone::Sleep, WhenDone::ShutDown] {
            session.when_done = action;
            assert_eq!(session.when_done_fires(false, now), Fired::Counting);
            assert_eq!(session.when_done, WhenDone::Nothing, "once only");
            let at = now + Duration::from_secs(super::super::power::COUNTDOWN_SECS);
            assert_eq!(session.countdown, Some((action, at)));
        }
    }

    #[test]
    fn a_countdown_is_carried_out_only_once_its_time_is_up() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let state = tempfile::tempdir().unwrap();
        let backend = crate::app::backend::testing::backend(&runtime, state.path());
        let mut session = Session::default();
        let now = Instant::now();
        assert!(session.countdown_due(&backend, now).is_none(), "none set");

        session.countdown = Some((WhenDone::Sleep, now + Duration::from_secs(5)));
        assert!(session.countdown_due(&backend, now).is_none(), "not yet");
        assert!(session.countdown.is_some());
        let due = now + Duration::from_secs(5);
        assert_eq!(
            session.countdown_due(&backend, due),
            Some(Ok(Carried::Asked))
        );
        assert!(session.countdown.is_none());

        session.countdown = Some((WhenDone::Quit, now));
        assert_eq!(
            session.countdown_due(&backend, now),
            Some(Ok(Carried::Quit))
        );
        assert!(session.countdown_due(&backend, now).is_none(), "ended");
    }

    #[test]
    fn a_when_done_another_dm_asked_for_is_taken_once() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(when_done_asked(home.path()), None, "nothing asked");
        let inbox = crate::host::inbox(home.path());
        std::fs::create_dir_all(&inbox).unwrap();
        let file = inbox.join(crate::shell::WHEN_DONE_FILE);
        for action in WhenDone::ALL {
            std::fs::write(&file, format!("{}\n", action.word())).unwrap();
            assert_eq!(when_done_asked(home.path()), Some(action));
            assert!(!file.exists(), "taken");
        }
        std::fs::write(&file, "dance").unwrap();
        assert_eq!(when_done_asked(home.path()), None, "unknown word");
        assert!(!file.exists(), "dropped");
    }

    #[test]
    fn a_new_download_another_dm_asked_for_is_taken_once() {
        let home = tempfile::tempdir().unwrap();
        assert!(!new_download_asked(home.path()), "nothing asked");
        let inbox = crate::host::inbox(home.path());
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join(crate::shell::NEW_DOWNLOAD_FILE), "").unwrap();
        assert!(new_download_asked(home.path()));
        assert!(!new_download_asked(home.path()), "taken");
    }
}

#[cfg(test)]
mod schedule_tick_tests;

/// The application layer driven by a presentation with no window at all, as
/// a future front end would: a hand-off added, the queue read until it runs
/// dry, When done counted down and carried out, and the control port's
/// answer, with nothing from any window toolkit.
#[cfg(test)]
mod no_window_tests;

/// What the application's work at each tick costs on a large queue (the
/// architecture audit's Phase 6). Ignored by default; run in release:
/// `cargo test -p dm-shell --release --lib large_queue -- --ignored --nocapture`.
#[cfg(test)]
mod large_queue {
    use super::*;

    fn time(what: &str, rows: usize, runs: u32, mut f: impl FnMut()) -> Duration {
        f();
        let start = Instant::now();
        for _ in 0..runs {
            f();
        }
        let each = start.elapsed() / runs;
        println!("{rows:>6} rows  {what:<28} {each:>10.2?}");
        each
    }

    #[test]
    #[ignore = "a measure, run by hand in release"]
    fn the_application_tick_stays_cheap_on_a_large_queue() {
        for size in [1_000usize, 10_000] {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let state = tempfile::tempdir().unwrap();
            let backend = crate::app::backend::testing::backend(&runtime, state.path());
            let items = (0..size)
                .map(|i| {
                    let with = dm_core::AddOptions {
                        later: true,
                        ..Default::default()
                    };
                    (format!("http://127.0.0.1:9/{i}.bin"), with)
                })
                .collect();
            assert!(backend.add_each(items).iter().all(Result::is_ok));
            let settings = backend.settings();
            let mut session = Session::default();
            let mut rows = Vec::new();
            let read = time("Session::read", size, 50, || {
                let read = session.read(&backend, &settings);
                rows = read.snapshot.rows;
            });
            assert_eq!(rows.len(), size);
            let dry = time("Session::ran_dry", size, 50, || {
                session.ran_dry(&rows);
            });
            let tick = time("Session::schedule_tick", size, 50, || {
                session.schedule_tick(&backend, &settings, &rows, |_| true);
            });
            time("rules::superseded", size, 50, || {
                super::super::rules::superseded(&rows);
            });
            time("control_api queue.list", size, 10, || {
                super::super::control_api::queue_list(&rows);
            });
            // Four times a second while something moves: the read, the dry
            // look and the schedules' tick stay well inside a frame.
            let per_tick = read + dry + tick;
            println!("{size:>6} rows  per tick (read+dry+schedule)  {per_tick:>10.2?}");
            assert!(per_tick < Duration::from_millis(50), "{per_tick:?}");
        }
    }
}

/// The application under load: a queue of `size` rows, several downloads
/// running against a server that never finishes them, rows moved and a
/// running download paused and resumed again and again, each read published
/// to two subscribers. What each costs, and that both subscribers heard the
/// same. The quick version runs with the tests; the whole is a measure, run
/// by hand in release (`large_queue_under_load`).
#[cfg(test)]
mod under_load_tests;

/// A read of 10,000 rows where a few changed (gaps row 181).
#[cfg(test)]
mod wake_tests;
