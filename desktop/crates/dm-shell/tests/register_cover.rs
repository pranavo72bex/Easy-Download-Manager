// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Registering dm with the browsers, through the ways in a person uses
//! (`--register`, `--unregister`, the check at every launch) and the
//! Windows registry steps, with every file kept under a home of the test's
//! own. The person's real home is never written: `HOME` is pointed at a
//! temporary folder first, and each test checks that it took before it
//! runs anything that writes. The registry steps are given a recorder in
//! place of `reg.exe`, or a stand-in `reg` on `PATH`, so no key is touched.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dm_shell::register::{self, Allowed, Family, Owner, Programs};

/// The environment is the process's: tests that read or set it take turns.
static ENV: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Points `HOME` at `home`, and stops the test there unless every lookup of
/// the home directory now finds it: nothing below may reach the real one.
#[cfg(not(windows))] // Used by the tests that run where HOME is the home directory, which Windows' is not.
fn redirect_home(home: &Path) {
    // SAFETY: under `ENV`, so nothing else in this binary reads the
    // environment while it is set.
    unsafe { std::env::set_var("HOME", home) };
    assert_eq!(
        dirs::home_dir().as_deref(),
        Some(home),
        "HOME did not redirect the home directory; stopping before anything is written"
    );
}

#[cfg(not(windows))] // Used by the tests that run where HOME is the home directory, which Windows' is not.
fn set(name: &str, value: Option<&Path>) {
    // SAFETY: as in `redirect_home`.
    unsafe {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}

/// Puts the environment back as it was when a test ends, however it ends.
#[cfg(not(windows))] // Used by the tests that run where HOME is the home directory, which Windows' is not.
struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);

