// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The desktop's own services, as the application asks for them
//! ([`Platform`]): the login item, the Trash, notifications held while the
//! person is busy, the sounds, opening and showing files, and When done.
//! What a window toolkit's crates do (the clipboard, the file choosers and,
//! away from Windows, posting a notification) the window supplies through
//! [`WindowServices`], so that any window can use the rest.

use std::path::{Path, PathBuf};

use dm_shell::app::backend::Platform;
use dm_shell::shell;

use crate::login::{login_item_program, login_launcher};
use crate::power::{Report, WhenDone};
use crate::quiet::Note;

/// What only the window's own crates can do for [`Desktop`].
pub trait WindowServices: Send + Sync {
    /// The clipboard's text, if it holds any.
    fn clipboard(&self) -> Option<String>;
    /// The system's chooser of a program to open a file with.
    fn choose_app(&self) -> Option<PathBuf>;
    /// The system's folder chooser.
    fn choose_folder(&self) -> Option<PathBuf>;
    /// The system's file chooser: one to save as `suggested` when `save`,
    /// else one to open.
    fn choose_file(&self, save: bool, suggested: &str) -> Option<PathBuf>;
    /// Posts a notification, one whose click opens its file where it names
    /// one. Not called on Windows, where dm posts its own toasts.
    fn say(&self, note: Note);
}

/// The desktop's own services, with the window's in `window`.
pub struct Desktop<W> {
    /// What the window supplies.
    pub window: W,
}

impl<W: WindowServices> Desktop<W> {
    /// Says a held or a new notification: on Windows dm's own toast,
    /// elsewhere the window's.
    fn say(&self, note: Note) {
        #[cfg(windows)]
        {
            let _ = &self.window;
            say_on_windows(note);
        }
        #[cfg(not(windows))]
        self.window.say(note);
    }
}

