// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The settings file: where it is, read leniently and set aside when odd, and written.

use std::path::PathBuf;

use super::{home, lenient, secrets, Settings, SETTINGS_FORMAT};

impl Settings {
    /// Where the settings are kept: `settings.json` in EDM's home.
    pub fn path() -> PathBuf {
        home().join(edm_core::paths::SETTINGS)
    }

    /// Reads what is stored, falling back to the defaults, and says so when
    /// it had to.
    ///
    /// A settings file that cannot be read is not worth failing to start
    /// over: the defaults are a working configuration, and refusing to open
    /// leaves someone with no way to fix the file through the app. But it is
    /// not overwritten either, or the next save would replace a file with one
    /// stray comma — a folder, a proxy, a schedule — with the defaults, and
    /// nothing would say why they had all gone. It is kept aside, as the queue
    /// file is, and the window is told.
    pub fn load() -> (Self, Option<String>) {
        Self::load_from(&Self::path())
    }

    pub(super) fn load_from(path: &std::path::Path) -> (Self, Option<String>) {
        let (mut settings, mut notice) = Self::load_file(path);
        // A file not read whole: its empty passwords may be defaults standing in.
        settings.keep_stored_passwords = notice.is_some();
        if let Some(store) = secrets::store() {
            let ns = edm_core::secretstore::namespace(path);
            let (moved, note) = settings.take_passwords_from(store, &ns);
            settings.keep_stored_passwords |= note.is_some();
            if moved {
                // Passwords the file held in clear are in the store now: write it again
                // without them.
                if let Err(io) = settings.save_to(path) {
                    notice = Some(join_notice(
                        notice,
                        format!("the passwords moved to the store are still in the settings file ({io})"),
                    ));
                }
            }
            if let Some(note) = note {
                notice = Some(join_notice(notice, note));
            }
        }
        (settings, notice)
    }

    /// A settings file's text as EDM reads it: whole, or, where one entry is
    /// odd, field by field (that entry costs only itself), with the names of
    /// what was left out. The error is the whole read's, where even the
    /// field-by-field one fails. One reader for the window and the browser's
    /// host: the host read strictly, so one odd field put every default in
    /// force there while the window kept the rest.
    pub(super) fn parse(text: &str) -> Result<(Self, Vec<String>), serde_json::Error> {
        // The mark some editors put before UTF-8 (and PowerShell 5.1's `Out-File -Encoding
        // utf8`) is not JSON: read with it, the file was set aside and every default put in
        // force (gaps row 212).
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        lenient::take();
        let parsed = match serde_json::from_str::<Settings>(text) {
            Ok(settings) => Ok((settings, Vec::new())),
            Err(e) => lenient::settings(text).ok_or(e),
        };
        parsed.map(|(read, mut dropped)| {
            // What the lenient readers inside left out: a proxy type, a field
            // of a schedule, an entry of a list or a field of one.
            dropped.extend(lenient::take());
            dropped.dedup();
            (read, dropped)
        })
    }

