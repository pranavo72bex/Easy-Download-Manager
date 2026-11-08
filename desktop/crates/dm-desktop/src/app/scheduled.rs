// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the schedule adds to the queue beyond holding it (Scheduler › When done
//! and Try a failed download again).
//!
//! When its window opens with something waiting to run, the schedule's choice
//! of what dm does at the end is armed as the session's own (Downloads › When
//! done), so the same cancellable countdown stands between it and the
//! computer; closing the window with the choice unused puts it away again. And
//! while the window is open a download that failed is tried again, up to the
//! number the person set, each time the queue has run dry. When the scheduled
//! downloads are done, the program set is run and, on Windows, the dial-up or
//! VPN connection hung up where the person asked (reference A's Scheduler).

use super::*;

/// Watches the window open and close between two reads of the list.
#[derive(Debug, Default)]
pub(crate) struct Watch {
    was_open: bool,
    /// The action armed from the schedule when the window opened, until it
    /// is used or the window closes.
    armed: Option<crate::power::WhenDone>,
    /// Rounds of tries again made since the window opened.
    rounds: u8,
    /// Whether something was waiting to run since the window opened.
    had_work: bool,
    /// Whether the program to run at the end has been run in this window.
    ran: bool,
}

/// What a read of the list finds of the window.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Nothing,
    Opened,
    Closed,
}

/// What the end of the scheduled downloads calls for at a read of the list.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AtEnd {
    Nothing,
    /// Start the failed ones again: a round of tries again is owed.
    Retry,
    /// They are done: the program to run, then the hang-up.
    Done,
}

impl Watch {
    pub(crate) fn step(&mut self, open: bool) -> Step {
        match (self.was_open, open) {
            (false, true) => {
                self.was_open = true;
                self.rounds = 0;
                self.had_work = false;
                self.ran = false;
                Step::Opened
            }
            (true, false) => {
                self.was_open = false;
                Step::Closed
            }
            _ => Step::Nothing,
        }
    }

    /// Whether the program to run at the end is due: the window is open, something
    /// was waiting in it, nothing is now, and it has not been run in this window.
    pub(crate) fn run_due(&mut self, open: bool, work_left: bool) -> bool {
        if !open {
            return false;
        }
        if work_left {
            self.had_work = true;
            return false;
        }
        if self.had_work && !self.ran {
            self.ran = true;
            return true;
        }
        false
    }

    /// At a read of the list: with nothing left to run and some failed, a
    /// round of tries again while rounds are left (`retries`); once none is
    /// owed, the end, once in a window after something ran in it. So the
    /// program and the hang-up come after the tries again, as reference A's
    /// "when done" comes after each file's retries, not between them.
    pub(crate) fn at_end(
        &mut self,
        open: bool,
        work_left: bool,
        failed: bool,
        retries: u8,
    ) -> AtEnd {
        if open && !work_left && failed && self.retry_round(retries) {
            return AtEnd::Retry;
        }
        if self.run_due(open, work_left) {
            AtEnd::Done
        } else {
            AtEnd::Nothing
        }
    }

    pub(crate) fn arm(&mut self, action: crate::power::WhenDone) {
        self.armed = Some(action);
    }

    /// The armed action, taken back: the window closed or it was spent.
    pub(crate) fn disarm(&mut self) -> Option<crate::power::WhenDone> {
        self.armed.take()
    }

    /// Whether another round of tries again is owed: the person allowed
    /// `retries`, and this window has made fewer.
    pub(crate) fn retry_round(&mut self, retries: u8) -> bool {
        if self.rounds < retries.min(5) {
            self.rounds += 1;
            true
        } else {
            false
        }
    }
}

/// Whether anything is left to run: running, queued, or held by the schedule
/// (which starts again when it opens). What is stopped by a person or has
/// settled is not.
pub(crate) fn work_left(rows: &[Arc<dm_core::Entry>]) -> bool {
    rows.iter().any(|r| {
        matches!(r.state, State::Running | State::Queued)
            || (r.state == State::Paused && r.paused_by_schedule)
    })
}

