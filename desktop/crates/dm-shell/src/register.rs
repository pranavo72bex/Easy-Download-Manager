// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Telling the browsers where dm is: `dm-desktop --register` and
//! `--unregister`.
//!
//! A browser only launches a native messaging host it has a manifest for, in
//! a directory it looks in, naming the extensions allowed to talk to it. That
//! last part is the security boundary — without it any extension could start
//! a program on this machine — so a manifest names dm's extension and no
//! other. The unpacked Chrome build's id is fixed by the key in its manifest
//! and Firefox's by `browser_specific_settings`, so both are registered with
//! no id typed; a store build's id is named on the command line.
//!
//! The binary registers itself: the path written is the running program's
//! (or `DM_ENGINE`'s), which is the binary the browser will start.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use std::path::{Path, PathBuf};

#[cfg(test)]
use dm_protocol::NATIVE_HOST;
use dm_protocol::{CHROME_EXTENSION_ID, EDGE_EXTENSION_ID, FIREFOX_EXTENSION_ID};

mod browsers;
mod handlers;
mod manifests;
mod windows_keys;

pub use browsers::{
    installed_browsers, installed_where, places, Family, Installed, Place, Programs, Store,
};
pub use handlers::{
    default_apps_keys, give_back_magnet_windows, give_back_torrent_windows, list_in_default_apps,
    magnet_backup, magnet_keys, magnet_owner, register_magnet_windows, register_torrent_windows,
    take_magnet_windows, take_torrent_windows, torrent_backup, torrent_keys, torrent_owner,
    unlist_from_default_apps, unregister_magnet_windows, unregister_torrent_windows, Owner,
};
use handlers::{listed_in_default_apps, magnet_is_ours, torrent_is_ours};
pub use manifests::{manifest, mounted_read_only, protected, transient, transient_on, Allowed};
use windows_keys::windows_data;
pub use windows_keys::{
    reg_add, reg_delete, register_windows, run_reg, unregister_windows, windows_manifest,
    WINDOWS_KEYS,
};

/// Writes a manifest for every installed browser, naming `exe` and the
/// extensions `allowed`. Returns the files written. A browser counts as
/// installed when it has run or its program is in `programs`; a folder is
/// made for no other.
pub fn register(
    home: &Path,
    exe: &Path,
    allowed: &Allowed,
    programs: &Programs,
) -> Result<Vec<PathBuf>, String> {
    if cfg!(target_os = "windows") {
        let written = register_windows(&windows_data(home), exe, allowed, run_reg)?;
        // Best effort, and never over another program's own: a failure here
        // does not undo the browsers' registration.
        let _ = register_magnet_windows(exe, run_reg);
        let _ = register_torrent_windows(exe, run_reg);
        let _ = list_in_default_apps(exe, run_reg);
        return Ok(written);
    }
    if !exe.is_absolute() || !exe.is_file() {
        return Err(format!(
            "{} is not a program a browser could start",
            exe.display()
        ));
    }
    // Before anything is written: a manifest pointing at a binary the browser
    // may not run is the failure this exists to prevent.
    if transient(exe) {
        return Err(format!(
            "{}\nis a copy macOS runs from a disk image or from a temporary copy it made \
             (AppTranslocation), gone by the next launch. Drag dm to Applications (or any \
             folder on a disk you can write to) and open it from there, so your browser can \
             hand it downloads.",
            exe.display()
        ));
    }
    if protected(exe, home, cfg!(target_os = "macos")) {
        return Err(format!(
            "{}\nis inside a folder macOS protects, and a browser will not be allowed to run it. \
             The browser would open the connection, dm would never start, and nothing would say \
             why.\n\nInstall dm (make install, or the app in /Applications) and register that copy, \
             or set DM_ENGINE to a copy outside ~/Downloads, ~/Desktop and ~/Documents.",
            exe.display()
        ));
    }

    let mut written = Vec::new();
    for place in places(home) {
        if !place.installed(programs) {
            continue; // that browser is not installed
        }
        let text = serde_json::to_string_pretty(&manifest(place.family, exe, allowed))
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&place.dir)
            .and_then(|()| dm_core::write_atomic(&place.manifest(), format!("{text}\n").as_bytes()))
            .map_err(|e| format!("{}: {e}", place.manifest().display()))?;
        written.push(place.manifest());
    }
    if written.is_empty() {
        return Err("no browser was found to register with".into());
    }
    Ok(written)
}

/// Removes dm's manifest from every browser, installed or not. Returns the
/// files removed.
pub fn unregister(home: &Path) -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        let _ = unregister_magnet_windows(run_reg);
        let _ = unregister_torrent_windows(run_reg);
        let _ = unlist_from_default_apps(run_reg);
        return unregister_windows(&windows_data(home), run_reg);
    }
    places(home)
        .into_iter()
        .map(|place| place.manifest())
        .filter(|file| std::fs::remove_file(file).is_ok())
        .collect()
}

