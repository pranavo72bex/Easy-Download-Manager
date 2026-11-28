// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What Windows opens magnet links and `.torrent` files with: EDM's registration, its listing
//! in Default apps, and taking them from another program and giving them back.

use std::path::{Path, PathBuf};

/// The key Windows opens a `magnet:` link through.
const MAGNET_KEY: &str = r"HKCU\Software\Classes\magnet";

fn reg_args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// `reg.exe` argument lists, in the order to run them, that make EDM what Windows opens a
/// `magnet:` link with: the protocol's own key, a marker value (`EDM`) saying the key is EDM's
/// own, and the command, `"<program>" "%1"`.
pub fn magnet_keys(exe: &Path) -> Vec<Vec<String>> {
    let command = format!("\"{}\" \"%1\"", exe.display());
    let open = format!(r"{MAGNET_KEY}\shell\open\command");
    vec![
        reg_args(&[
            "add",
            MAGNET_KEY,
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            "URL:Magnet Protocol",
            "/f",
        ]),
        reg_args(&[
            "add",
            MAGNET_KEY,
            "/v",
            "URL Protocol",
            "/t",
            "REG_SZ",
            "/d",
            "",
            "/f",
        ]),
        reg_args(&[
            "add", MAGNET_KEY, "/v", "EDM", "/t", "REG_SZ", "/d", "1", "/f",
        ]),
        reg_args(&["add", &open, "/ve", "/t", "REG_SZ", "/d", &command, "/f"]),
    ]
}

/// Whether the magnet key is EDM's own: it carries the marker `magnet_keys` writes.
pub(super) fn magnet_is_ours(reg: &mut impl FnMut(&[String]) -> Result<(), String>) -> bool {
    reg(&reg_args(&["query", MAGNET_KEY, "/v", "EDM"])).is_ok()
}

