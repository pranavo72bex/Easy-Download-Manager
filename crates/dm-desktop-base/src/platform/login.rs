// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's login item on macOS and Windows (`auto-launch`'s launch agent and
//! Run key; Linux's entry is `autostart`'s), and `--unregister`'s removal
//! of it. A phone has none (gaps row N17): Android starts no app at login.

use dm_shell::{settings, shell};

#[cfg(not(target_os = "android"))]
use crate::Error;

/// Whether this system has a login item at all: Android starts no app at
/// login, so there dm leaves the setting be (`boot`) and refuses to make one
/// (`desktop`).
pub const HAS_LOGIN_ITEM: bool = !cfg!(target_os = "android");

/// The login item for `exe`, launched into the tray.
#[cfg(not(target_os = "android"))]
pub fn login_launcher(exe: &std::path::Path) -> Result<auto_launch::AutoLaunch, Error> {
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name("dm")
        .set_app_path(&exe.to_string_lossy())
        // The login item launches into the tray, not onto the desktop.
        .set_args(&["--hidden"])
        // The current user's Run key, never the machine's: run elevated,
        // the default wrote HKLM, which dm unelevated can neither see nor
        // remove.
        .set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser)
        .build()
        .map_err(Error::from_display)
}

// Each system's login item, in a file of its own, the same two calls in
// each.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(not(any(target_os = "macos", windows)))]
mod linux;
#[cfg(not(any(target_os = "macos", windows)))]
pub use linux::*;

/// `--unregister`'s other half: dm's login item, removed, and what was
/// done said. Only this copy's, or one whose program is gone: uninstalling
/// a second copy must not take the login item from the installed one.
/// The browsers' manifests are `register::from_args`'s.
#[cfg(not(target_os = "android"))]
pub fn remove_login_item() {
    use crate::autostart::{self, Removal};
    let removed: Result<Removal, Error> = if cfg!(target_os = "linux") {
        match (autostart::entry_path(), autostart::program()) {
            (Some(path), Ok(this)) => autostart::remove_at(&path, &this, |p| p.exists()),
            (None, _) => Ok(Removal::None),
            (_, Err(e)) => Err(Error::from_display(e)),
        }
    } else {
        std::env::current_exe()
            .map_err(Error::from_display)
            .and_then(|exe| {
                let found = login_item_program();
                if let Some(found) = found
                    .as_deref()
                    .filter(|f| !autostart::removes(Some(f), &exe, |p| p.exists()))
                {
                    return Ok(Removal::Other(found.to_path_buf()));
                }
                let launcher = login_launcher(&exe)?;
                // On Windows, unconditionally: a login item turned off in
                // Task Manager reads as not enabled while its Run value is
                // still there, and a missing value is no error.
                let there = found.is_some() || launcher.is_enabled().unwrap_or(true);
                if !there && !cfg!(target_os = "windows") {
                    return Ok(Removal::None);
                }
                launcher.disable().map_err(Error::from_display)?;
                Ok(if there {
                    Removal::Removed(if cfg!(target_os = "macos") {
                        "the login item (~/Library/LaunchAgents/dm.plist)".to_string()
                    } else {
                        r"the login item (HKCU\Software\Microsoft\Windows\CurrentVersion\Run\dm)"
                            .to_string()
                    })
                } else {
                    Removal::None
                })
            })
    };
    match removed {
        Ok(Removal::Removed(what)) => println!("removed {what}"),
        Ok(Removal::None) => println!("dm had no login item"),
        Ok(Removal::Other(program)) => println!(
            "the login item starts another copy of dm, at {}; left alone",
            program.display()
        ),
        Err(why) => eprintln!("dm: the login item could not be removed: {why}"),
    }
}

/// `--unregister`'s other half, on a phone: there is no login item to remove.
#[cfg(target_os = "android")]
pub fn remove_login_item() {
    println!("dm had no login item");
}

/// `--unregister`'s last part: the login setting turned off, so the next
/// launch does not put the login item back, and a dm still running asked
/// to end, which otherwise wrote the browser manifest and the login entry
/// straight back.
pub fn unregister_the_rest() {
    let (mut saved, _) = settings::Settings::load();
    if saved.general.start_on_login {
        saved.general.start_on_login = false;
        match saved.save() {
            Ok(()) => println!("turned off starting dm at login"),
            Err(e) => eprintln!("dm: the login setting could not be turned off: {e}"),
        }
    }
    match shell::ask_running_to_quit(&settings::home()) {
        Ok(true) => println!("asked the running dm to end"),
        Ok(false) => {}
        Err(e) => eprintln!("dm: the running dm could not be asked to end: {e}"),
    }
}