/// Registers this copy of dm with the browsers if they do not already start
/// it, so an installed app works with the extension without a terminal.
///
/// Run each time the installed app opens. A browser whose manifest already
/// names `exe` and allows this version's extension is left alone; one that
/// names another copy (the app moved, or an older install), none, or does
/// not allow this version's extension id (an upgrade across an id change:
/// 0.1.0's Firefox id was `dm@pranavo72bex.github.io`) is written, keeping
/// any extension ids already allowed there — the retired one included,
/// which is harmless and keeps an old copy of the extension working until
/// it is replaced. `None` when nothing needed doing, including when no
/// browser is installed.
pub fn ensure(
    home: &Path,
    exe: &Path,
    programs: &Programs,
) -> Result<Option<Vec<PathBuf>>, String> {
    let files: Vec<(PathBuf, Family)> = if cfg!(target_os = "windows") {
        let data = windows_data(home);
        [Family::Chromium, Family::Firefox]
            .into_iter()
            .map(|family| (windows_manifest(&data, family), family))
            .collect()
    } else {
        places(home)
            .into_iter()
            .filter(|place| place.installed(programs))
            .map(|place| (place.manifest(), place.family))
            .collect()
    };
    if files.is_empty() {
        return Ok(None);
    }
    let mut stale = false;
    let mut known = Vec::new();
    for (file, family) in &files {
        let manifest = std::fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        let Some(manifest) = manifest else {
            stale = true;
            continue;
        };
        if manifest.get("path").and_then(|p| p.as_str()) != exe.to_str() {
            stale = true;
        }
        // Every id dm allows of itself: a manifest written before the Edge
        // Add-ons listing had its id refused the extension from that store.
        let (key, current): (&str, Vec<String>) = match family {
            Family::Chromium => (
                "allowed_origins",
                [CHROME_EXTENSION_ID, EDGE_EXTENSION_ID]
                    .iter()
                    .map(|id| format!("chrome-extension://{id}/"))
                    .collect(),
            ),
            Family::Firefox => ("allowed_extensions", vec![FIREFOX_EXTENSION_ID.to_string()]),
        };
        let allows_current = manifest
            .get(key)
            .and_then(|v| v.as_array())
            .is_some_and(|ids| {
                current
                    .iter()
                    .all(|want| ids.iter().any(|id| id.as_str() == Some(want.as_str())))
            });
        if !allows_current {
            stale = true;
        }
        for id in ids_allowed_by(&manifest) {
            if !known.contains(&id) {
                known.push(id);
            }
        }
    }
    if !stale {
        return Ok(None);
    }
    // An id this version would not accept is dropped rather than failing
    // the registration of the ones it would.
    known.retain(|id| Allowed::with(std::slice::from_ref(id)).is_ok());
    let allowed = Allowed::with(&known)?;
    register(home, exe, &allowed, programs).map(Some)
}

/// [`ensure`] for this user, for the copy of dm that is running — or, in an
/// AppImage, the AppImage file, since the program inside it is mounted at a
/// new place every launch.
pub fn ensure_this_copy() -> Result<Option<Vec<PathBuf>>, String> {
    let exe = match std::env::var_os("APPIMAGE") {
        Some(image) => PathBuf::from(image),
        None => std::env::current_exe()
            .and_then(|exe| exe.canonicalize())
            .map(dm_core::plain_path)
            .map_err(|e| format!("could not tell where this program is: {e}"))?,
    };
    if transient(&exe) {
        return Err(
            "dm is running from its disk image or a temporary copy macOS made of it. Drag dm \
             to Applications and open it from there, so your browser can hand it downloads"
                .into(),
        );
    }
    let home = dm_core::folders::home().unwrap_or_default();
    let browsers = ensure(&home, &exe, &Programs::here(&home));
    // Windows' magnet and .torrent handlers, whatever the browsers' manifests needed: `ensure`
    // registers only when a manifest is stale, and a dm installed over an older one at the
    // same path finds its manifests current, so the handlers that version never wrote would
    // never be written (a torrent site's button would do nothing). Two queries a
    // start; written only where dm's marker is missing, and never over another program's.
    // Here, not in `ensure`, so tests of `ensure` never touch the real keys.
    if cfg!(target_os = "windows") {
        refresh_windows_handlers(&exe, run_reg);
    }
    browsers
}