/// Whether a once-only date of `schedule` has been and gone, by the clock now.
fn once_over_now(schedule: &dm_core::Schedule) -> bool {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    schedule.once_at.is_some()
        && schedule.once_over(
            (now.year(), now.month() as u8, now.day() as u8),
            now.hour() * 60 + now.minute(),
        )
}

impl DmApp {
    /// A once-only date that is over is put away, so the schedule goes back to its weekly window
    /// (left set, its queue would stay shut for good).
    fn clear_spent_once_at(&mut self) {
        let global = once_over_now(&self.applied.schedule);
        let queues: Vec<String> = self
            .applied
            .queue_schedules
            .iter()
            .filter(|(_, s)| once_over_now(s))
            .map(|(name, _)| name.clone())
            .collect();
        if !global && queues.is_empty() {
            return;
        }
        let mut wanted = self.applied.clone();
        if global {
            wanted.schedule.once_at = None;
        }
        for name in queues {
            if let Some(s) = wanted.queue_schedules.get_mut(&name) {
                s.once_at = None;
            }
        }
        if let Err(why) = self.apply_settings(wanted) {
            self.problems.push(why);
        }
    }

    /// Called after every read of the list.
    pub(super) fn schedule_hooks(&mut self) {
        self.clear_spent_once_at();
        // The slow hours' limit in, or out, as they begin or end.
        let limit = self.applied.speed_limit_bytes_now();
        if self.limit_in_force != Some(limit) {
            self.backend.set_speed_limit(limit);
            self.limit_in_force = Some(limit);
        }
        let schedule = self.applied.schedule.clone();
        let open = schedule.enabled && schedule.open();
        match self.schedule_watch.step(open) {
            Step::Opened => {
                let wanted = crate::power::WhenDone::from_word(&schedule.when_done);
                let can = wanted == crate::power::WhenDone::Nothing
                    || wanted == crate::power::WhenDone::Quit
                    || crate::power::available(wanted);
                // Only for something waiting to run, and never over a choice
                // the person made this session in the Downloads menu.
                if wanted != crate::power::WhenDone::Nothing
                    && can
                    && self.when_done == crate::power::WhenDone::Nothing
                    && work_left(&self.rows)
                {
                    self.when_done = wanted;
                    self.schedule_watch.arm(wanted);
                    self.said = crate::i18n::Said::new(
                        "The schedule has set what dm does when the downloads are done: {}.",
                        &[&crate::i18n::t(wanted.verb())],
                    );
                }
            }
            Step::Closed => {
                // Unused: put away, unless it was changed in the menu since.
                if let Some(armed) = self.schedule_watch.disarm() {
                    if self.when_done == armed {
                        self.when_done = crate::power::WhenDone::Nothing;
                    }
                }
            }
            Step::Nothing => {}
        }
        // Fired (the choice was taken as the queue ran dry): nothing to put away.
        if self.when_done == crate::power::WhenDone::Nothing {
            self.schedule_watch.disarm();
        }
        // The rows the schedule for every queue looks after: those of a queue
        // with no schedule of its own (all of them, unless one has).
        let own = self.applied.queue_schedules.clone();
        let shared = looked_after(&self.rows, &own, None);
        let mut watch = std::mem::take(&mut self.schedule_watch);
        self.end_of_window(&schedule, open, &shared, &mut watch);
        self.schedule_watch = watch;
        // Each named queue with a schedule of its own: its program at the end and
        // its tries again, over its own rows. (What dm does when the downloads
        // are done stays the one for every queue: it acts on the computer.)
        let mut watches = std::mem::take(&mut self.queue_watches);
        watches.retain(|name, _| own.contains_key(name));
        for (name, schedule) in &own {
            let open = schedule.enabled && schedule.open();
            let watch = watches.entry(name.clone()).or_default();
            watch.step(open);
            let mine = looked_after(&self.rows, &own, Some(name));
            self.end_of_window(schedule, open, &mine, watch);
        }
        self.queue_watches = watches;
    }

