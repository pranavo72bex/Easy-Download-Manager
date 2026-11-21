// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The settings in force: read, changed, kept and written; and the banner's problems.

use std::sync::{Arc, Mutex};

use super::Backend;
use crate::{shell, Settings};

impl Backend {
    /// The settings in force.
    #[must_use]
    pub fn settings(&self) -> Settings {
        crate::app::costs::copied_settings();
        dm_core::lock(&self.settings).clone()
    }

    /// A client for the application's own fetches, which are not downloads
    /// (a site's pages crawled, the update check, a ZIP's listing): through
    /// the proxy in force, with its read timeout.
    #[must_use]
    pub fn client(&self) -> dm_core::reqwest::Client {
        let settings = dm_core::lock(&self.settings);
        settings.client().unwrap_or_else(|_| dm_core::client())
    }

    /// How long downloads stay held where the download limit (the day's or
    /// the month's allowance) is spent; `None` while it is not.
    #[must_use]
    pub fn allowance_spent(&self) -> Option<std::time::Duration> {
        self.queue.allowance_spent()
    }

    /// What of the computer's state holds every download now (gaps row W11: a metered
    /// connection, a low battery); `None` while nothing does.
    #[must_use]
    pub fn held_by(&self) -> Option<dm_core::Hold> {
        self.queue.held_by()
    }

    /// Every download held for `hold`, or let go on with `None` (gaps row W11): told by the
    /// application's tick, which reads the computer's state as Options › Downloads asks.
    pub fn hold_for(&self, hold: Option<dm_core::Hold>) {
        self.queue.hold_for(hold);
    }

    /// The settings as they change, for a thread of its own to read.
    pub fn settings_handle(&self) -> Arc<Mutex<Settings>> {
        Arc::clone(&self.settings)
    }

    /// Applies a change asked for by a person or, `by_script`, by a script (the
    /// control port, an AI client): what makes dm run something (the command
    /// after each download, the scheduled hours' program, the proxy script) is
    /// a person's to set, as a settings file imported never sets it, so a
    /// script's change keeps the ones in force (`Settings::keep_programs_of`).
    /// Returns what was kept as well as the outcome, so the presentation can
    /// say so even when the change itself is then refused.
    pub fn set_settings_from(
        &self,
        mut wanted: Settings,
        by_script: bool,
    ) -> (Result<Settings, String>, crate::settings::KeptPrograms) {
        let kept = if by_script {
            let mine = self.settings();
            let mut kept = wanted.keep_programs_of(&mine);
            kept.folders = wanted.keep_places_of(&mine);
            kept
        } else {
            Default::default()
        };
        (self.set_settings(wanted), kept)
    }

    /// Applies a change and writes it down, returning what is now in force.
    ///
    /// Whole or not at all. What the change refuses (a relative folder, a
    /// time that is not one) and a proxy that cannot be built are found
    /// first, before anything is touched; then the login item, the other
    /// thing that can fail; then the queue, putting the login item back if
    /// that fails after all. Before, the login item changed and then a
    /// refused proxy left it so, beside settings that said otherwise. Then
    /// the record of what is held, before the save that might fail — this
    /// is what the queue is actually running, so it is what the next change
    /// is compared against.
    pub fn set_settings(&self, settings: Settings) -> Result<Settings, String> {
        // Held to the end: the login item and the setting change together.
        let _changing = dm_core::lock(&self.login);
        let previous = self.settings();
        // Judged as typed, before `clamped` quietly mends what it can: a
        // relative download folder is refused, not swapped for the default.
        if let Some(why) = settings.refusal(&previous) {
            return Err(why);
        }
        let settings = settings.clamped();
        shell::check(&settings, Some(&previous))?;
        let was_at_login = previous.general.start_on_login;
        let login_changed = was_at_login != settings.general.start_on_login;
        if login_changed {
            self.platform
                .set_login_item(settings.general.start_on_login)
                .map_err(|e| format!("could not change start-on-login: {e}"))?;
        }
        let problems = match self.block_on(shell::apply(&settings, Some(&previous), &self.queue)) {
            Ok(problems) => problems,
            Err(e) => {
                if login_changed {
                    if let Err(back) = self.platform.set_login_item(was_at_login) {
                        self.note(format!(
                            "start-on-login could not be put back as it was: {back}"
                        ));
                    }
                }
                return Err(e.into());
            }
        };
        for problem in problems {
            self.note(problem);
        }
        *dm_core::lock(&self.settings) = settings.clone();
        settings
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        Ok(settings)
    }

    /// A change to how the window looks and nothing else (a column or a
    /// toolbar button dragged to another place): in force at once, and the
    /// settings file written off the caller's thread, after the errands
    /// asked before it (gaps row 183). Nothing reaches the queue, so
    /// nothing is applied to it. What is written is the settings in force
    /// when the errand runs, so a change made meanwhile is not written over
    /// by an older copy; a file that cannot be written is said in the
    /// banner.
    pub fn keep_look(&self, change: impl FnOnce(&mut Settings)) -> Settings {
        let now = {
            let mut settings = dm_core::lock(&self.settings);
            change(&mut settings);
            settings.clone()
        };
        // Not awaited: the errand runs whether or not its answer is.
        drop(self.later(|backend| {
            if let Err(e) = backend.settings().save() {
                backend.note(format!("settings could not be saved: {e}"));
            }
        }));
        now
    }

