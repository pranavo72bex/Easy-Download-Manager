// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The application's work over time, apart from any window: what is due at a
//! tick (files whose "Delete file after" has come, and, as they move here, the
//! other timers), done by the application and reported, so each presentation
//! only reacts (forgets a row's dialogs, says what could not be done).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_core::{Entry, Id, State};

use super::backend::Backend;
use super::detect::{Detected, Detector, DryWatch};
use super::power::WhenDone;
use super::schedule::{looked_after, work_left, AtEnd, Step, Watch};
use super::snapshot::Snapshot;
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
}

impl Session {
    /// Reads the queue and does what follows from it that is the
    /// application's: the command a person set to run on each finished file
    /// (Options › Downloads). Call `changes::take` before, never after, so a
    /// change made during the read is still owed one.
    pub fn read(&mut self, backend: &Backend, settings: &Settings) -> Read {
        let snapshot = backend.snapshot();
        let detected = self.detector.read(&snapshot.rows);
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
        }
    }

    /// A row the presentation added itself: not "started elsewhere".
    pub fn own(&mut self, id: Id) {
        self.detector.own(id);
    }

    /// Each row's state at the read before the latest (When done's dry-queue
    /// watch compares the latest rows with it).
    pub fn before(&self) -> &HashMap<Id, State> {
        self.detector.before()
    }

    /// Whether the queue has just run dry since the read before (When done:
    /// the sound and the action it arms are the presentation's to start).
    /// Call once per read, after it.
    pub fn ran_dry(&mut self, rows: &[Arc<Entry>]) -> bool {
        self.dry.look(rows, self.detector.before())
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
}

/// The application layer driven by a presentation with no window at all, as
/// a future front end would: a hand-off added, the queue read until it runs
/// dry, When done counted down and carried out, and the control port's
/// answer, with nothing from any window toolkit.
#[cfg(test)]
mod no_window_tests {
    use super::*;
    use crate::app::handoff::{Asking, Route};

    #[test]
    fn a_presentation_with_no_window_drives_a_download_to_when_done() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let state = tempfile::tempdir().unwrap();
        let backend = crate::app::backend::testing::backend(&runtime, state.path());
        let addr = runtime.block_on(async {
            let app = axum::Router::new().fallback(|| async { "eleven byte" });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            addr
        });
        let settings = backend.settings();
        let mut session = Session {
            when_done: WhenDone::Sleep,
            ..Session::default()
        };
        // Each read is followed by one look for the queue running dry, the
        // first included.
        let first = session.read(&backend, &settings);
        assert!(!session.ran_dry(&first.snapshot.rows));

        dm_core::lock(&backend.handed).push_back(crate::host::Request {
            url: format!("http://{addr}/file.bin"),
            ..Default::default()
        });
        let Route::Added(Ok(id)) =
            session
                .hands
                .next(&backend, &settings, &[], Asking::Nobody, Instant::now())
        else {
            panic!("not added");
        };

        let mut finished = false;
        let mut fired = None;
        let until = Instant::now() + Duration::from_secs(20);
        while fired.is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
            let read = session.read(&backend, &settings);
            finished |= read.detected.finished.contains(&id);
            if session.ran_dry(&read.snapshot.rows) {
                fired = Some(session.when_done_fires(true, Instant::now()));
            }
        }
        assert!(finished, "the download was not seen to finish");
        assert_eq!(
            fired,
            Some(Fired::Counting),
            "a sleep waits out its countdown"
        );
        assert_eq!(session.when_done, WhenDone::Nothing, "taken once");
        let due = Instant::now() + Duration::from_secs(super::super::power::COUNTDOWN_SECS);
        assert_eq!(
            session.countdown_due(&backend, due),
            Some(Ok(Carried::Asked))
        );

        let rows = backend.snapshot().rows;
        let listed = crate::app::control_api::answer(
            &backend,
            &rows,
            "queue.list",
            &serde_json::Value::Null,
        )
        .unwrap()
        .unwrap();
        assert_eq!(listed[0]["id"], serde_json::json!(id));
        assert_eq!(listed[0]["state"], serde_json::json!({"kind": "completed"}));
    }
}

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