    /// The failed ones of `rows` tried again while the window is open, a
    /// round each time they have run dry; and once no round is owed, the
    /// program a person set to run when the scheduled downloads are done,
    /// then the connection hung up where asked, once in a window after
    /// something ran in it.
    fn end_of_window(
        &mut self,
        schedule: &dm_core::Schedule,
        open: bool,
        rows: &[Arc<dm_core::Entry>],
        watch: &mut Watch,
    ) {
        let failed: Vec<Id> = rows
            .iter()
            .filter(|r| matches!(r.state, State::Failed(_)))
            .map(|r| r.id)
            .collect();
        match watch.at_end(open, work_left(rows), !failed.is_empty(), schedule.retries) {
            AtEnd::Retry => {
                for id in failed {
                    self.backend.retry(id);
                }
            }
            AtEnd::Done => {
                let command = schedule.run_when_done.trim();
                if !command.is_empty() {
                    if let Err(why) = dm_shell::shell::run_program(command) {
                        self.problems.push(why);
                    }
                }
                // After the program, as reference A's Scheduler hangs up after
                // the file it opens. Dial-up and VPN connections are Windows'.
                if schedule.hang_up && cfg!(windows) {
                    self.hang_up_later();
                }
            }
            AtEnd::Nothing => {}
        }
    }
}

impl DmApp {
    /// Hangs up the connection Options › Advanced › Dial Up / VPN sets, off
    /// the window's thread (Windows takes a moment to end it); what it came
    /// to is said when it is known.
    fn hang_up_later(&mut self) {
        let slot = Arc::clone(&self.hung_up);
        let spawned = std::thread::Builder::new()
            .name("dm-hang-up".into())
            .spawn(move || {
                let hung = dm_shell::dialup::hang_up();
                *dm_core::lock(&slot) = Some(hung);
            });
        if spawned.is_err() {
            self.say_hung_up(dm_shell::dialup::hang_up());
        }
    }

    /// Says what hanging up came to: a connection that was not up needs no
    /// word, one Windows would not end is a problem.
    pub(super) fn say_hung_up(&mut self, hung: dm_shell::dialup::HungUp) {
        use dm_shell::dialup::HungUp;
        match hung {
            HungUp::Done(name) => {
                self.said = crate::i18n::Said::new(
                    "The scheduled downloads are done: {} is hung up.",
                    &[&name],
                );
            }
            HungUp::WasDown(_) => {}
            HungUp::NoneSet => {
                self.said = crate::i18n::Said::new(
                    "Hang up modem when done: no connection is set in Options › Advanced › Dial Up / VPN.",
                    &[],
                );
            }
            HungUp::Failed(name, why) => self
                .problems
                .push(crate::i18n::tf("{} was not hung up: {}", &[&name, &why])),
        }
    }
}

