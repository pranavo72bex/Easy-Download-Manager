// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where the browsers on Windows look for dm's host: a registry key each, naming a manifest
//! in dm's own folder.

use std::path::{Path, PathBuf};

use dm_protocol::NATIVE_HOST;

use super::{manifest, Allowed, Family};

/// Where browsers on Windows look for a host: a key under
/// `HKEY_CURRENT_USER` whose default value is the manifest's path. Windows
/// reads no manifest directories, so a key is written for each browser
/// whether or not it is installed yet; one installed later then finds dm.
pub const WINDOWS_KEYS: &[(&str, Family)] = &[
    (
        r"Software\Google\Chrome\NativeMessagingHosts",
        Family::Chromium,
    ),
    (r"Software\Chromium\NativeMessagingHosts", Family::Chromium),
    (
        r"Software\Microsoft\Edge\NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
        Family::Chromium,
    ),
    (r"Software\Vivaldi\NativeMessagingHosts", Family::Chromium),
    (r"Software\Mozilla\NativeMessagingHosts", Family::Firefox),
];

/// The manifest file of each family on Windows, under `data` (dm's folder in
/// `%LOCALAPPDATA%`). The two families read different keys, so they are two
/// files.
pub fn windows_manifest(data: &Path, family: Family) -> PathBuf {
    let sub = match family {
        Family::Chromium => "chromium",
        Family::Firefox => "firefox",
    };
    data.join("NativeMessagingHosts")
        .join(sub)
        .join(format!("{NATIVE_HOST}.json"))
}

/// The full registry key of dm's host under one browser's key.
fn windows_key(key: &str) -> String {
    format!(r"HKCU\{key}\{NATIVE_HOST}")
}

/// `reg.exe` arguments that point one browser at dm's manifest.
pub fn reg_add(key: &str, manifest: &Path) -> Vec<String> {
    [
        "add",
        &windows_key(key),
        "/ve",
        "/t",
        "REG_SZ",
        "/d",
        &manifest.to_string_lossy(),
        "/f",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// `reg.exe` arguments that remove dm from one browser.
pub fn reg_delete(key: &str) -> Vec<String> {
    ["delete", &windows_key(key), "/f"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Registers on Windows: both manifests written under `data`, then a key
/// for every browser. `reg` runs `reg.exe` with the arguments given; a test
/// passes one that records them. Returns what was written, files then keys.
pub fn register_windows(
    data: &Path,
    exe: &Path,
    allowed: &Allowed,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<Vec<PathBuf>, String> {
    if !exe.is_absolute() || !exe.is_file() {
        return Err(format!(
            "{} is not a program a browser could start",
            exe.display()
        ));
    }
    let mut written = Vec::new();
    for family in [Family::Chromium, Family::Firefox] {
        let file = windows_manifest(data, family);
        let text = serde_json::to_string_pretty(&manifest(family, exe, allowed))
            .map_err(|e| e.to_string())?;
        file.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| dm_core::write_atomic(&file, format!("{text}\n").as_bytes()))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        written.push(file);
    }
    for (key, family) in WINDOWS_KEYS {
        reg(&reg_add(key, &windows_manifest(data, *family)))?;
        written.push(PathBuf::from(windows_key(key)));
    }
    Ok(written)
}

/// Removes dm from every browser on Windows. Returns what was removed.
pub fn unregister_windows(
    data: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Vec<PathBuf> {
    let keys = WINDOWS_KEYS
        .iter()
        .filter(|(key, _)| reg(&reg_delete(key)).is_ok())
        .map(|(key, _)| PathBuf::from(windows_key(key)));
    let files = [Family::Chromium, Family::Firefox]
        .into_iter()
        .map(|family| windows_manifest(data, family))
        .filter(|file| std::fs::remove_file(file).is_ok());
    keys.collect::<Vec<_>>().into_iter().chain(files).collect()
}

/// Carries out one of `reg.exe`'s argument lists: `add`, `delete` and `query` through the
/// registry's own API ([`crate::registry`]), with no program started; `export` and `import`, the
/// `.reg` copy kept of another program's key, by `reg.exe`.
#[cfg(windows)]
pub fn run_reg(args: &[String]) -> Result<(), String> {
    crate::registry::run(args)
}

/// Away from Windows there is no registry.
#[cfg(not(windows))]
pub fn run_reg(args: &[String]) -> Result<(), String> {
    Err(format!(
        "reg {}: there is no registry here",
        args.first().map(String::as_str).unwrap_or_default()
    ))
}

/// dm's folder in `%LOCALAPPDATA%`, where its Windows manifests live.
pub(super) fn windows_data(home: &Path) -> PathBuf {
    dm_core::folders::local_data()
        .unwrap_or_else(|| home.join("AppData").join("Local"))
        .join("dm")
}