#[cfg(not(windows))] // Used by the tests that run where HOME is the home directory, which Windows' is not.
impl Restore {
    fn of(names: &[&'static str]) -> Self {
        Restore(
            names
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect(),
        )
    }
}

#[cfg(not(windows))] // Used by the tests that run where HOME is the home directory, which Windows' is not.
impl Drop for Restore {
    fn drop(&mut self) {
        for (name, value) in &self.0 {
            // SAFETY: as in `redirect_home`; dropped before the lock is.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

fn program(dir: &Path) -> PathBuf {
    let exe = dir.join("bin").join("dm-desktop");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
    exe
}

/// Firefox's folder made under `home`, as a browser that has run has it,
/// and where dm's manifest for it goes.
#[cfg(not(windows))] // Used by the tests that run where HOME is the home directory, which Windows' is not.
fn firefox_has_run(home: &Path) -> PathBuf {
    let place = register::places(home)
        .into_iter()
        .find(|p| p.family == Family::Firefox)
        .expect("a place Firefox reads manifests from");
    std::fs::create_dir_all(place.dir.parent().unwrap()).unwrap();
    place.manifest()
}

#[cfg(not(windows))] // Used by the tests that run where HOME is the home directory, which Windows' is not.
fn read(file: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

#[test]
fn this_computers_programs_are_looked_for_on_path_and_in_the_applications_folders() {
    let _env = env_lock();
    let home = Path::new("/home/someone");
    let programs = Programs::here(home);
    let path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    assert_eq!(programs.path, path);
    assert_eq!(
        programs.applications,
        [PathBuf::from("/Applications"), home.join("Applications")]
    );
    // A browser unpacked by hand, in the folders one is usually put in.
    assert!(programs.unpacked.contains(&PathBuf::from("/opt")));
    assert!(programs.unpacked.contains(&home.join(".local/share")));
}

#[cfg(target_os = "macos")]
#[test]
fn an_app_bundle_in_the_home_applications_folder_is_a_browser_offered_the_extension() {
    let _env = env_lock();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("Applications/Arc.app")).unwrap();
    let found = register::installed_browsers(home.path());
    assert!(
        found
            .iter()
            .any(|b| b.name == "Arc" && b.store == register::Store::Chrome),
        "{found:?}"
    );
    // A file of the bundle's name is not an app.
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(other.path().join("Applications")).unwrap();
    std::fs::write(other.path().join("Applications/Arc.app"), b"").unwrap();
    let real_arc = Path::new("/Applications/Arc.app").is_dir();
    assert_eq!(
        register::installed_browsers(other.path())
            .iter()
            .any(|b| b.name == "Arc"),
        real_arc
    );
}

#[test]
fn who_has_torrent_files_is_another_program_when_the_extension_is_set_without_dms_marker() {
    // `.torrent` set, but no dm ProgId and no choice in Windows' Default apps.
    let owner = register::torrent_owner(|args| {
        if args[0] == "query" && args[1].ends_with(r"Classes\.torrent") && args.len() == 2 {
            Ok(())
        } else {
            Err("not there".into())
        }
    });
    assert_eq!(owner, Owner::Other);
    // A choice made in Default apps decides over every registration.
    assert_eq!(register::torrent_owner(|_| Ok(())), Owner::WindowsChoice);
    assert_eq!(
        register::torrent_owner(|_| Err("none".into())),
        Owner::Nobody
    );
}

/// A `reg.exe` that finds no dm marker: every query fails, the rest is done.
fn record_without_marker(ran: &mut Vec<Vec<String>>, args: &[String]) -> Result<(), String> {
    ran.push(args.to_vec());
    if args[0] == "query" {
        Err("not there".into())
    } else {
        Ok(())
    }
}

#[test]
fn giving_back_what_dm_never_had_removes_nothing_and_still_imports_the_backup() {
    let dir = tempfile::tempdir().unwrap();
    type GiveBack = fn(&Path, &mut Vec<Vec<String>>) -> Result<(), String>;
    let magnet: GiveBack = |backup, ran| {
        register::give_back_magnet_windows(backup, |args| record_without_marker(ran, args))
    };
    let torrent: GiveBack = |backup, ran| {
        register::give_back_torrent_windows(backup, |args| record_without_marker(ran, args))
    };
    for (backup, give_back) in [
        (register::magnet_backup(dir.path()), magnet),
        (register::torrent_backup(dir.path()), torrent),
    ] {
        std::fs::write(&backup, b"Windows Registry Editor Version 5.00\n").unwrap();
        let mut ran: Vec<Vec<String>> = Vec::new();
        give_back(&backup, &mut ran).unwrap();
        assert!(!ran.iter().any(|a| a[0] == "delete"), "{ran:?}");
        let import = ran.iter().find(|a| a[0] == "import").expect("imported");
        assert_eq!(import[1], backup.display().to_string());
        assert!(!backup.exists(), "the backup is removed once imported");
    }
    // Where an import fails, that is said and the backup is kept.
    let backup = register::magnet_backup(dir.path());
    std::fs::write(&backup, b"kept").unwrap();
    let failed = register::give_back_magnet_windows(&backup, |args| {
        if args[0] == "import" {
            Err("reg import: cannot".into())
        } else {
            Err("not there".into())
        }
    });
    assert_eq!(failed, Err("reg import: cannot".into()));
    assert!(backup.exists());
}

#[test]
fn the_backups_of_what_another_program_had_are_kept_in_dms_home() {
    let home = Path::new("/data/dm");
    assert_eq!(
        register::magnet_backup(home),
        home.join("magnet-before-dm.reg")
    );
    assert_eq!(
        register::torrent_backup(home),
        home.join("torrent-before-dm.reg")
    );
}

#[test]
fn taking_torrent_files_from_nobody_writes_dms_keys_and_keeps_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let exe = program(dir.path());
    let backup = register::torrent_backup(dir.path());
    let mut ran: Vec<Vec<String>> = Vec::new();
    register::take_torrent_windows(&exe, &backup, |args| {
        ran.push(args.to_vec());
        if args[0] == "query" {
            Err("none".into())
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert!(
        !ran.iter().any(|a| a[0] == "export" || a[0] == "delete"),
        "{ran:?}"
    );
    assert_eq!(ran.iter().filter(|a| a[0] == "add").count(), 5);
    // A key that cannot be written stops it there, and says so.
    let failed = register::take_torrent_windows(&exe, &backup, |args| {
        if args[0] == "add" {
            Err("reg add: denied".into())
        } else {
            Err("none".into())
        }
    });
    assert_eq!(failed, Err("reg add: denied".into()));
    // A program Windows could not start is refused before anything runs.
    let mut ran: Vec<Vec<String>> = Vec::new();
    let refused = register::take_torrent_windows(Path::new("dm.exe"), &backup, |args| {
        ran.push(args.to_vec());
        Ok(())
    });
    assert!(refused.unwrap_err().contains("dm.exe"));
    assert!(ran.is_empty());
}

#[test]
fn a_path_that_cannot_be_asked_about_is_taken_as_writable() {
    // A NUL byte cannot be passed to the system at all.
    assert!(!register::mounted_read_only(Path::new("/tmp/a\0b")));
    // Nor can a path that is not there be asked about.
    assert!(!register::mounted_read_only(Path::new(
        "/no/such/folder/for/dm/to/ask/about"
    )));
}

#[test]
fn the_manifest_names_the_program_and_only_the_extensions_allowed() {
    let exe = Path::new("/opt/dm/dm-desktop");
    let allowed = Allowed::with(&["{0d7a1a4b-3c2d-4e5f-8a9b-0c1d2e3f4a5b}".into()]).unwrap();
    let chromium = register::manifest(Family::Chromium, exe, &allowed);
    assert_eq!(chromium["path"], "/opt/dm/dm-desktop");
    assert_eq!(chromium["type"], "stdio");
    assert!(chromium.get("allowed_extensions").is_none());
    let origins = chromium["allowed_origins"].as_array().unwrap();
    assert_eq!(origins.len(), allowed.chromium.len());
    assert!(origins
        .iter()
        .all(|o| o.as_str().unwrap().starts_with("chrome-extension://")));
    let firefox = register::manifest(Family::Firefox, exe, &allowed);
    assert!(firefox.get("allowed_origins").is_none());
    assert!(firefox["allowed_extensions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id == "{0d7a1a4b-3c2d-4e5f-8a9b-0c1d2e3f4a5b}"));
}

/// A translocated copy is refused before anything is written, the place a
/// browser would look included.
#[cfg(target_os = "macos")]
#[test]
fn registering_a_copy_macos_made_for_one_launch_is_refused_before_anything_is_written() {
    let home = tempfile::tempdir().unwrap();
    let manifest = firefox_has_run(home.path());
    let away = tempfile::tempdir().unwrap();
    let exe = away.path().join("AppTranslocation/1F2E/d/dm-desktop");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"").unwrap();
    let refused = register::register(
        home.path(),
        &exe,
        &Allowed::with(&[]).unwrap(),
        &Programs::default(),
    )
    .unwrap_err();
    assert!(refused.contains("AppTranslocation"), "{refused}");
    assert!(!manifest.exists());
}

/// `reg.exe` stands in for itself here: a script of that name on `PATH`,
/// which says what it was asked and fails or not as the test wants.
#[cfg(unix)]
#[test]
fn reg_exe_is_run_with_the_arguments_and_its_complaint_is_the_error() {
    use std::os::unix::fs::PermissionsExt;
    let _env = env_lock();
    let _restore = Restore::of(&["PATH"]);
    let bin = tempfile::tempdir().unwrap();
    let asked = bin.path().join("asked");
    let reg = bin.path().join("reg");
    std::fs::write(
        &reg,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nif [ \"$1\" = add ]; then exit 0; fi\necho \"  ERROR: The system was unable to find the key.  \" >&2\nexit 1\n",
            asked.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&reg, std::fs::Permissions::from_mode(0o755)).unwrap();
    set("PATH", Some(bin.path()));

    let add: Vec<String> = ["add", r"HKCU\Software\dm", "/f"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(register::run_reg(&add), Ok(()));
    let query: Vec<String> = ["query", r"HKCU\Software\dm"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        register::run_reg(&query),
        Err("reg query: ERROR: The system was unable to find the key.".into())
    );
    assert_eq!(
        std::fs::read_to_string(&asked).unwrap(),
        "add HKCU\\Software\\dm /f\nquery HKCU\\Software\\dm\n"
    );

    // With no `reg` to run at all, as on every system but Windows.
    let empty = tempfile::tempdir().unwrap();
    set("PATH", Some(empty.path()));
    let missing = register::run_reg(&query).unwrap_err();
    assert!(missing.starts_with("reg.exe could not be run"), "{missing}");
}

#[cfg(not(windows))]
#[test]
fn register_and_unregister_on_the_command_line_write_and_remove_the_manifests() {
    let _env = env_lock();
    let _restore = Restore::of(&["HOME", "DM_ENGINE"]);
    let home = tempfile::tempdir().unwrap();
    redirect_home(home.path());
    let manifest = firefox_has_run(home.path());
    let elsewhere = tempfile::tempdir().unwrap();
    let exe = program(elsewhere.path());

    // Neither flag: not this module's to answer.
    assert_eq!(register::from_args(&["--hidden".into()]), None);
    assert_eq!(register::from_args(&[]), None);

    // An id that is no extension's is refused before anything is written.
    set("DM_ENGINE", Some(&exe));
    assert_eq!(
        register::from_args(&["--register".into(), "not-an-id".into()]),
        Some(2)
    );
    assert!(!manifest.exists());

    // A program that is not there cannot be registered.
    set("DM_ENGINE", Some(&elsewhere.path().join("gone")));
    assert_eq!(register::from_args(&["--register".into()]), Some(1));
    assert!(!manifest.exists());

    // The program DM_ENGINE names, for the browser that has run.
    set("DM_ENGINE", Some(&exe));
    assert_eq!(register::from_args(&["--register".into()]), Some(0));
    assert_eq!(read(&manifest)["path"], exe.display().to_string());

    // Without DM_ENGINE, the program running: this test's.
    set("DM_ENGINE", None);
    assert_eq!(register::from_args(&["--register".into()]), Some(0));
    let running = dm_core::plain_path(std::env::current_exe().unwrap().canonicalize().unwrap());
    assert_eq!(read(&manifest)["path"], running.display().to_string());

    // Removed, then nothing left to remove.
    assert_eq!(register::from_args(&["--unregister".into()]), Some(0));
    assert!(!manifest.exists());
    assert_eq!(register::from_args(&["--unregister".into()]), Some(0));
}

#[cfg(not(windows))]
#[test]
fn the_check_at_launch_registers_the_appimage_file_once() {
    let _env = env_lock();
    let _restore = Restore::of(&["HOME", "APPIMAGE"]);
    let home = tempfile::tempdir().unwrap();
    redirect_home(home.path());
    let manifest = firefox_has_run(home.path());
    let elsewhere = tempfile::tempdir().unwrap();
    let image = program(elsewhere.path());

    // In an AppImage the file itself is what the browser must start.
    set("APPIMAGE", Some(&image));
    let written = register::ensure_this_copy().unwrap().expect("registered");
    assert!(written.contains(&manifest), "{written:?}");
    assert_eq!(read(&manifest)["path"], image.display().to_string());
    // Already registered: nothing to do the next time.
    assert_eq!(register::ensure_this_copy(), Ok(None));

    // Outside one, the program running, which is this test's.
    set("APPIMAGE", None);
    let written = register::ensure_this_copy().unwrap().expect("moved");
    assert!(written.contains(&manifest));
    let running = dm_core::plain_path(std::env::current_exe().unwrap().canonicalize().unwrap());
    assert_eq!(read(&manifest)["path"], running.display().to_string());
}

#[cfg(target_os = "macos")]
#[test]
fn the_check_at_launch_refuses_a_copy_run_from_where_macos_put_it_for_once() {
    let _env = env_lock();
    let _restore = Restore::of(&["HOME", "APPIMAGE"]);
    let home = tempfile::tempdir().unwrap();
    redirect_home(home.path());
    let manifest = firefox_has_run(home.path());
    let away = tempfile::tempdir().unwrap();
    let exe = away.path().join("AppTranslocation/1F2E/d/dm-desktop");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"").unwrap();
    set("APPIMAGE", Some(&exe));
    let refused = register::ensure_this_copy().unwrap_err();
    assert!(refused.contains("Applications"), "{refused}");
    assert!(!manifest.exists());
}
