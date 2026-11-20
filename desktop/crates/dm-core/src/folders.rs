// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where a person's files and the system's folders are, in one place (gaps row X1): each
//! system's answer side by side, so a folder that moves on one system is changed here alone.
//! `cargo xtask layers` refuses the variables and paths these read anywhere else.
//!
//! The ones read from the environment take its lookup as an argument (`*_in`), so a test gives
//! each system's variables without changing the process's own.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

// Android's folders, which the app hands in (gaps row N2); built on the desktop for its tests.
#[cfg(any(target_os = "android", test))]
mod phone;
#[cfg(target_os = "android")]
pub use phone::set_phone_folders;

/// The environment as the process has it.
fn process(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// A variable that is set and not empty, as a path.
fn set(var: &impl Fn(&str) -> Option<OsString>, name: &str) -> Option<PathBuf> {
    var(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The person's home folder, as the system says it (on Windows the profile's folder, whatever
/// `HOME` holds). On Android the app's own files folder, as the app gave it (`phone.rs`).
pub fn home() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        phone::home()
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::home_dir()
    }
}

/// The person's Downloads folder, as the system names it. On Android the folder the app gave
/// for new downloads (`phone.rs`).
pub fn downloads() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        phone::downloads()
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::download_dir()
    }
}

/// The folder for a program's own data on this computer: `%LOCALAPPDATA%` on Windows,
/// `~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` on Linux,
/// the app's own files folder on Android (`phone.rs`).
pub fn local_data() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        phone::home()
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::data_local_dir()
    }
}

/// `$HOME` as the environment holds it, set or not, empty or not: what macOS's launch agents
/// and the login item's checks are written against.
pub fn home_variable() -> Option<PathBuf> {
    home_variable_in(process)
}

/// [`home_variable`] in the environment `var`.
pub fn home_variable_in(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    var("HOME").map(PathBuf::from)
}

/// `$HOME`, else Windows' `%USERPROFILE%`: under which the browsers' own folders are looked for.
pub fn home_or_profile() -> Option<PathBuf> {
    home_or_profile_in(process)
}

/// [`home_or_profile`] in the environment `var`.
pub fn home_or_profile_in(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    var("HOME")
        .or_else(|| var("USERPROFILE"))
        .map(PathBuf::from)
}

/// Linux's folder for a person's settings: `$XDG_CONFIG_HOME`, or `~/.config`.
pub fn xdg_config() -> Option<PathBuf> {
    xdg_config_in(process)
}

/// [`xdg_config`] in the environment `var`.
pub fn xdg_config_in(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    set(&var, "XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// Linux's folders of data, the person's first: `$XDG_DATA_HOME` (or `~/.local/share`), then
/// `$XDG_DATA_DIRS` (or `/usr/local/share` and `/usr/share`), as the XDG base-directory
/// specification says.
pub fn xdg_data() -> Vec<PathBuf> {
    xdg_data_in(process)
}

/// [`xdg_data`] in the environment `var`.
pub fn xdg_data_in(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = set(&var, "XDG_DATA_HOME")
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .into_iter()
        .collect();
    let system = var("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(std::env::split_paths(&system));
    dirs
}

/// macOS's folder of the person's launch agents (`~/Library/LaunchAgents`), under `$HOME`.
pub fn launch_agents() -> Option<PathBuf> {
    launch_agents_in(process)
}

/// [`launch_agents`] in the environment `var`.
pub fn launch_agents_in(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    home_variable_in(var).map(|home| home.join("Library/LaunchAgents"))
}

/// macOS's `Application Support` under the home folder `home`, where the browsers keep theirs.
pub fn application_support(home: &Path) -> PathBuf {
    home.join("Library/Application Support")
}

/// Windows' folders programs are installed in: `%ProgramFiles%`, `%ProgramFiles(x86)%` and
/// `%LOCALAPPDATA%` (a browser installed for one person), those that are set.
pub fn windows_programs() -> Vec<PathBuf> {
    windows_programs_in(process)
}

/// [`windows_programs`] in the environment `var`.
pub fn windows_programs_in(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|name| var(name))
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests;
