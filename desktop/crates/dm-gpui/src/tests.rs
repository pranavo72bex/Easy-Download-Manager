// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

/// The release tools read dm's version from this crate's manifest (the
/// Makefile's `APP_VERSION`, the workflows), and so do the packages and
/// About; `--version`, the native host's hello, the support block and the
/// User-Agent are the other crates': one number, or a release names
/// itself twice.
#[test]
fn this_window_carries_the_version_dm_is_released_under() {
    for manifest in [
        include_str!("../../dm-desktop-base/Cargo.toml"),
        include_str!("../../dm-shell/Cargo.toml"),
        include_str!("../../dm-core/Cargo.toml"),
    ] {
        let released = manifest
            .lines()
            .find_map(|line| line.strip_prefix("version = \""))
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or_default();
        assert_eq!(env!("CARGO_PKG_VERSION"), released);
    }
}

/// The language chosen is in force before the window and the tray are
/// made: set only by Options' OK, every word would be English after each
/// start (review).
#[test]
fn the_language_chosen_is_in_force_before_the_window_opens() {
    let source = include_str!("lib.rs");
    let main = source.split("#[cfg(test)]").next().unwrap_or_default();
    let set = main
        .find("i18n::set(i18n::chosen(")
        .expect("main puts the chosen language in force");
    let run = main
        .find("application.run(")
        .expect("main runs the application");
    assert!(set < run, "the language is set after the window is made");
}

/// The login item is put in step once the window is made, not before it:
/// asked of the system first, it held back the first frame of every
/// launch (gaps row S16).
#[test]
fn the_login_item_follows_the_setting_after_the_window_is_made() {
    let source = include_str!("lib.rs");
    let main = source.split("#[cfg(test)]").next().unwrap_or_default();
    let opened = main
        .find("open_main_window(")
        .expect("main opens the window");
    let follows = main
        .rfind("item.follow(&backend, wake)")
        .expect("main puts the login item in step");
    assert!(
        opened < follows,
        "the login item is asked before the window"
    );
    assert!(
        !main.contains("set_login_item"),
        "main asks the system itself"
    );
}

/// The windows that save a program dm runs (the command after each
/// download, the scheduled hours' program, the proxy script) save
/// through `Downloads::apply_settings`, which keeps those programs when
/// a script was driving the window a moment ago. A direct
/// `backend.set_settings` there would let a control-port client set the
/// Scheduler's program (security review).
#[test]
fn windows_that_set_a_program_save_through_the_guard() {
    // Split so this test's own source does not match it.
    let direct = concat!(".set_", "settings(");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut read = 0;
    // Every folder of it: the windows are in `dialogs/`, `options/` and `window/` (A13).
    let mut files = Vec::new();
    let mut folders = vec![src.clone()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            // Tests, in files of their own: `tests.rs`, `*_tests.rs`, the window's tests and
            // their harness.
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name == "tests.rs"
                || name.ends_with("_tests.rs")
                || ["window_tests", "harness", "harness.rs", "i18n_check.rs"].contains(&name)
            {
                continue;
            }
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    for path in files {
        let whole = std::fs::read_to_string(&path).unwrap();
        // The window's own code, not its tests (this one names both).
        let source = whole.split("#[cfg(test)]").next().unwrap_or_default();
        read += 1;
        let sets_a_program = [
            ".run_when_done",
            ".on_complete",
            ".proxy.script",
            ".queue_schedules",
        ]
        .iter()
        .any(|field| source.contains(field));
        assert!(
            !(sets_a_program && source.contains(direct)),
            "{} saves around the guard on programs dm runs",
            path.display()
        );
    }
    assert!(
        read > 50,
        "the window's sources were not found in {}",
        src.display()
    );
}