    /// Add URL's remembered addresses, kept and saved alone: nothing else
    /// changes, so nothing is applied to the queue — a full apply on every
    /// address added rebuilt its client and its cookies each time.
    pub fn remember_urls(&self, recent: Vec<String>) -> Result<Settings, String> {
        // Only this field changes on disk: the file is read, the list put
        // in, and written back, so nothing else saved there is overwritten.
        let (mut on_disk, _) = Settings::load();
        on_disk.recent_urls.clone_from(&recent);
        on_disk
            .clamped()
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        let mut settings = dm_core::lock(&self.settings);
        settings.recent_urls = recent;
        settings.recent_urls.truncate(crate::settings::RECENT_URLS);
        Ok(settings.clone())
    }

    /// The folder Download details last saved a kind of file to, kept and
    /// saved alone as Add URL's addresses are; `None` forgets it, for the
    /// kind's own folder.
    pub fn remember_folder(&self, kind: &str, folder: Option<&str>) -> Result<Settings, String> {
        let (mut on_disk, _) = Settings::load();
        let set = |s: &mut Settings| match &folder {
            Some(f) => {
                s.last_folders.insert(kind.to_string(), (*f).to_string());
            }
            None => {
                s.last_folders.remove(kind);
            }
        };
        set(&mut on_disk);
        on_disk
            .clamped()
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        let mut settings = dm_core::lock(&self.settings);
        set(&mut settings);
        Ok(settings.clone())
    }

    /// Whether a download the browser catches is asked about first
    /// (Options › Browser), as "Always save here without asking" switches
    /// it off from the dialog itself.
    pub fn set_ask_caught_downloads(&self, ask: bool) -> Result<Settings, String> {
        let (mut on_disk, _) = Settings::load();
        on_disk.general.ask_caught_downloads = ask;
        on_disk
            .clamped()
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        let mut settings = dm_core::lock(&self.settings);
        settings.general.ask_caught_downloads = ask;
        Ok(settings.clone())
    }

    /// What went wrong that no row can carry, each handed over once.
    pub fn problems(&self) -> Vec<String> {
        let mut all = std::mem::take(&mut *dm_core::lock(&self.notices));
        all.extend(self.queue.problems());
        all
    }

    /// Where something running apart from the window leaves a problem for
    /// the banner.
    pub fn notices(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.notices)
    }

    /// Leaves a problem for the banner.
    pub fn note(&self, problem: String) {
        dm_core::lock(&self.notices).push(problem);
    }
}

#[cfg(test)]
mod keep_look_tests {
    use crate::app::backend::testing::backend;

    /// A column dragged is in force at once and reaches the settings file
    /// off the caller's thread; a change made meanwhile is not written over.
    #[test]
    fn a_look_kept_is_in_force_at_once_and_written_after() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(&runtime, dir.path());
        let order = vec!["size".to_string(), "status".to_string()];
        let now = backend.keep_look(|s| s.column_order.clone_from(&order));
        assert_eq!(now.column_order, order);
        assert_eq!(backend.settings().column_order, order);
        // An errand after it runs once it is written. (What the file holds
        // is not compared: this crate's other tests write it at the same
        // time, from their own backends.)
        let written = runtime.block_on(backend.later(|_| crate::Settings::path().is_file()));
        assert_eq!(written, Some(true));
    }
}

#[cfg(test)]
mod settings_from_tests {
    use crate::app::backend::testing::backend;

    /// What makes dm run something is a person's to set: a script's change
    /// keeps the command in force and says it did; a person's sets it.
    #[test]
    fn a_scripts_settings_change_keeps_the_command_a_persons_sets_it() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(&runtime, dir.path());
        let mut wanted = backend.settings();
        wanted.downloads.on_complete = "echo done".into();
        let (now, kept) = backend.set_settings_from(wanted.clone(), true);
        assert!(kept.command);
        assert_eq!(now.unwrap().downloads.on_complete, "");
        let (now, kept) = backend.set_settings_from(wanted, false);
        assert!(!kept.command);
        assert_eq!(now.unwrap().downloads.on_complete, "echo done");
    }

    /// Where downloads go is a person's to set too: a script moving the download folder into a
    /// startup folder keeps the one in force and says it did; a person's change sets it
    /// (security audit).
    #[test]
    fn a_scripts_settings_change_keeps_the_folders_a_persons_sets_them() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(&runtime, dir.path());
        let before = backend.settings().folders.download_dir;
        let elsewhere = dir.path().join("LaunchAgents");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let mut wanted = backend.settings();
        wanted.folders.download_dir = elsewhere.clone();
        wanted.folders.categorize = !wanted.folders.categorize;
        let (now, kept) = backend.set_settings_from(wanted.clone(), true);
        assert!(kept.folders);
        let now = now.unwrap();
        assert_eq!(now.folders.download_dir, before);
        assert_ne!(now.folders.categorize, wanted.folders.categorize);
        let (now, kept) = backend.set_settings_from(wanted, false);
        assert!(!kept.folders);
        assert_eq!(now.unwrap().folders.download_dir, elsewhere);
    }
}
