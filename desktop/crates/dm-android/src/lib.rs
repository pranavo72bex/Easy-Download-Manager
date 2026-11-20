// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm on Android (gaps rows N1 and N3): the desktop's engine and the application's rules, the
//! same `Backend` the desktop's window and the headless run drive, as one library the phone's
//! Compose window calls through UniFFI. Nothing a download does is written here: each call is
//! one of `Backend`'s, so a feature works on the phone as it does on the desktop, and a fix to
//! it is made once.
//!
//! What the phone does for the engine (a notification, the clipboard, opening a file) is asked
//! of the app through [`AndroidSystem`], which `system.rs` turns into dm-shell's `Platform`.

#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use std::path::PathBuf;
use std::sync::Arc;

use dm_shell::app::backend::Backend;
use dm_shell::settings::Settings;
use dm_shell::shell;

mod row;
mod system;

pub use row::{Row, RowState, TorrentSwarm};
pub use system::AndroidSystem;

uniffi::setup_scaffolding!();

/// What a call into the engine is refused with, in dm's own words for the person.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EngineError {
    /// dm could not start: its runtime or its home folder.
    #[error("{0}")]
    Start(String),
    /// A request the engine refused (a link it cannot download, settings it will not take).
    #[error("{0}")]
    Refused(String),
}

/// What adding links gave: the rows made and each link left out, with why.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Added {
    /// The new rows' numbers, in the order asked.
    pub ids: Vec<u64>,
    /// Each link not added, with why.
    pub skipped: Vec<String>,
}

/// dm's engine, running for as long as this object lives: the queue, the settings in force and
/// the runtime every download runs on. The app makes one, in its foreground service
/// (gaps row N6), and every screen asks it.
#[derive(uniffi::Object)]
pub struct Engine {
    /// Kept for as long as dm runs; dropped last, after the backend has stopped the downloads.
    runtime: tokio::runtime::Runtime,
    backend: Backend,
}