impl<W: WindowServices> Platform for Desktop<W> {
    /// Only when the setting and the system disagree — or when the login
    /// item starts another copy of dm that is gone (the app moved, or was
    /// removed and installed elsewhere) and this one may claim it
    /// (`autostart::takes_over`, `autostart::may_claim`: a release build,
    /// not a test's dm). A copy still there keeps it, so two installed
    /// copies do not take it from each other at every launch. Registering
    /// writes the path of the running binary, so a copy that will not be
    /// there at the next login, or that macOS keeps from starting there, is
    /// refused (`autostart::refused_at_login`). Not being able to read the
    /// state is fatal only for registering: a login item that cannot be
    /// read must still be one a person can get rid of.
    fn set_login_item(&self, wanted: bool) -> Result<(), String> {
        // Linux's entry is dm's own: see `autostart`.
        if cfg!(target_os = "linux") {
            return crate::autostart::set(wanted);
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if wanted {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            if let Some(why) = crate::autostart::refused_at_login(
                &exe,
                &home,
                cfg!(target_os = "macos"),
                dm_shell::register::mounted_read_only,
            ) {
                return Err(why);
            }
        }
        let launcher = login_launcher(&exe)?;
        match launcher.is_enabled() {
            Ok(now) if now == wanted => {
                let gone = wanted
                    && login_item_program().is_some_and(|found| {
                        crate::autostart::takes_over(
                            &found,
                            &exe,
                            crate::autostart::may_claim(),
                            |p| p.exists(),
                        )
                    });
                if !gone {
                    return Ok(());
                }
                // Enabled for a copy that is gone: written again for this one.
            }
            Ok(_) => {}
            Err(e) if wanted => return Err(e.to_string()),
            Err(_) => {}
        }
        if wanted {
            launcher.enable()
        } else {
            launcher.disable()
        }
        .map_err(|e| e.to_string())
    }

    /// To the Recycle Bin or the Trash, not erased: a file deleted by a
    /// mistake can be put back.
    fn trash(&self, path: &str) -> Result<(), String> {
        trash::delete(path).map_err(|e| e.to_string())
    }

    /// Posted from a thread of its own: on macOS, `show` waits on the
    /// system for about two seconds, and called from the window's thread
    /// every finished download froze the window that long (several at once,
    /// 25 seconds). Found by `scripts/ui-shot.py` and a `sample` of the
    /// stuck process.
    fn notify(&self, title: &str, body: &str) {
        // Held while a fullscreen app, a presentation or Focus Assist has
        // the person's attention, and said when it does not.
        let note = Note {
            title: title.to_string(),
            body: body.to_string(),
            path: None,
        };
        if let Some(note) = crate::quiet::offer(note) {
            self.say(note);
        }
    }

    fn tick(&self) {
        for note in crate::quiet::due_notes() {
            self.say(note);
        }
    }

    /// As `notify`, and a click on it opens the file, where the system
    /// says which was clicked: Linux's notification servers and Windows'
    /// toasts do. macOS says so only to a sender that waits on it in step,
    /// which can hold a thread for as long as the notification is kept, so
    /// there a click brings dm forward, as the system does by itself.
    fn notify_file(&self, title: &str, body: &str, path: &str) {
        let note = Note {
            title: title.to_string(),
            body: body.to_string(),
            path: Some(path.to_string()),
        };
        if let Some(note) = crate::quiet::offer(note) {
            self.say(note);
        }
    }

    fn chime(&self) {
        // A sound that comes late is only noise: not while the person is busy.
        if !crate::quiet::quiet_now() {
            crate::power::chime();
        }
    }

    fn alert(&self) {
        if !crate::quiet::quiet_now() {
            crate::power::alert();
        }
    }

    fn play_file(&self, path: &str) {
        if !crate::quiet::quiet_now() && !crate::power::play_file(path) {
            crate::power::chime();
        }
    }

    fn clipboard(&self) -> Option<String> {
        self.window.clipboard()
    }

    fn reveal(&self, path: &str) -> Result<(), String> {
        shell::reveal_in_folder(path)
    }

    fn open(&self, path: &str) -> Result<(), String> {
        shell::open_file(path)
    }

    fn open_later(&self, path: &str, report: Report) {
        let path = path.to_string();
        std::thread::spawn(move || {
            if let Err(e) = shell::open_file(&path) {
                report(e);
            }
        });
    }

    fn reveal_later(&self, path: &str, report: Report) {
        let path = path.to_string();
        std::thread::spawn(move || {
            // The file manager's own "show this file", selected in its
            // folder, where one answers on the session bus; otherwise the
            // folder alone.
            #[cfg(target_os = "linux")]
            if Path::new(&path).exists() && show_item(&path) {
                return;
            }
            if let Err(e) = shell::reveal_in_folder(&path) {
                report(e);
            }
        });
    }

    fn power(&self, action: WhenDone, report: Report) -> Result<(), String> {
        crate::power::run(action, report)
    }

    fn choose_app(&self) -> Option<PathBuf> {
        self.window.choose_app()
    }

    fn open_with(&self, path: &str, app: &Path) -> Result<(), String> {
        shell::open_with(path, app)
    }

    fn choose_folder(&self) -> Option<PathBuf> {
        self.window.choose_folder()
    }

    /// On Linux the chooser is the desktop portal's, else zenity's; with
    /// neither, the window's answers as if cancelled. Elsewhere the system
    /// has one.
    fn can_choose_folder(&self) -> bool {
        #[cfg(target_os = "linux")]
        return dm_shell::app::backend::chooser_here(
            file_chooser_portal(),
            std::env::var_os("PATH").as_deref(),
        );
        #[cfg(not(target_os = "linux"))]
        true
    }

    fn choose_file(&self, save: bool, suggested: &str) -> Option<PathBuf> {
        self.window.choose_file(save, suggested)
    }
}

/// Shows `path` selected in its folder through the freedesktop file
/// manager interface, which the usual file managers answer (and are
/// started for). False where none is there to ask.
#[cfg(target_os = "linux")]
fn show_item(path: &str) -> bool {
    const NAME: &str = "org.freedesktop.FileManager1";
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let there = zbus::blocking::fdo::DBusProxy::new(&connection).is_ok_and(|bus| {
        let Ok(name) = zbus::names::BusName::try_from(NAME) else {
            return false;
        };
        bus.name_has_owner(name).unwrap_or(false)
            || bus
                .list_activatable_names()
                .is_ok_and(|names| names.iter().any(|n| n.as_str() == NAME))
    });
    if !there {
        return false;
    }
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination(NAME)
        .and_then(|b| b.path("/org/freedesktop/FileManager1"))
        .and_then(|b| b.interface(NAME))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    let Ok(proxy) = proxy else {
        return false;
    };
    proxy
        .call::<_, _, ()>("ShowItems", &(vec![crate::format::file_uri(path)], ""))
        .is_ok()
}

/// Whether the desktop portal offers its file chooser on the session bus.
#[cfg(target_os = "linux")]
fn file_chooser_portal() -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination("org.freedesktop.portal.Desktop")
        .and_then(|b| b.path("/org/freedesktop/portal/desktop"))
        .and_then(|b| b.interface("org.freedesktop.portal.FileChooser"))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    proxy.is_ok_and(|p| p.get_property::<u32>("version").is_ok())
}

/// Says a toast: one whose click opens a finished file, or a plain one.
/// Posted by dm itself on Windows (`dm_desktop::toast`): notify-rust's kept
/// three handles for every toast (gaps row 156).
#[cfg(windows)]
fn say_on_windows(note: Note) {
    let Note { title, body, path } = note;
    // On a thread of its own, as before: the session's first toast starts
    // Windows' runtime, which takes a moment the window should not wait.
    std::thread::spawn(move || {
        let open = path.filter(|p| !p.is_empty()).map(|path| {
            move || {
                let _ = shell::open_file(&path);
            }
        });
        let _ = crate::toast::show(&title, &body, open);
    });
}