/// The rows a schedule looks after: with `Some(queue)`, that queue's own
/// schedule's (its rows); with `None`, the one for every queue's (the rows of
/// every queue that has no schedule of its own).
fn looked_after(
    rows: &[Arc<dm_core::Entry>],
    own: &std::collections::BTreeMap<String, dm_core::Schedule>,
    queue: Option<&str>,
) -> Vec<Arc<dm_core::Entry>> {
    rows.iter()
        .filter(|r| match queue {
            Some(q) => r.queue == q,
            None => !own.contains_key(&r.queue),
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_opening_and_closing_is_seen_once_each() {
        let mut watch = Watch::default();
        assert_eq!(watch.step(false), Step::Nothing);
        assert_eq!(watch.step(true), Step::Opened);
        assert_eq!(watch.step(true), Step::Nothing);
        assert_eq!(watch.step(false), Step::Closed);
        assert_eq!(watch.step(false), Step::Nothing);
    }

    #[test]
    fn tries_again_are_as_many_as_allowed_in_one_window_and_start_over_at_the_next() {
        let mut watch = Watch::default();
        watch.step(true);
        assert!(!watch.retry_round(0), "zero leaves it failed, as before");
        assert!(watch.retry_round(2));
        assert!(watch.retry_round(2));
        assert!(!watch.retry_round(2), "no more than two rounds");
        watch.step(false);
        watch.step(true);
        assert!(watch.retry_round(2), "a new window starts over");
        let mut capped = Watch::default();
        capped.step(true);
        let rounds = (0..10).filter(|_| capped.retry_round(200)).count();
        assert_eq!(rounds, 5, "never more than five");
    }

    #[test]
    fn an_armed_action_is_taken_back_once() {
        let mut watch = Watch::default();
        watch.arm(crate::power::WhenDone::Sleep);
        assert_eq!(watch.disarm(), Some(crate::power::WhenDone::Sleep));
        assert_eq!(watch.disarm(), None);
    }

    #[test]
    fn the_kept_words_name_the_actions_and_back() {
        use crate::power::WhenDone;
        for w in WhenDone::ALL {
            assert_eq!(WhenDone::from_word(w.word()), w);
        }
        assert_eq!(WhenDone::from_word(""), WhenDone::Nothing);
        assert_eq!(WhenDone::from_word("reboot"), WhenDone::Nothing);
        assert_eq!(WhenDone::from_word(" sleep "), WhenDone::Sleep);
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn each_schedule_looks_after_its_own_rows() {
        let row = |id: Id, queue: &str| {
            let mut e = crate::app::rows::status_tests::paused(false);
            e.id = id;
            e.queue = queue.into();
            Arc::new(e)
        };
        let rows = vec![
            row(1, ""),
            row(2, "Night"),
            row(3, "Music"),
            row(4, "Night"),
        ];
        let ids = |v: Vec<Arc<dm_core::Entry>>| v.iter().map(|r| r.id).collect::<Vec<_>>();
        let mut own = std::collections::BTreeMap::new();
        // No queue has a schedule of its own: the shared one looks after all.
        assert_eq!(ids(looked_after(&rows, &own, None)), [1, 2, 3, 4]);
        own.insert("Night".to_string(), dm_core::Schedule::default());
        assert_eq!(ids(looked_after(&rows, &own, None)), [1, 3]);
        assert_eq!(ids(looked_after(&rows, &own, Some("Night"))), [2, 4]);
    }
}

#[cfg(test)]
mod run_tests {
    use super::{AtEnd, Watch};

    #[test]
    fn the_end_of_the_scheduled_downloads_comes_after_their_tries_again() {
        let mut watch = Watch::default();
        watch.step(true);
        assert_eq!(
            watch.at_end(true, true, false, 2),
            AtEnd::Nothing,
            "running"
        );
        assert_eq!(
            watch.at_end(true, false, true, 2),
            AtEnd::Retry,
            "one failed"
        );
        assert_eq!(watch.at_end(true, true, false, 2), AtEnd::Nothing);
        assert_eq!(
            watch.at_end(true, false, true, 2),
            AtEnd::Retry,
            "the second round"
        );
        assert_eq!(watch.at_end(true, true, false, 2), AtEnd::Nothing);
        assert_eq!(
            watch.at_end(true, false, true, 2),
            AtEnd::Done,
            "no round left: done, failed or not"
        );
        assert_eq!(watch.at_end(true, false, true, 2), AtEnd::Nothing, "once");
        // None to try again: done as the queue runs dry.
        let mut clean = Watch::default();
        clean.step(true);
        assert_eq!(clean.at_end(true, true, false, 0), AtEnd::Nothing);
        assert_eq!(clean.at_end(true, false, true, 0), AtEnd::Done);
        // Shut: neither.
        let mut shut = Watch::default();
        assert_eq!(shut.at_end(false, false, true, 2), AtEnd::Nothing);
        assert_eq!(shut.at_end(false, true, false, 2), AtEnd::Nothing);
    }

    #[test]
    fn the_program_at_the_end_runs_once_after_something_ran_in_the_window() {
        let mut watch = Watch::default();
        watch.step(true);
        // Nothing waited in this window: nothing to do at its end.
        assert!(!watch.run_due(true, false));
        // Something runs, then the queue runs dry: due once.
        assert!(!watch.run_due(true, true));
        assert!(watch.run_due(true, false));
        assert!(!watch.run_due(true, false), "twice in one window");
        // Closed: never. A new window starts over.
        assert!(!watch.run_due(false, false));
        watch.step(false);
        watch.step(true);
        assert!(!watch.run_due(true, true));
        assert!(watch.run_due(true, false));
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;
    use dm_shell::dialup::HungUp;

    /// A system that does nothing, for a window no one sees.
    struct Quiet;

    impl crate::backend::Platform for Quiet {
        fn set_login_item(&self, _: bool) -> Result<(), String> {
            Ok(())
        }
        fn notify(&self, _: &str, _: &str) {}
        fn clipboard(&self) -> Option<String> {
            None
        }
        fn reveal(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn open(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn chime(&self) {}
        fn power(&self, _: crate::power::WhenDone, _: crate::power::Report) -> Result<(), String> {
            Ok(())
        }
        fn choose_app(&self) -> Option<std::path::PathBuf> {
            None
        }
        fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
            Ok(())
        }
        fn choose_file(&self, _: bool, _: &str) -> Option<std::path::PathBuf> {
            None
        }
    }

    /// `DM_HOME` pointed at a folder of the test's own while a window is made
    /// and used (a window reads its inbox there as it opens), and put back as
    /// it was however the test ends.
    struct Home {
        before: Option<std::ffi::OsString>,
        _held: std::sync::MutexGuard<'static, ()>,
    }

    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl Home {
        fn at(dir: &std::path::Path) -> Home {
            let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
            let before = std::env::var_os("DM_HOME");
            // SAFETY: under `ENV`; another test of this binary reading the home
            // meanwhile finds this temporary one, never the person's.
            unsafe { std::env::set_var("DM_HOME", dir) };
            assert_eq!(
                dm_shell::settings::home(),
                dir,
                "not the test's home; stopping"
            );
            Home {
                before,
                _held: guard,
            }
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            // SAFETY: as in `Home::at`, still under `ENV`.
            unsafe {
                match &self.before {
                    Some(before) => std::env::set_var("DM_HOME", before),
                    None => std::env::remove_var("DM_HOME"),
                }
            }
        }
    }

    #[test]
    fn what_hanging_up_came_to_is_said_and_a_connection_that_was_down_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let _home = Home::at(dir.path());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let settings = dm_shell::Settings {
            download_dir: dir.path().join("out"),
            language: "en".into(),
            ..dm_shell::Settings::default()
        };
        let queue = dm_core::Queue::new(
            dm_core::client(),
            settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
            settings.concurrency,
        );
        let backend = crate::backend::Backend::new(
            runtime.handle().clone(),
            queue,
            settings,
            Vec::new(),
            Arc::new(Quiet),
        );
        let mut app = DmApp::new(backend);

        app.say_hung_up(HungUp::Done("Office VPN".into()));
        assert_eq!(
            app.said.english(),
            "The scheduled downloads are done: Office VPN is hung up."
        );
        app.said = Default::default();
        app.say_hung_up(HungUp::WasDown("Office VPN".into()));
        assert!(
            app.said.is_empty(),
            "a connection that was down needs no word"
        );
        app.say_hung_up(HungUp::NoneSet);
        assert!(app
            .said
            .english()
            .starts_with("Hang up modem when done: no connection is set"));
        app.say_hung_up(HungUp::Failed("Office VPN".into(), "error 691".into()));
        assert_eq!(
            app.problems.last().map(String::as_str),
            Some("Office VPN was not hung up: error 691")
        );

        // Off the window's thread, and what it came to read with the list.
        // Not on Windows, where it would hang up the person's own connection.
        if !cfg!(windows) {
            app.said = Default::default();
            app.hang_up_later();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while dm_core::lock(&app.hung_up).is_none() {
                assert!(std::time::Instant::now() < deadline, "never came back");
                std::thread::sleep(Duration::from_millis(10));
            }
            app.logic(&egui::Context::default());
            assert!(dm_core::lock(&app.hung_up).is_none(), "not taken");
            assert!(app
                .said
                .english()
                .starts_with("Hang up modem when done: no connection is set"));
        }
    }
}