    /// The settings at `path` as [`Settings::load`] reads them, every value
    /// brought into range, but read only: nothing is moved aside or copied,
    /// and the store of secrets is not asked. For a process with no window
    /// to say what it did: the browser's host.
    pub fn read_only(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Self::parse(&text).ok())
            .map(|(read, _)| read.readable().0)
            .unwrap_or_else(|| Settings::default().clamped())
    }

    pub(super) fn load_file(path: &std::path::Path) -> (Self, Option<String>) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return (Settings::default().clamped(), None);
        };
        let (read, dropped) = match Self::parse(&text) {
            Ok(parsed) => parsed,
            Err(e) => {
                let aside = edm_core::paths::unreadable(path);
                let notice = match std::fs::rename(path, &aside) {
                    Ok(()) => format!(
                        "the saved settings could not be read ({e}); they were kept as {} \
                             and the defaults are in use",
                        aside.display()
                    ),
                    Err(io) => format!(
                        "the saved settings could not be read ({e}) or moved aside ({io}); \
                             the defaults are in use and the next change will replace them"
                    ),
                };
                return (Settings::default().clamped(), Some(notice));
            }
        };
        let (settings, mut notes) = read.readable();
        if !dropped.is_empty() {
            notes.insert(
                0,
                format!(
                    "some saved settings could not be read ({}), so their defaults are in use",
                    dropped.join(", ")
                ),
            );
        }
        if notes.is_empty() {
            return (settings, None);
        }
        // Whatever was left out or changed, the file as it was is kept
        // before the next save writes over it.
        let aside = edm_core::paths::unreadable(path);
        notes.push(match std::fs::copy(path, &aside) {
            Ok(_) => format!("the file as it was is kept as {}", aside.display()),
            Err(io) => format!("the file as it was could not be kept aside ({io})"),
        });
        (settings, Some(notes.join("; ")))
    }

    /// What was read, held to what the engine takes, with a word about each
    /// thing that had to be changed to be used: a schedule whose times
    /// cannot be read (ignored anyway, so now off, and said to be), and a
    /// category whose name is not one folder (it sorted nothing, and would
    /// otherwise sit in Options failing every OK).
    pub(super) fn readable(self) -> (Self, Vec<String>) {
        let mut settings = self.clamped();
        let mut notes = Vec::new();
        let schedule = &mut settings.schedule;
        if schedule.enabled
            && !(edm_core::is_time(&schedule.from) && edm_core::is_time(&schedule.to))
        {
            schedule.enabled = false;
            notes.push(format!(
                "the saved schedule's times ({:?} to {:?}) are not HH:MM, so the schedule is \
                 off; set it again in the Scheduler",
                schedule.from, schedule.to
            ));
        }
        let (kept, bad): (Vec<_>, Vec<_>) = std::mem::take(&mut settings.custom_categories)
            .into_iter()
            .partition(|c| edm_core::one_folder(&c.name));
        settings.custom_categories = kept;
        if !bad.is_empty() {
            let names: Vec<String> = bad.iter().map(|c| format!("{:?}", c.name)).collect();
            notes.push(format!(
                "the categories {} were left out: a category's name is also its folder, and \
                 these cannot be a folder on this computer",
                names.join(", ")
            ));
        }
        // A kind's folder that cannot be used here sorts into the download
        // folder itself; said, rather than done without a word.
        // Empty is "not nested", on purpose.
        let unusable: Vec<String> = settings
            .folders
            .category_folders
            .iter()
            .filter(|f| !f.trim().is_empty() && !edm_core::one_folder(f))
            .map(|f| format!("{f:?}"))
            .collect();
        if !unusable.is_empty() {
            notes.push(format!(
                "the folders {} cannot be a folder on this computer, so those kinds of file \
                 land in the download folder itself; rename them in Options",
                unusable.join(", ")
            ));
        }
        (settings, notes)
    }

    /// Writes the settings to [`Settings::path`] at once, readable by this
    /// account alone; a stop mid-write leaves the file before it whole.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    /// [`Settings::save`] to `path`. Where the operating system's store works the Sign-ins',
    /// the proxy's and a saved dial-up password go there and the file keeps them empty
    /// (`secrets.rs`).
    pub(super) fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        self.save_with(path, secrets::store())
    }

    /// [`Settings::save_to`] with the store given, for the tests.
    pub(super) fn save_with(
        &self,
        path: &std::path::Path,
        store: Option<&dyn edm_core::secretstore::SecretStore>,
    ) -> std::io::Result<()> {
        let mut file = self.clone();
        // A dial-up password not to be saved is written nowhere, store or no store: kept in
        // memory for this run and forgotten when EDM closes, as reference A's (`dialup.rs`).
        if !file.advanced.dialup.save_password {
            file.advanced.dialup.password.clear();
        }
        if let Some(store) = store {
            // A refusal leaves the password in the file, as before; the next read says so.
            let _ = file.move_passwords_to(store, &edm_core::secretstore::namespace(path));
        }
        let stamped = edm_core::saved::Stamped {
            format: SETTINGS_FORMAT,
            body: &file,
        };
        edm_core::write_atomic(path, serde_json::to_string_pretty(&stamped)?.as_bytes())
    }
}

/// `more` after what was already to be said.
fn join_notice(first: Option<String>, more: String) -> String {
    match first {
        Some(first) => format!("{first}; {more}"),
        None => more,
    }
}