/// Writes dm's magnet and `.torrent` registrations where dm's marker is not yet there (a new
/// install, or an update from a version that wrote none). A registration that is dm's is left
/// as it is: `ensure` rewrites everything when the program has moved.
pub fn refresh_windows_handlers(exe: &Path, mut reg: impl FnMut(&[String]) -> Result<(), String>) {
    if !magnet_is_ours(&mut reg) {
        let _ = register_magnet_windows(exe, &mut reg);
    }
    if !torrent_is_ours(&mut reg) {
        let _ = register_torrent_windows(exe, &mut reg);
    }
    // Settings › Default apps: listed once, as an install would (spec T5).
    if !listed_in_default_apps(&mut reg) {
        let _ = list_in_default_apps(exe, &mut reg);
    }
}

/// The extension ids a manifest allows, as ids: a Chromium manifest names
/// them as origins (`chrome-extension://<id>/`), a Firefox one as ids.
fn ids_allowed_by(manifest: &serde_json::Value) -> Vec<String> {
    ["allowed_origins", "allowed_extensions"]
        .iter()
        .flat_map(|key| {
            manifest
                .get(*key)
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
        })
        .filter_map(|v| v.as_str())
        .map(|id| {
            id.trim_start_matches("chrome-extension://")
                .trim_end_matches('/')
                .to_string()
        })
        .collect()
}