/// Makes EDM what Windows opens a magnet link with, so a click the extension does not catch
/// (a button the page's script handles, a browser without the extension, another program's
/// link) still starts the download in EDM. Another program's own registration is left alone:
/// returns `Ok(false)` then, and `Ok(true)` once EDM's is written (a path that moved is
/// rewritten, since the key is EDM's).
pub fn register_magnet_windows(
    exe: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<bool, String> {
    if !exe.is_absolute() {
        return Err(format!(
            "{} is not a program Windows could start",
            exe.display()
        ));
    }
    let present = reg(&reg_args(&["query", MAGNET_KEY])).is_ok();
    if present && !magnet_is_ours(&mut reg) {
        return Ok(false);
    }
    for args in magnet_keys(exe) {
        reg(&args)?;
    }
    Ok(true)
}

/// Removes EDM's magnet registration, only if it is EDM's. True when it was removed.
pub fn unregister_magnet_windows(mut reg: impl FnMut(&[String]) -> Result<(), String>) -> bool {
    magnet_is_ours(&mut reg) && reg(&reg_args(&["delete", MAGNET_KEY, "/f"])).is_ok()
}

/// The key that says which program a `.torrent` file opens with.
const TORRENT_EXTENSION_KEY: &str = r"HKCU\Software\Classes\.torrent";
/// EDM's own kind of file the extension names (a ProgId).
const TORRENT_PROGID: &str = "EDM.torrent";
const TORRENT_PROGID_KEY: &str = r"HKCU\Software\Classes\EDM.torrent";

/// `reg.exe` argument lists, in order, that make a `.torrent` file open with EDM: EDM's ProgId
/// (its name, a marker value `EDM`, and the command `"<program>" "%1"`), then the extension
/// pointing at it with its content type.
pub fn torrent_keys(exe: &Path) -> Vec<Vec<String>> {
    let command = format!("\"{}\" \"%1\"", exe.display());
    let open = format!(r"{TORRENT_PROGID_KEY}\shell\open\command");
    vec![
        reg_args(&[
            "add",
            TORRENT_PROGID_KEY,
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            "BitTorrent file",
            "/f",
        ]),
        reg_args(&[
            "add",
            TORRENT_PROGID_KEY,
            "/v",
            "EDM",
            "/t",
            "REG_SZ",
            "/d",
            "1",
            "/f",
        ]),
        reg_args(&["add", &open, "/ve", "/t", "REG_SZ", "/d", &command, "/f"]),
        reg_args(&[
            "add",
            TORRENT_EXTENSION_KEY,
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            TORRENT_PROGID,
            "/f",
        ]),
        reg_args(&[
            "add",
            TORRENT_EXTENSION_KEY,
            "/v",
            "Content Type",
            "/t",
            "REG_SZ",
            "/d",
            "application/x-bittorrent",
            "/f",
        ]),
    ]
}

/// Whether EDM's `.torrent` ProgId is there with the marker `torrent_keys` writes.
pub(super) fn torrent_is_ours(reg: &mut impl FnMut(&[String]) -> Result<(), String>) -> bool {
    reg(&reg_args(&["query", TORRENT_PROGID_KEY, "/v", "EDM"])).is_ok()
}

/// Makes a `.torrent` file opened from Explorer (or a browser's "Open") start in EDM. As for
/// magnet links, the extension is never taken from another program: when `.torrent` is already
/// set and EDM's marked ProgId is not there, nothing is written (`Ok(false)`). EDM's own is
/// rewritten, so a program that moved is followed.
pub fn register_torrent_windows(
    exe: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<bool, String> {
    if !exe.is_absolute() {
        return Err(format!(
            "{} is not a program Windows could start",
            exe.display()
        ));
    }
    let present = reg(&reg_args(&["query", TORRENT_EXTENSION_KEY])).is_ok();
    if present && !torrent_is_ours(&mut reg) {
        return Ok(false);
    }
    for args in torrent_keys(exe) {
        reg(&args)?;
    }
    Ok(true)
}

/// Removes EDM's `.torrent` registration, only if it is EDM's. True when it was removed.
pub fn unregister_torrent_windows(mut reg: impl FnMut(&[String]) -> Result<(), String>) -> bool {
    torrent_is_ours(&mut reg)
        && reg(&reg_args(&["delete", TORRENT_EXTENSION_KEY, "/f"])).is_ok()
        && reg(&reg_args(&["delete", TORRENT_PROGID_KEY, "/f"])).is_ok()
}

/// Where Windows lists the programs a person can choose in Settings › Default apps.
const REGISTERED_APPLICATIONS_KEY: &str = r"HKCU\Software\RegisteredApplications";
/// EDM's own key, holding what it can open.
const EDM_APP_KEY: &str = r"HKCU\Software\EDM";
const CAPABILITIES_KEY: &str = r"HKCU\Software\EDM\Capabilities";
/// The same, as `RegisteredApplications` names it (from the hive's root).
const CAPABILITIES_PATH: &str = r"Software\EDM\Capabilities";
/// EDM's own ProgIds for Default apps: apart from the `magnet` key and `EDM.torrent`, so being
/// listed takes nothing from another program and nothing of EDM's own registration.
const MAGNET_LISTED_KEY: &str = r"HKCU\Software\Classes\EDM.magnet";
const TORRENT_LISTED_KEY: &str = r"HKCU\Software\Classes\EDM.torrentfile";

/// `reg.exe` argument lists, in order, that put EDM in Settings › Default apps for magnet
/// links and `.torrent` files (spec T5): a ProgId for each with its command, a
/// `Capabilities` key naming them, and EDM's line in `RegisteredApplications`. Nothing here
/// makes EDM the default of anything: Windows' "How do you want to open this?" and Default
/// apps then offer EDM, and the person chooses.
pub fn default_apps_keys(exe: &Path) -> Vec<Vec<String>> {
    let command = format!("\"{}\" \"%1\"", exe.display());
    let magnet_open = format!(r"{MAGNET_LISTED_KEY}\shell\open\command");
    let torrent_open = format!(r"{TORRENT_LISTED_KEY}\shell\open\command");
    let urls = format!(r"{CAPABILITIES_KEY}\URLAssociations");
    let files = format!(r"{CAPABILITIES_KEY}\FileAssociations");
    let add = |key: &str, value: Option<&str>, data: &str| {
        let mut args = vec!["add".to_string(), key.to_string()];
        match value {
            Some(name) => args.extend(["/v".to_string(), name.to_string()]),
            None => args.push("/ve".to_string()),
        }
        args.extend(reg_args(&["/t", "REG_SZ", "/d", data, "/f"]));
        args
    };
    vec![
        add(MAGNET_LISTED_KEY, None, "URL:Magnet link"),
        add(MAGNET_LISTED_KEY, Some("URL Protocol"), ""),
        add(&magnet_open, None, &command),
        add(TORRENT_LISTED_KEY, None, "BitTorrent file"),
        add(&torrent_open, None, &command),
        add(CAPABILITIES_KEY, Some("ApplicationName"), "EDM"),
        add(
            CAPABILITIES_KEY,
            Some("ApplicationDescription"),
            "Downloads files, videos and torrents",
        ),
        add(&urls, Some("magnet"), "EDM.magnet"),
        add(&files, Some(".torrent"), "EDM.torrentfile"),
        add(REGISTERED_APPLICATIONS_KEY, Some("EDM"), CAPABILITIES_PATH),
    ]
}

/// Whether EDM is in Default apps' list already.
pub(super) fn listed_in_default_apps(
    reg: &mut impl FnMut(&[String]) -> Result<(), String>,
) -> bool {
    reg(&reg_args(&[
        "query",
        REGISTERED_APPLICATIONS_KEY,
        "/v",
        "EDM",
    ]))
    .is_ok()
}

/// Puts EDM in Settings › Default apps ([`default_apps_keys`]). Every key is EDM's own, so it is
/// rewritten whatever is there, and a program that moved is followed.
pub fn list_in_default_apps(
    exe: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    if !exe.is_absolute() {
        return Err(format!(
            "{} is not a program Windows could start",
            exe.display()
        ));
    }
    for args in default_apps_keys(exe) {
        reg(&args)?;
    }
    Ok(())
}

/// Takes EDM out of Default apps: its line in `RegisteredApplications`, its `Capabilities` and
/// its two ProgIds. A person's choice of EDM in Default apps then falls back to Windows' own.
/// True when every one was removed (or was not there).
pub fn unlist_from_default_apps(mut reg: impl FnMut(&[String]) -> Result<(), String>) -> bool {
    let mut all = true;
    if listed_in_default_apps(&mut reg) {
        all &= reg(&reg_args(&[
            "delete",
            REGISTERED_APPLICATIONS_KEY,
            "/v",
            "EDM",
            "/f",
        ]))
        .is_ok();
    }
    for key in [EDM_APP_KEY, MAGNET_LISTED_KEY, TORRENT_LISTED_KEY] {
        if reg(&reg_args(&["query", key])).is_ok() {
            all &= reg(&reg_args(&["delete", key, "/f"])).is_ok();
        }
    }
    all
}

/// Who Windows opens a kind of link or file with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// EDM: its registration, with its marker.
    Dm,
    /// Another program's registration.
    Other,
    /// None at all.
    Nobody,
    /// The program chosen in Windows' Default apps, which decides over any
    /// program's registration and which no program may write.
    WindowsChoice,
}

/// Where Windows keeps a person's own choice for `magnet:` links.
const MAGNET_CHOICE_KEY: &str =
    r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\magnet\UserChoice";
/// Where Windows keeps a person's own choice for `.torrent` files.
const TORRENT_CHOICE_KEY: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.torrent\UserChoice";

/// Who opens magnet links: Windows' own choice where one is made, else EDM,
/// another program or nobody, by the registration and EDM's marker.
pub fn magnet_owner(mut reg: impl FnMut(&[String]) -> Result<(), String>) -> Owner {
    if reg(&reg_args(&["query", MAGNET_CHOICE_KEY])).is_ok() {
        Owner::WindowsChoice
    } else if magnet_is_ours(&mut reg) {
        Owner::Dm
    } else if reg(&reg_args(&["query", MAGNET_KEY])).is_ok() {
        Owner::Other
    } else {
        Owner::Nobody
    }
}

/// Who opens `.torrent` files, as `magnet_owner` reads it.
pub fn torrent_owner(mut reg: impl FnMut(&[String]) -> Result<(), String>) -> Owner {
    if reg(&reg_args(&["query", TORRENT_CHOICE_KEY])).is_ok() {
        Owner::WindowsChoice
    } else if torrent_is_ours(&mut reg) {
        Owner::Dm
    } else if reg(&reg_args(&["query", TORRENT_EXTENSION_KEY])).is_ok() {
        Owner::Other
    } else {
        Owner::Nobody
    }
}

/// Makes EDM what opens magnet links, at a person's asking, though another
/// program has them: that program's key is first written to `backup` (a
/// `.reg` file, `reg export`), so `give_back_magnet_windows` can put it back.
pub fn take_magnet_windows(
    exe: &Path,
    backup: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    if !exe.is_absolute() {
        return Err(format!(
            "{} is not a program Windows could start",
            exe.display()
        ));
    }
    let backup_text = backup.display().to_string();
    if reg(&reg_args(&["query", MAGNET_KEY])).is_ok() && !magnet_is_ours(&mut reg) {
        reg(&reg_args(&["export", MAGNET_KEY, &backup_text, "/y"]))?;
        reg(&reg_args(&["delete", MAGNET_KEY, "/f"]))?;
    }
    for args in magnet_keys(exe) {
        reg(&args)?;
    }
    Ok(())
}

/// Gives magnet links back to the program that had them before EDM took
/// them: EDM's key removed, `backup` imported, and then removed.
pub fn give_back_magnet_windows(
    backup: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    if magnet_is_ours(&mut reg) {
        reg(&reg_args(&["delete", MAGNET_KEY, "/f"]))?;
    }
    reg(&reg_args(&["import", &backup.display().to_string()]))?;
    let _ = std::fs::remove_file(backup);
    Ok(())
}

/// Makes EDM what opens `.torrent` files, at a person's asking, though
/// another program has them; the extension's key is first written to
/// `backup`, as for magnet links.
pub fn take_torrent_windows(
    exe: &Path,
    backup: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    if !exe.is_absolute() {
        return Err(format!(
            "{} is not a program Windows could start",
            exe.display()
        ));
    }
    let backup_text = backup.display().to_string();
    if reg(&reg_args(&["query", TORRENT_EXTENSION_KEY])).is_ok() && !torrent_is_ours(&mut reg) {
        reg(&reg_args(&[
            "export",
            TORRENT_EXTENSION_KEY,
            &backup_text,
            "/y",
        ]))?;
        reg(&reg_args(&["delete", TORRENT_EXTENSION_KEY, "/f"]))?;
    }
    for args in torrent_keys(exe) {
        reg(&args)?;
    }
    Ok(())
}

/// Gives `.torrent` files back to the program that had them before EDM.
pub fn give_back_torrent_windows(
    backup: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    if torrent_is_ours(&mut reg) {
        reg(&reg_args(&["delete", TORRENT_EXTENSION_KEY, "/f"]))?;
        reg(&reg_args(&["delete", TORRENT_PROGID_KEY, "/f"]))?;
    }
    reg(&reg_args(&["import", &backup.display().to_string()]))?;
    let _ = std::fs::remove_file(backup);
    Ok(())
}

/// Where what another program had for magnet links is kept while EDM has them.
pub fn magnet_backup(home: &Path) -> PathBuf {
    home.join(edm_core::paths::MAGNET_BACKUP)
}

/// Where what another program had for `.torrent` files is kept while EDM has them.
pub fn torrent_backup(home: &Path) -> PathBuf {
    home.join(edm_core::paths::TORRENT_BACKUP)
}
