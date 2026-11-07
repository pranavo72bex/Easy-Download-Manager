// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Helper programs started without a console window, and without an AppImage's folders.

/// On Windows, starts a helper program without a console window: dm's
/// release build has none of its own, so each `powershell` or `reg` it
/// ran flashed one up. On Linux, takes the AppImage's own
/// folders out of what the helper inherits ([`appimage_env`]).
pub fn no_window(command: &mut std::process::Command) -> &mut std::process::Command {
    for (name, value) in appimage_env() {
        match value {
            Some(v) => command.env(name, v),
            None => command.env_remove(name),
        };
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// [`no_window`] for tokio's commands.
pub fn no_window_async(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    for (name, value) in appimage_env() {
        match value {
            Some(v) => command.env(name, v),
            None => command.env_remove(name),
        };
    }
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The variables the AppImage's start program (`AppRun`) points into the
/// AppImage, as a helper should have them: without the AppImage's folders,
/// or unset where nothing else is left. `PYTHONHOME` set to the AppImage
/// made a person's own tools, scripts run by the system's Python, stop
/// with "Fatal Python error: Failed to import encodings module … <no Python
/// frame>"; `LD_LIBRARY_PATH` would hand it the AppImage's libraries.
/// The AppImage's folder is `APPDIR`, which its runtime sets, or else the
/// one dm's program sits in as `<folder>/usr/bin/dm-desktop` (an AppImage
/// unpacked and started by its `AppRun`). Nothing outside an AppImage.
pub fn appimage_env() -> Vec<(&'static str, Option<std::ffi::OsString>)> {
    let appdir = std::env::var_os("APPDIR")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let exe = std::env::current_exe().ok()?;
            let usr = exe.parent()?.parent()?;
            (usr.file_name()? == "usr" && usr.parent()?.join("AppRun").exists())
                .then(|| usr.parent().map(std::path::Path::to_path_buf))
                .flatten()
        });
    match appdir {
        Some(appdir) => without_appdir(&appdir, |name| std::env::var_os(name)),
        None => Vec::new(),
    }
}

pub(crate) fn without_appdir(
    appdir: &std::path::Path,
    get: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(&'static str, Option<std::ffi::OsString>)> {
    const SET_BY_APPRUN: [&str; 11] = [
        "PYTHONHOME",
        "PYTHONPATH",
        "PYTHONDONTWRITEBYTECODE",
        "LD_LIBRARY_PATH",
        "PERLLIB",
        "GSETTINGS_SCHEMA_DIR",
        "QT_PLUGIN_PATH",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "XDG_DATA_DIRS",
        "PATH",
    ];
    let mut out = Vec::new();
    for name in SET_BY_APPRUN {
        let Some(value) = get(name) else { continue };
        if name == "PYTHONDONTWRITEBYTECODE" {
            out.push((name, None));
            continue;
        }
        let kept: Vec<std::path::PathBuf> = std::env::split_paths(&value)
            .filter(|p| !p.as_os_str().is_empty() && !p.starts_with(appdir))
            .collect();
        let rebuilt = if kept.is_empty() {
            None
        } else {
            std::env::join_paths(kept).ok()
        };
        if rebuilt.as_ref() != Some(&value) {
            out.push((name, rebuilt));
        }
    }
    out
}
