// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the engine asks of the phone, written in Kotlin by the app (`AndroidSystem`) and seen
//! by the application as dm-shell's `Platform`, as the desktop's `platform.rs` is.

use std::sync::Arc;

use dm_shell::app::platform::Platform;
use dm_shell::app::power::{Report, WhenDone};

/// The phone's side, implemented by the app in Kotlin. Each call comes from one of the
/// engine's threads, never the window's: the app posts what draws to its main thread.
#[uniffi::export(with_foreign)]
pub trait AndroidSystem: Send + Sync {
    /// A notification: `path` is the finished file a tap opens, `failed` the row a Retry
    /// button tries again; neither for a plain message.
    fn notify(&self, title: String, body: String, path: Option<String>, failed: Option<u64>);
    /// The clipboard's text, where there is any (the link watcher, Add from clipboard).
    fn clipboard(&self) -> Option<String>;
    /// Puts `text` on the clipboard (Copy link).
    fn set_clipboard(&self, text: String);
    /// Opens a finished file with the app the phone opens it with; why not, if it could not.
    fn open(&self, path: String) -> Option<String>;
    /// Shows the folder a finished file is in; why not, if it could not.
    fn reveal(&self, path: String) -> Option<String>;
    /// A short sound: a download finished (`failed` false) or failed.
    fn sound(&self, failed: bool);
    /// The list changed: the screens showing it draw again.
    fn changed(&self);
}

/// [`AndroidSystem`] as the application's `Platform`.
pub(crate) struct Phone {
    system: Arc<dyn AndroidSystem>,
}

impl Phone {
    pub(crate) fn new(system: Arc<dyn AndroidSystem>) -> Self {
        Phone { system }
    }
}

/// `None` from the app is success; a reason is the failure.
fn outcome(why_not: Option<String>) -> Result<(), String> {
    why_not.map_or(Ok(()), Err)
}

impl Platform for Phone {
    fn set_login_item(&self, _wanted: bool) -> Result<(), String> {
        // A phone has no login item: dm's service starts again on its own after a reboot where
        // a download was running (gaps row N12).
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) {
        self.system.notify(title.into(), body.into(), None, None);
    }

    fn notify_file(&self, title: &str, body: &str, path: &str) {
        self.system
            .notify(title.into(), body.into(), Some(path.into()), None);
    }

    fn notify_failure(&self, title: &str, body: &str, id: u64) {
        self.system
            .notify(title.into(), body.into(), None, Some(id));
    }

    fn clipboard(&self) -> Option<String> {
        self.system.clipboard()
    }

    fn set_clipboard(&self, text: &str) {
        self.system.set_clipboard(text.into());
    }

    fn reveal(&self, path: &str) -> Result<(), String> {
        outcome(self.system.reveal(path.into()))
    }

    fn open(&self, path: &str) -> Result<(), String> {
        outcome(self.system.open(path.into()))
    }

    fn chime(&self) {
        self.system.sound(false);
    }

    fn alert(&self) {
        self.system.sound(true);
    }

    fn power(&self, action: WhenDone, _report: Report) -> Result<(), String> {
        // An app cannot sleep or turn off a phone; When done's other choices still work.
        Err(format!(
            "dm cannot do {action:?} on a phone: an app may not put it to sleep or turn it off"
        ))
    }

    fn open_with(&self, path: &str, _app: &std::path::Path) -> Result<(), String> {
        // Android's own chooser picks the app.
        self.open(path)
    }

    fn can_choose_folder(&self) -> bool {
        // The phone's folders are chosen on its Options page (the storage picker, gaps row
        // N9), not through the desktop's chooser.
        false
    }
}
