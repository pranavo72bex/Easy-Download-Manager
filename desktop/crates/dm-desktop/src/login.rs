use super::*;

/// The login item for `exe`, launched into the tray.
pub(super) fn login_launcher(exe: &std::path::Path) -> Result<auto_launch::AutoLaunch, String> {
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
        .map_err(|e| e.to_string())
}

/// The program the login item `auto-launch` wrote starts: on macOS its
/// launch agent's plist, on Windows the Run key's value. `None` when there
/// is none or it cannot be read, which is not taken as a reason to rewrite
/// it.
pub(super) fn login_item_program() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        let plist = std::path::PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/LaunchAgents/dm.plist");
        let text = std::fs::read_to_string(plist).ok()?;
        return dm_desktop::autostart::plist_program(&text);
    }
    #[cfg(windows)]
    {
        use dm_desktop::autostart::{registry, run_value_program, RUN_KEY};
        registry::string(RUN_KEY, "dm").and_then(|value| run_value_program(&value))
    }
    #[cfg(not(windows))]
    None
}

/// Whether the person turned dm's login item off outside dm: on Windows,
/// in Task Manager's Startup tab. Elsewhere there is no such switch.
pub(super) fn login_item_turned_off_outside() -> bool {
    #[cfg(windows)]
    {
        use dm_desktop::autostart::{registry, turned_off_in_task_manager, STARTUP_APPROVED_KEY};
        registry::binary(STARTUP_APPROVED_KEY, "dm")
            .is_some_and(|approved| turned_off_in_task_manager(&approved))
    }
    #[cfg(not(windows))]
    false
}

/// `--unregister`'s other half: dm's login item, removed, and what was
/// done said. Only this copy's, or one whose program is gone: uninstalling
/// a second copy must not take the login item from the installed one.
/// The browsers' manifests are `register::from_args`'s.
pub(super) fn remove_login_item() {
    use dm_desktop::autostart::{self, Removal};
    let removed: Result<Removal, String> = if cfg!(target_os = "linux") {
        match (autostart::entry_path(), autostart::program()) {
            (Some(path), Ok(this)) => autostart::remove_at(&path, &this, |p| p.exists()),
            (None, _) => Ok(Removal::None),
            (_, Err(e)) => Err(e.to_string()),
        }
    } else {
        std::env::current_exe()
            .map_err(|e| e.to_string())
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
                launcher.disable().map_err(|e| e.to_string())?;
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

/// `--unregister`'s last part: the login setting turned off, so the next
/// launch does not put the login item back, and a dm still running asked
/// to end, which otherwise wrote the browser manifest and the login entry
/// straight back.
pub(super) fn unregister_the_rest() {
    let (mut saved, _) = settings::Settings::load();
    if saved.start_on_login {
        saved.start_on_login = false;
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