#[uniffi::export]
impl Engine {
    /// Starts dm with its settings, queue and lists in `home` (the app's own files folder) and
    /// new downloads, until Options says otherwise, in `downloads`. `system` is the phone's
    /// side: notifications, the clipboard, opening files, and the call that the list changed.
    #[uniffi::constructor]
    pub fn start(
        home: String,
        downloads: String,
        system: Arc<dyn AndroidSystem>,
    ) -> Result<Arc<Self>, EngineError> {
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            return Err(EngineError::Start(format!(
                "dm's home must be a full path, not {}",
                home.display()
            )));
        }
        std::fs::create_dir_all(&home)
            .map_err(|e| EngineError::Start(format!("dm could not make its folder: {e}")))?;
        // dm's home is found through `DM_HOME` on every system (`dm_shell::settings::home`).
        // Set here, before the runtime's threads exist, so nothing reads the environment while
        // it changes.
        std::env::set_var("DM_HOME", &home);
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| EngineError::Start(format!("dm could not start: {e}")))?;
        dm_core::share_one_client();
        let shell::Started {
            queue,
            settings,
            notices,
        } = runtime.block_on(shell::start(&shell::Orders::default()));
        // A change to the queue is told to the phone, which draws again; as the desktop's
        // window is woken (`dm_desktop_base::boot`).
        dm_shell::app::changes::connect(&queue);
        {
            let system = Arc::clone(&system);
            dm_shell::app::changes::subscribe(move || system.changed());
        }
        {
            let queue = queue.clone();
            runtime.spawn(async move { queue.run().await });
        }
        let platform = Arc::new(system::Phone::new(system));
        let backend = Backend::new(runtime.handle().clone(), queue, settings, notices, platform);
        let engine = Engine { runtime, backend };
        engine.download_to_by_default(PathBuf::from(downloads))?;
        Ok(Arc::new(engine))
    }

    /// Adds every link in `text` (http, https, ftp, magnet, a `.torrent`'s address, a video
    /// page): what a share hands over is often a title and a link, so the links are found in
    /// it as the desktop finds them on the clipboard (`dm_shell::app::links::links_in`).
    pub fn add(&self, text: String) -> Result<Added, EngineError> {
        let links = dm_shell::app::links::links_in(&text);
        if links.is_empty() {
            return Err(EngineError::Refused(
                "there is no link dm can download in what was shared".into(),
            ));
        }
        let added = self
            .backend
            .add(&links.join(" "))
            .map_err(|e| EngineError::Refused(e.to_string()))?;
        Ok(Added {
            ids: added.ids,
            skipped: added.skipped,
        })
    }

    /// Every row of the list, in the list's order.
    pub fn list(&self) -> Vec<Row> {
        let swarms = self.backend.swarms();
        self.backend
            .list_shared()
            .iter()
            .map(|entry| Row::of(entry, swarms.get(&entry.id)))
            .collect()
    }

    /// One row, if it is still in the list.
    pub fn row(&self, id: u64) -> Option<Row> {
        let swarms = self.backend.swarms();
        self.backend
            .row(id)
            .map(|entry| Row::of(&entry, swarms.get(&id)))
    }

    /// Stops a download, to be resumed.
    pub fn pause(&self, id: u64) {
        self.backend.pause(id);
    }

    /// Resumes a paused download.
    pub fn resume(&self, id: u64) {
        self.backend.resume(id);
    }

    /// Tries a failed download again from where it stopped.
    pub fn retry(&self, id: u64) {
        self.backend.retry(id);
    }

    /// Stops a download for good.
    pub fn cancel(&self, id: u64) {
        self.backend.cancel(id);
    }

    /// Takes a row out of the list, keeping its file.
    pub fn remove(&self, id: u64) {
        self.backend.remove(id);
    }

    /// Stops every download; how many were stopped.
    pub fn pause_all(&self) -> u32 {
        count(self.backend.pause_all())
    }

    /// Resumes every paused download; how many were.
    pub fn resume_all(&self) -> u32 {
        count(self.backend.resume_all())
    }

    /// Takes the finished rows out of the list; how many.
    pub fn clear_finished(&self) -> u32 {
        count(self.backend.clear_finished())
    }

    /// Which of a torrent's files are downloaded, one `true` a file in the torrent's order.
    pub fn set_torrent_files(&self, id: u64, wanted: Vec<bool>) -> Result<(), EngineError> {
        self.backend
            .set_torrent_files(id, &wanted)
            .map_err(EngineError::Refused)
    }

    /// The whole program's speed limit, bytes a second; 0 for none.
    pub fn set_speed_limit(&self, bytes_per_second: u64) {
        self.backend.set_speed_limit(bytes_per_second);
    }

    /// Whether anything is downloading or waiting its turn: the foreground service stays while
    /// this is so (gaps row N6).
    pub fn busy(&self) -> bool {
        self.backend
            .list_shared()
            .iter()
            .any(|entry| !entry.state.settled() && entry.state != dm_core::State::Paused)
    }

    /// The settings in force, as the JSON dm saves them as: every Options line, so the phone's
    /// Options page needs no second copy of each.
    pub fn settings_json(&self) -> Result<String, EngineError> {
        serde_json::to_string(&self.backend.settings())
            .map_err(|e| EngineError::Refused(e.to_string()))
    }

    /// Puts `json` (all of [`Engine::settings_json`], changed) in force and saves it, whole or
    /// not at all, as Options' OK does; the settings now in force come back.
    pub fn set_settings_json(&self, json: String) -> Result<String, EngineError> {
        let wanted: Settings = serde_json::from_str(&json)
            .map_err(|e| EngineError::Refused(format!("these settings do not read: {e}")))?;
        let now = self
            .backend
            .set_settings(wanted)
            .map_err(EngineError::Refused)?;
        serde_json::to_string(&now).map_err(|e| EngineError::Refused(e.to_string()))
    }

    /// What went wrong at start and was put right, said once to the person.
    pub fn take_notices(&self) -> Vec<String> {
        std::mem::take(&mut *dm_core::lock(&self.backend.notices()))
    }

    /// Stops the downloads, saving each to be resumed, and writes the list down; how many were
    /// stopped. Called when the service ends.
    pub fn shut_down(&self) -> u32 {
        count(self.backend.shut_down())
    }
}

impl Engine {
    /// The download folder is the phone's (`downloads`) where the settings name none a phone
    /// can write to: on the first start, the desktop's default (`dirs`' Downloads) means
    /// nothing on Android.
    fn download_to_by_default(&self, downloads: PathBuf) -> Result<(), EngineError> {
        let mut settings = self.backend.settings();
        let current = &settings.folders.download_dir;
        if current.is_absolute() && current.parent().is_some_and(|p| p.is_dir()) {
            return Ok(());
        }
        if !downloads.is_absolute() {
            return Err(EngineError::Start(format!(
                "the download folder must be a full path, not {}",
                downloads.display()
            )));
        }
        settings.folders.download_dir = downloads;
        self.backend
            .set_settings(settings)
            .map(|_| ())
            .map_err(EngineError::Start)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.backend.shut_down();
    }
}

/// A count for Kotlin, which has no `usize`.
fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
