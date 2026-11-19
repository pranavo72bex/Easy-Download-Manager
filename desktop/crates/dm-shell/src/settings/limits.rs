// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every value held to what the engine takes, and a change refused that cannot be made.

use dm_core::Category;

use super::{full_path, Settings, POPUPS_VERSION, RECENT_URLS, SIDEBAR_WIDTH, TEXT_SIZE};

/// Bounds the engine will honour, applied here so a value typed into the
/// window and a value read from a hand-edited file are held to the same rule.
const CONNECTIONS: (usize, usize) = (1, 32);
const CONCURRENCY: (usize, usize) = (1, 10);
/// A gigabyte a second is past any link this would be throttling.
const SPEED_LIMIT_KB: (u64, u64) = (0, 1024 * 1024);
/// A petabyte, and a month: past any allowance a link has.
const LIMIT_MB: u64 = 1024 * 1024 * 1024;
const LIMIT_HOURS: u64 = 24 * 31;

/// Bounds for the attempts and the read timeout (Options › Advanced): a hundred tries is
/// already an hour of asking,
/// and a connection silent for ten minutes is not slow.
const ATTEMPTS: (u32, u32) = (0, 100);
const READ_TIMEOUT_SECS: (u64, u64) = (5, 600);

impl Settings {
    /// Forces every value into a range the engine will accept.
    ///
    /// Applied on read as well as on write: the file is plain JSON on a disk
    /// anyone can edit, and a hand-typed 500 connections should be clamped
    /// rather than handed to the engine or rejected with an error nobody sees.
    pub fn clamped(mut self) -> Self {
        self.downloads.connections = self
            .downloads
            .connections
            .clamp(CONNECTIONS.0, CONNECTIONS.1);
        self.downloads.concurrency = self
            .downloads
            .concurrency
            .clamp(CONCURRENCY.0, CONCURRENCY.1);
        self.advanced.speed_limit = self
            .advanced
            .speed_limit
            .clamp(SPEED_LIMIT_KB.0, SPEED_LIMIT_KB.1);
        self.recent_urls.truncate(RECENT_URLS);
        self.advanced.retry_wait = self.advanced.retry_wait.min(3600);
        self.text_size = TEXT_SIZE.held(self.text_size);
        self.sidebar_width = SIDEBAR_WIDTH.held(self.sidebar_width);
        // A day at most: a stream meant to run longer is stopped by hand.
        self.downloads.record_minutes = self.downloads.record_minutes.min(24 * 60);
        if !["ask", "tray", "exit"].contains(&self.general.on_close.as_str()) {
            self.general.on_close = "ask".into();
        }
        if !["properties", "open", "folder"].contains(&self.general.double_click.as_str()) {
            self.general.double_click = "properties".into();
        }
        self.browser.catch.bring_up_to_date();
        // A file from an older edition takes this edition's defaults once
        // (reference A's: every pop-up on); a choice saved since stands.
        if self.popups_version < POPUPS_VERSION {
            self.general.show_progress = true;
            self.general.show_complete = true;
            self.general.ask_caught_downloads = true;
            self.general.ask_panel_downloads = true;
            self.popups_version = POPUPS_VERSION;
        }
        // The click keys are one of those the extension knows, or (to take)
        // none; anything else is what they ship as.
        if !dm_protocol::CLICK_KEYS.contains(&self.browser.catch.keep_key.as_str()) {
            self.browser.catch.keep_key = "alt".into();
        }
        if !self.browser.catch.take_key.is_empty()
            && !dm_protocol::CLICK_KEYS.contains(&self.browser.catch.take_key.as_str())
        {
            self.browser.catch.take_key = "shift".into();
        }
        self.last_speed_limit = self
            .last_speed_limit
            .clamp(SPEED_LIMIT_KB.0, SPEED_LIMIT_KB.1);
        self.limit_mb = self.limit_mb.min(LIMIT_MB);
        self.limit_hours = self.limit_hours.clamp(1, LIMIT_HOURS);
        self.advanced.attempts = self.advanced.attempts.clamp(ATTEMPTS.0, ATTEMPTS.1);
        self.downloads.recheck_hours = self.downloads.recheck_hours.min(LIMIT_HOURS);
        self.advanced.read_timeout = self
            .advanced
            .read_timeout
            .clamp(READ_TIMEOUT_SECS.0, READ_TIMEOUT_SECS.1);
        // A folder for unfinished downloads is a full path: `~` is the home
        // folder, and a relative one is no folder — a Dock launch starts in
        // `/`, where nothing can be written, and every download would fail.
        self.folders.parts_dir = full_path(&self.folders.parts_dir)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        // A limit names a queue or a site, or it limits nothing; and it is a
        // number the engine will take.
        self.downloads
            .queue_limits
            .retain(|limit| !limit.queue.trim().is_empty());
        for limit in &mut self.downloads.queue_limits {
            limit.queue = limit.queue.trim().to_string();
            limit.files = limit.files.clamp(CONCURRENCY.0, CONCURRENCY.1);
        }
        self.advanced
            .host_limits
            .retain(|limit| !limit.host.trim().is_empty());
        for limit in &mut self.advanced.host_limits {
            limit.host = limit.host.trim().to_ascii_lowercase();
            limit.connections = limit.connections.clamp(CONNECTIONS.0, CONNECTIONS.1);
        }
        // A rule names a site, and its folder is a whole path or none.
        self.advanced
            .site_rules
            .retain(|rule| !rule.host.trim().is_empty());
        for rule in &mut self.advanced.site_rules {
            rule.host = rule
                .host
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            rule.folder = full_path(&rule.folder)
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            rule.speed = rule.speed.min(SPEED_LIMIT_KB.1);
        }
        // The download folder is held to the parts folder's rule. Options refuses a
        // relative one as it is typed (`refusal`); one in a file edited by
        // hand falls back to the default rather than to wherever dm was
        // started from.
        self.folders.download_dir = full_path(&self.folders.download_dir.display().to_string())
            .unwrap_or_else(|| Settings::default().folders.download_dir);
        // One name per category, always: the engine indexes this list by
        // category, and a short one read from a hand-edited file would leave
        // the kinds past its end unsorted for no stated reason.
        self.folders
            .category_folders
            .resize(Category::ALL.len(), String::new());
        self
    }