/// Every extension that may start dm's host: the ids built into this
/// version, and any the manifests dm wrote allow (read from the files,
/// whether or not their browser is installed, so a manifest a person edited
/// to allow a build of their own counts).
pub fn allowed_ids(home: &Path) -> Vec<String> {
    let mut ids: Vec<String> = [CHROME_EXTENSION_ID, EDGE_EXTENSION_ID, FIREFOX_EXTENSION_ID]
        .iter()
        .map(|id| id.to_string())
        .collect();
    let files: Vec<PathBuf> = if cfg!(target_os = "windows") {
        let data = windows_data(home);
        [Family::Chromium, Family::Firefox]
            .into_iter()
            .map(|family| windows_manifest(&data, family))
            .collect()
    } else {
        places(home)
            .into_iter()
            .map(|place| place.manifest())
            .collect()
    };
    for file in files {
        let manifest = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        for id in manifest.as_ref().map(ids_allowed_by).unwrap_or_default() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Whether this copy of dm may write the browsers' registration: an
/// installed, release dm — not a development build or a test's dm
/// (`DM_HOME`), which must not take the browsers from the installed one.
pub fn may_claim() -> bool {
    !cfg!(debug_assertions) && dm_core::env::get_os("DM_HOME").is_none()
}

/// [`ensure_this_copy`] when this copy [`may_claim`] the browsers, and
/// nothing otherwise. Run at every launch — the one that hands off to a
/// running dm too, before it hands off — and each time the running
/// dm is asked to show itself, so a browser installed since is registered
/// the next time the person opens dm.
pub fn ensure_if_claimed() -> Result<Option<Vec<PathBuf>>, String> {
    if may_claim() {
        ensure_this_copy()
    } else {
        Ok(None)
    }
}

/// What to say about a registration check, given what was last said: a
/// failure is said once, not again at every opening of the window while it
/// stays the same; one that clears is forgotten, so it is said again if it
/// comes back.
pub fn worth_saying(
    last: &mut Option<String>,
    result: Result<Option<Vec<PathBuf>>, String>,
) -> Option<String> {
    match result {
        Ok(_) => {
            *last = None;
            None
        }
        Err(why) if last.as_deref() == Some(why.as_str()) => None,
        Err(why) => {
            *last = Some(why.clone());
            Some(why)
        }
    }
}

/// The last registration failure said by [`ensure_and_say`], so the same one is not said
/// again at every Show.
static LAST_SAID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// [`ensure_if_claimed`], run as the window starts and each time it is asked to show itself,
/// and what to note of it: a failure, in words, the first time it is met ([`worth_saying`]).
pub fn ensure_and_say() -> Option<String> {
    let result = ensure_if_claimed();
    let mut last = LAST_SAID
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    worth_saying(&mut last, result).map(|why| failed_words(&why))
}

/// A registration failure, as the problems list says it.
pub fn failed_words(why: &str) -> String {
    format!(
        "your browsers could not be told where dm is: {why}. Until they are, the extension \
         leaves downloads in the browser."
    )
}

/// `--register [id…]` or `--unregister`, run and finished with an exit code;
/// `None` when neither was asked for.
pub fn from_args(args: &[String]) -> Option<i32> {
    let home = dm_core::folders::home().unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("--unregister") => {
            let removed = unregister(&home);
            for file in &removed {
                println!("removed {}", file.display());
            }
            if removed.is_empty() {
                println!("dm was not registered with any browser");
            }
            Some(0)
        }
        Some("--register") => {
            let allowed = match Allowed::with(args.get(1..).unwrap_or_default()) {
                Ok(allowed) => allowed,
                Err(why) => {
                    eprintln!("dm: {why}");
                    return Some(2);
                }
            };
            let exe = match dm_core::env::get_os("DM_ENGINE") {
                Some(named) => PathBuf::from(named),
                None => match std::env::current_exe()
                    .and_then(|e| e.canonicalize())
                    .map(dm_core::plain_path)
                {
                    Ok(exe) => exe,
                    Err(e) => {
                        eprintln!("dm: could not tell where this program is: {e}");
                        return Some(1);
                    }
                },
            };
            match register(&home, &exe, &allowed, &Programs::here(&home)) {
                Ok(written) => {
                    for file in &written {
                        println!("registered {}", file.display());
                    }
                    println!(
                        "\nThe browser starts {} for these extensions:\n  {}\n\nRestart the \
                         browser. A download it catches is handed to dm.",
                        exe.display(),
                        allowed
                            .chromium
                            .iter()
                            .chain(&allowed.firefox)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n  ")
                    );
                    Some(0)
                }
                Err(why) => {
                    eprintln!("dm: {why}");
                    Some(1)
                }
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod installed_tests {
    use super::*;

    #[test]
    fn the_browsers_found_are_offered_with_their_stores() {
        let found = installed_where(|bundle, on_path, _| {
            bundle == "Firefox.app" || on_path.contains(&"brave-browser")
        });
        assert_eq!(
            found,
            [
                Installed {
                    name: "Firefox",
                    store: Store::Firefox
                },
                Installed {
                    name: "Brave",
                    store: Store::Chrome
                },
            ]
        );
        assert!(installed_where(|_, _, _| false).is_empty());
        // Windows paths are written with single separators.
        let edge = installed_where(|_, _, under| under == r"Microsoft\Edge\Application\msedge.exe");
        assert_eq!(edge.first().map(|b| b.store), Some(Store::Edge));
    }
}

#[cfg(test)]
mod tests;

/// What only this module can reach: how a browser is told to be installed
/// where no public way in asks on this system (an unpacked Linux browser,
/// Firefox as a Snap), and where the Windows manifests go.
#[cfg(test)]
mod coverage_tests {
    use super::*;

    #[test]
    fn a_browser_unpacked_into_a_folder_of_its_own_counts_only_with_its_program() {
        let root = tempfile::tempdir().unwrap();
        let programs = Programs {
            unpacked: vec![root.path().to_path_buf(), root.path().join("not-there")],
            ..Programs::default()
        };
        // A folder named after it, but without the program inside.
        std::fs::create_dir_all(root.path().join("Firefox-ESR")).unwrap();
        assert!(!programs.unpacked_has(&["firefox"]));
        // Named in any case and any version, with the program in it.
        std::fs::create_dir_all(root.path().join("Firefox-128.0")).unwrap();
        std::fs::write(root.path().join("Firefox-128.0/firefox"), b"").unwrap();
        assert!(programs.unpacked_has(&["firefox"]));
        // Another browser's program in that folder is not this one's.
        assert!(!programs.unpacked_has(&["vivaldi"]));
        // Nowhere to look.
        assert!(!Programs::default().unpacked_has(&["firefox"]));
    }

    #[test]
    fn firefox_as_a_snap_counts_as_installed_beside_an_empty_mozilla_folder() {
        let home = tempfile::tempdir().unwrap();
        let place = Place {
            dir: home.path().join(".mozilla/native-messaging-hosts"),
            family: Family::Firefox,
            programs: &[],
        };
        assert_eq!(place.browser(), home.path().join(".mozilla"));
        assert!(!place.installed(&Programs::default()));
        std::fs::create_dir_all(home.path().join("snap/firefox")).unwrap();
        assert!(place.installed(&Programs::default()));
        // The Snap's folder says nothing of a Chromium browser.
        let chromium = Place {
            family: Family::Chromium,
            ..place.clone()
        };
        assert!(!chromium.installed(&Programs::default()));
        // A place with no parent is its own browser folder.
        let bare = Place {
            dir: PathBuf::from("/"),
            family: Family::Chromium,
            programs: &[],
        };
        assert_eq!(bare.browser(), Path::new("/"));
    }

    #[test]
    fn the_windows_manifests_live_in_dms_own_folder() {
        let home = Path::new("/home/someone");
        let data = windows_data(home);
        assert_eq!(data.file_name(), Some(std::ffi::OsStr::new("dm")));
        assert_eq!(
            windows_manifest(&data, Family::Firefox),
            data.join("NativeMessagingHosts/firefox/com.dm.engine.json")
        );
    }
}
