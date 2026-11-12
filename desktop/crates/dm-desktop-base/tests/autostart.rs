// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The login entry (`autostart`), with nothing of the person's touched:
//! every folder the entry is looked for in is pointed at a temporary one
//! first, under a lock, and put back after. Moved from the egui window's
//! tests.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dm_desktop_base::autostart::{self, Removal};

/// The environment is the process's: tests that set it take turns.
static ENV: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The variables these tests change, put back as they were when a test
/// ends, however it ends.
const NAMES: [&str; 5] = [
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_DATA_DIRS",
    "APPIMAGE",
];

struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl Restore {
    fn all() -> Self {
        Restore(NAMES.iter().map(|n| (*n, std::env::var_os(n))).collect())
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        for (name, value) in &self.0 {
            // SAFETY: under `ENV`, and dropped before the lock is.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

fn set(name: &str, value: Option<&std::ffi::OsStr>) {
    // SAFETY: under `ENV`, so nothing else in this binary reads the
    // environment while it is set.
    unsafe {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}

/// Every folder the entry or its icon is looked for in, inside `dir`; and
/// a check that it took before anything is written.
fn redirect(dir: &Path) {
    set("HOME", Some(dir.join("home").as_os_str()));
    set("XDG_CONFIG_HOME", Some(dir.join("config").as_os_str()));
    set("XDG_DATA_HOME", Some(dir.join("data").as_os_str()));
    set("XDG_DATA_DIRS", Some(dir.join("system").as_os_str()));
    set("APPIMAGE", None);
    assert_eq!(
        autostart::entry_path(),
        Some(dir.join("config/autostart/dm.desktop")),
        "the entry is not under the test's folder; stopping before anything is written"
    );
}

#[test]
fn the_entry_goes_under_the_config_folder_else_the_homes() {
    let _env = env_lock();
    let _back = Restore::all();
    let dir = tempfile::tempdir().unwrap();
    redirect(dir.path());
    // An empty XDG_CONFIG_HOME is none: the home's .config.
    set("XDG_CONFIG_HOME", Some("".as_ref()));
    assert_eq!(
        autostart::entry_path(),
        Some(dir.path().join("home/.config/autostart/dm.desktop"))
    );
    // Neither: nowhere to put one.
    set("XDG_CONFIG_HOME", None);
    set("HOME", None);
    assert_eq!(autostart::entry_path(), None);
    assert_eq!(
        autostart::set(true),
        Err("no home directory to put a login entry in".into())
    );
}

#[test]
fn the_program_started_at_login_is_the_appimage_when_run_from_one() {
    let _env = env_lock();
    let _back = Restore::all();
    let dir = tempfile::tempdir().unwrap();
    redirect(dir.path());
    assert_eq!(
        autostart::program().unwrap(),
        std::env::current_exe().unwrap()
    );
    set("APPIMAGE", Some("".as_ref()));
    assert_eq!(
        autostart::program().unwrap(),
        std::env::current_exe().unwrap(),
        "an empty APPIMAGE is none"
    );
    set("APPIMAGE", Some("/home/a/Apps/dm.AppImage".as_ref()));
    assert_eq!(
        autostart::program().unwrap(),
        PathBuf::from("/home/a/Apps/dm.AppImage")
    );
}

#[test]
fn turning_start_on_login_on_and_off_writes_and_removes_the_entry() {
    let _env = env_lock();
    let _back = Restore::all();
    let dir = tempfile::tempdir().unwrap();
    redirect(dir.path());
    let image = dir.path().join("Apps/dm.AppImage");
    set("APPIMAGE", Some(image.as_os_str()));
    // The .deb's icon in the data home: the entry names it.
    let apps = dir.path().join("data/icons/hicolor/64x64/apps");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("dm-desktop.png"), b"png").unwrap();

    autostart::set(true).unwrap();
    let path = autostart::entry_path().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\nIcon=dm-desktop\n"), "{text}");
    assert_eq!(autostart::entry_program(&text), Some(image.clone()));
    // Again: nothing to change.
    autostart::set(true).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

    autostart::set(false).unwrap();
    assert!(!path.exists());
    // Off when it is already off is no failure.
    autostart::set(false).unwrap();
}

#[test]
fn the_icon_is_found_in_the_homes_data_folder_or_the_systems_pixmaps() {
    let _env = env_lock();
    let _back = Restore::all();
    let dir = tempfile::tempdir().unwrap();
    redirect(dir.path());
    let entry = || autostart::entry(Path::new("/usr/bin/dm"));
    // Nothing installed anywhere (an AppImage): `dm`.
    assert!(entry().contains("\nIcon=dm\n"));

    // The system's pixmaps, with no data home of its own: through HOME.
    set("XDG_DATA_HOME", None);
    let pixmaps = dir.path().join("system/pixmaps");
    std::fs::create_dir_all(&pixmaps).unwrap();
    std::fs::write(pixmaps.join("dm-desktop.png"), b"png").unwrap();
    assert!(entry().contains("\nIcon=dm-desktop\n"));
    // `dm` installed in the home's own data folder wins.
    let apps = dir
        .path()
        .join("home/.local/share/icons/hicolor/48x48/apps");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("dm.png"), b"png").unwrap();
    assert!(entry().contains("\nIcon=dm\n"));

    // An empty XDG_DATA_DIRS is the spec's own default, not nothing.
    set("XDG_DATA_DIRS", Some("".as_ref()));
    std::fs::remove_file(apps.join("dm.png")).unwrap();
    assert!(entry().contains("\nIcon=dm\n"));
}

#[test]
fn an_entry_that_cannot_be_written_or_removed_says_where() {
    // The entry's Icon line follows HOME and XDG_DATA_HOME, which other tests here move.
    let _env = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let program = Path::new("/usr/bin/dm");
    // A folder where the entry should be cannot be removed as one.
    let folder = dir.path().join("dm.desktop");
    std::fs::create_dir_all(&folder).unwrap();
    let removed = autostart::set_at(&folder, false, program, true, |_| true).unwrap_err();
    assert!(
        removed.starts_with(&folder.display().to_string()),
        "{removed}"
    );
    // Nor read and written over.
    assert!(autostart::set_at(&folder, true, program, true, |_| true).is_err());
    assert!(folder.is_dir(), "the folder was replaced");
    // A file where its folder should be.
    let blocked = dir.path().join("file");
    std::fs::write(&blocked, b"x").unwrap();
    assert!(autostart::set_at(&blocked.join("dm.desktop"), true, program, true, |_| true).is_err());
    // `--unregister` reading it says where too.
    let unread = autostart::remove_at(&folder, program, |_| true).unwrap_err();
    assert!(
        unread.starts_with(&folder.display().to_string()),
        "{unread}"
    );
}

#[test]
fn an_entry_of_this_modules_naming_no_program_is_replaced_only_by_a_copy_that_may_claim_it() {
    // The entry's Icon line follows HOME and XDG_DATA_HOME, which other tests here move.
    let _env = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dm.desktop");
    let program = Path::new("/usr/bin/dm");
    // This module's mark, but an Exec line that names nothing.
    let ours = autostart::entry(program);
    let broken = ours.replace("Exec=/usr/bin/dm --hidden", "Exec=");
    assert_ne!(broken, ours);
    std::fs::write(&path, &broken).unwrap();
    assert!(!autostart::set_at(&path, true, program, false, |_| true).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    assert!(autostart::set_at(&path, true, program, true, |_| true).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), ours);

    // One of this module's for this program, of an older text: rewritten.
    let older = ours.replace("StartupNotify=false\n", "");
    std::fs::write(&path, &older).unwrap();
    assert!(autostart::set_at(&path, true, program, false, |_| true).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), ours);
}

#[test]
fn an_entry_whose_exec_line_does_not_read_names_no_program() {
    // The entry's Icon line follows HOME and XDG_DATA_HOME, which other tests here move.
    let _env = env_lock();
    let read = |exec: &str| autostart::entry_program(&format!("[Desktop Entry]\nExec={exec}\n"));
    assert_eq!(read(""), None);
    assert_eq!(read("\\"), None, "an escape with nothing after it");
    assert_eq!(read("\"/opt/no end"), None, "a quote never closed");
    assert_eq!(
        read("\"/opt/a\\"),
        None,
        "an escape inside quotes at the end"
    );
    assert_eq!(read("\"\" --hidden"), None, "an empty program");
    assert_eq!(
        read("/opt/100%%/dm --hidden").as_deref(),
        Some(Path::new("/opt/100%/dm"))
    );
    assert_eq!(
        read("/opt/a%b/dm").as_deref(),
        Some(Path::new("/opt/a%b/dm")),
        "a lone % is kept"
    );
    // A program with no name at all is still written as one argument.
    assert!(autostart::entry(Path::new("")).contains("\nExec=\"\" --hidden\n"));
    // Every character the spec reserves is quoted.
    for c in [
        '\'', '>', '<', '~', '|', '&', ';', '*', '?', '#', '(', ')', '`',
    ] {
        let program = format!("/opt/a{c}b/dm");
        let entry = autostart::entry(Path::new(&program));
        assert!(entry.contains("\nExec=\""), "{c}: {entry}");
        assert_eq!(
            autostart::entry_program(&entry).as_deref(),
            Some(Path::new(&program)),
            "{c}"
        );
    }
}

#[test]
fn unregister_removes_an_entry_it_cannot_read_the_program_of() {
    // The entry's Icon line follows HOME and XDG_DATA_HOME, which other tests here move.
    let _env = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dm.desktop");
    std::fs::write(&path, "[Desktop Entry]\nName=dm\n").unwrap();
    assert_eq!(
        autostart::remove_at(&path, Path::new("/usr/bin/dm"), |_| true).unwrap(),
        Removal::Removed(path.display().to_string())
    );
    assert!(!path.exists());
    assert!(autostart::removes(None, Path::new("/usr/bin/dm"), |_| true));
    assert!(autostart::removes(
        Some(Path::new("/usr/bin/dm")),
        Path::new("/usr/bin/dm"),
        |_| true
    ));
    assert!(!autostart::removes(
        Some(Path::new("/opt/dm")),
        Path::new("/usr/bin/dm"),
        |_| true
    ));
}

#[test]
fn a_launch_agent_or_run_value_with_nothing_in_it_names_no_program() {
    // The entry's Icon line follows HOME and XDG_DATA_HOME, which other tests here move.
    let _env = env_lock();
    let none = [
        "<key>ProgramArguments</key>",
        "<key>ProgramArguments</key><array><string>",
        "<key>ProgramArguments</key><array><string></string></array>",
    ];
    for plist in none {
        assert_eq!(autostart::plist_program(plist), None, "{plist}");
    }
    assert_eq!(autostart::run_value_program("--hidden"), None);
    assert_eq!(autostart::run_value_program("\"\" --hidden"), None);
    assert_eq!(
        autostart::run_value_program("C:\\dm\\dm-desktop.exe").as_deref(),
        Some(Path::new("C:\\dm\\dm-desktop.exe"))
    );
    // A test's dm (a debug build, and DM_HOME set or not) never takes the
    // login item from an installed one.
    assert!(!autostart::may_claim());
    assert!(autostart::same_program(
        Path::new("/usr/bin/dm"),
        Path::new("/usr/bin/dm")
    ));
    assert_eq!(
        autostart::same_program(Path::new("/usr/bin/DM"), Path::new("/usr/bin/dm")),
        cfg!(windows)
    );
}

#[test]
fn a_copy_refused_at_login_is_told_where_to_move() {
    // The entry's Icon line follows HOME and XDG_DATA_HOME, which other tests here move.
    let _env = env_lock();
    let home = Path::new("/Users/a");
    let image = Path::new("/Volumes/dm/dm.app/Contents/MacOS/dm-desktop");
    let why = autostart::refused_at_login(image, home, true, |_| true).unwrap();
    assert!(why.starts_with(&image.display().to_string()));
    assert!(why.contains("Move dm to Applications"));
    let desktop = Path::new("/Users/a/Desktop/dm.app/Contents/MacOS/dm-desktop");
    let why = autostart::refused_at_login(desktop, home, true, |_| false).unwrap();
    assert!(why.contains("Downloads, Desktop or Documents"), "{why}");
}
