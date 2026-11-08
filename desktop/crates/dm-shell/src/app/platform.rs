// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the application needs from the computer it runs on, carried out by
//! whichever presentation runs it: the desktop's `platform.rs` today, a test's
//! stand-in in tests. The application asks through this trait and never calls
//! the system itself, so no window toolkit or system API reaches this layer.

/// What the window needs from the operating system, kept apart so a test can
/// stand in for it: the window is driven in tests against a real queue, and
/// a test must not register a login item or post a notification.
pub trait Platform: Send + Sync {
    /// Registers or removes dm as a login item, as `wanted` says.
    fn set_login_item(&self, wanted: bool) -> Result<(), String>;
    /// Announces a finished download.
    fn notify(&self, title: &str, body: &str);
    /// Called often: what a platform held back while the person was busy
    /// (a fullscreen app, a presentation) is said once it is not.
    fn tick(&self) {}
    /// Announces a finished file, which a click on the notification opens
    /// where the system says it was clicked.
    fn notify_file(&self, title: &str, body: &str, _path: &str) {
        self.notify(title, body);
    }
    /// The clipboard's text, for the opt-in link watcher.
    fn clipboard(&self) -> Option<String>;
    /// Shows a finished file in the file manager.
    fn reveal(&self, path: &str) -> Result<(), String>;
    /// Opens a finished file with whatever the system opens it with.
    fn open(&self, path: &str) -> Result<(), String>;
    /// [`Platform::open`] without waiting on it: an opener's verdict takes
    /// up to a second and a half on Linux, and the window froze for it.
    /// A failure goes to `report`.
    fn open_later(&self, path: &str, report: super::power::Report) {
        if let Err(e) = self.open(path) {
            report(e);
        }
    }
    /// [`Platform::reveal`] without waiting on it, as [`Platform::open_later`].
    fn reveal_later(&self, path: &str, report: super::power::Report) {
        if let Err(e) = self.reveal(path) {
            report(e);
        }
    }
    /// Moves a file to the Recycle Bin or the Trash, so a deleted download can
    /// be got back. A platform with none deletes it.
    fn trash(&self, path: &str) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
    /// Plays the system's short sound for a finished download.
    fn chime(&self);
    /// Plays a sound file the person chose; the finish sound when it cannot be played.
    fn play_file(&self, _path: &str) {
        self.chime();
    }
    /// Plays the system's short sound for a failed download. The finish's
    /// sound unless a platform has a different one.
    fn alert(&self) {
        self.chime();
    }
    /// Puts the computer to sleep or shuts it down, when done. A failure to
    /// start is the answer; a refusal that comes later goes to `report`.
    fn power(
        &self,
        action: super::power::WhenDone,
        report: super::power::Report,
    ) -> Result<(), String>;
    /// Asks for an application to open a file with. `None` when the person
    /// cancels.
    fn choose_app(&self) -> Option<std::path::PathBuf>;
    /// Opens a finished file with the application `app`.
    fn open_with(&self, path: &str, app: &std::path::Path) -> Result<(), String>;
    /// Asks for a file: one to read (`save` false) or a place to write one
    /// called `suggested`. `None` when the person cancels.
    fn choose_file(&self, save: bool, suggested: &str) -> Option<std::path::PathBuf>;
    /// Asks for a folder, for a Browse… button beside a folder's field.
    /// `None` when the person cancels, or where there is nothing to ask with.
    fn choose_folder(&self) -> Option<std::path::PathBuf> {
        None
    }
    /// Whether this desktop has a folder chooser to ask with. Where it has
    /// none, `choose_folder` answers `None` at once, as for a Cancel, and
    /// Browse… says so instead of doing nothing.
    fn can_choose_folder(&self) -> bool {
        true
    }
    /// Whether this desktop has a file chooser to ask with, for Import and
    /// Export. Where it has none, they ask for the file's path instead of
    /// doing nothing.
    fn can_choose_file(&self) -> bool {
        self.can_choose_folder()
    }
}

/// Whether the file chooser dm's dialogs use (the desktop portal's, else
/// zenity's) is there: the portal answering, or zenity on `path` (a `PATH`
/// value).
pub fn chooser_here(portal: bool, path: Option<&std::ffi::OsStr>) -> bool {
    portal
        || path
            .is_some_and(|path| std::env::split_paths(path).any(|dir| dir.join("zenity").is_file()))
}