    /// Why a change from `previous` to this cannot be put into force, if
    /// it cannot: a time the schedule cannot read, a download folder that
    /// is not a full path, a category name that is not one folder, a
    /// cookies file that cannot be read. Judged on what changed only, so a
    /// value already saved, however odd, never blocks an unrelated change
    /// — the language, a saved search's last run.
    pub fn refusal(&self, previous: &Settings) -> Option<String> {
        if self.schedule != previous.schedule
            && self.schedule.enabled
            && !(dm_core::is_time(&self.schedule.from) && dm_core::is_time(&self.schedule.to))
        {
            return Some(format!(
                "the schedule's times must be HH:MM in 24 hours, not {:?} and {:?}",
                self.schedule.from, self.schedule.to
            ));
        }
        if self.downloads.queue_starts != previous.downloads.queue_starts {
            if let Some(bad) = self
                .downloads
                .queue_starts
                .iter()
                .find(|s| !dm_core::is_time(&s.at))
            {
                return Some(format!(
                    "a queue's start time must be HH:MM in 24 hours, not {:?}",
                    bad.at
                ));
            }
        }
        if self.folders.download_dir != previous.folders.download_dir {
            let typed = self.folders.download_dir.display().to_string();
            if full_path(&typed).is_none() {
                return Some(format!(
                    "the download folder must be a full path, such as {}, not {typed:?}",
                    Settings::default().folders.download_dir.display()
                ));
            }
        }
        if self.folders.parts_dir.trim() != previous.folders.parts_dir.trim()
            && !self.folders.parts_dir.trim().is_empty()
            && full_path(&self.folders.parts_dir).is_none()
        {
            return Some(format!(
                "the folder for unfinished downloads must be a full path, not {:?}",
                self.folders.parts_dir
            ));
        }
        // A name made or renamed is held to every computer's rules, since
        // settings travel; one already saved is not.
        for c in &self.custom_categories {
            if previous.custom_categories.iter().any(|p| p.name == c.name) {
                continue;
            }
            if let Err(why) = dm_core::portable_folder(&c.name) {
                return Some(format!(
                    "a category's name is also its folder, so it cannot be {:?}: {why}",
                    c.name
                ));
            }
        }
        for (at, folder) in self.folders.category_folders.iter().enumerate() {
            if folder.trim().is_empty() || previous.folders.category_folders.get(at) == Some(folder)
            {
                continue;
            }
            if let Err(why) = dm_core::portable_folder(folder) {
                return Some(format!("a folder cannot be called {folder:?}: {why}"));
            }
        }
        let cookies = self.advanced.cookies_file.trim();
        if cookies != previous.advanced.cookies_file.trim() && !cookies.is_empty() {
            if let Err(e) = std::fs::metadata(cookies).and_then(|m| {
                if m.is_file() {
                    Ok(())
                } else {
                    Err(std::io::Error::other("it is not a file"))
                }
            }) {
                return Some(format!(
                    "the cookies file {cookies} could not be read: {}",
                    dm_core::file_error(&e)
                ));
            }
        }
        None
    }
}
