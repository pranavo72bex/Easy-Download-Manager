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

impl DmApp {
    /// A once-only date that is over is put away, so the schedule goes back to its weekly window
    /// (left set, its queue would stay shut for good).
    fn clear_spent_once_at(&mut self) {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        let Some(wanted) = dm_shell::app::schedule::spent_once_put_away(
            &self.applied,
            (now.year(), now.month() as u8, now.day() as u8),
            now.hour() * 60 + now.minute(),
        ) else {
            return;
        };
        if let Err(why) = self.apply_settings(wanted) {
            self.problems.push(why);
        }
    }

    /// Called after every read of the list.
    pub(super) fn schedule_hooks(&mut self) {
        self.clear_spent_once_at();
        // Done by the application (`Session::schedule_tick`); the window says
        // what the schedule armed and hangs up where it is asked to.
        let tick = self.session.schedule_tick(
            &self.backend,
            &self.applied,
            &self.rows,
            crate::power::available,
        );
        if let Some(wanted) = tick.armed {
            self.said = crate::i18n::Said::new(
                "The schedule has set what dm does when the downloads are done: {}.",
                &[&crate::i18n::t(wanted.verb())],
            );
        }
        self.problems.extend(tick.problems);
        if tick.hang_up && cfg!(windows) {
            self.hang_up_later();
        }
    }
}

impl DmApp {
    /// Hangs up the connection Options › Advanced › Dial Up / VPN sets, off
    /// the window's thread (Windows takes a moment to end it); what it came
    /// to is said when it is known.
    fn hang_up_later(&mut self) {
        if let Some(hung) = dm_shell::dialup::hang_up_later(Arc::clone(&self.hung_up)) {
            self.say_hung_up(hung);
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
