// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::ffi::OsString;
use std::path::PathBuf;

use super::*;

/// An environment holding `vars` alone.
fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name| {
        vars.iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| OsString::from(v))
    }
}

#[test]
fn linux_settings_are_in_xdg_config_home_else_under_home() {
    let linux = env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "/cfg")]);
    assert_eq!(xdg_config_in(linux), Some(PathBuf::from("/cfg")));
    // Empty is unset, as the specification says.
    let empty = env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "")]);
    assert_eq!(xdg_config_in(empty), Some(PathBuf::from("/home/a/.config")));
    assert_eq!(xdg_config_in(env(&[])), None);
}

#[test]
fn linux_data_is_the_persons_first_then_the_systems() {
    let set = env(&[
        ("HOME", "/home/a"),
        ("XDG_DATA_HOME", "/data"),
        ("XDG_DATA_DIRS", "/one:/two"),
    ]);
    assert_eq!(
        xdg_data_in(set),
        [PathBuf::from("/data"), "/one".into(), "/two".into()]
    );
    let unset = env(&[("HOME", "/home/a")]);
    assert_eq!(
        xdg_data_in(unset),
        [
            PathBuf::from("/home/a/.local/share"),
            "/usr/local/share".into(),
            "/usr/share".into()
        ]
    );
}

#[test]
fn macos_launch_agents_and_application_support_are_under_home() {
    let mac = env(&[("HOME", "/Users/a")]);
    assert_eq!(
        launch_agents_in(mac),
        Some(PathBuf::from("/Users/a/Library/LaunchAgents"))
    );
    assert_eq!(launch_agents_in(env(&[])), None);
    assert_eq!(
        application_support(std::path::Path::new("/Users/a")),
        PathBuf::from("/Users/a/Library/Application Support")
    );
}

#[test]
fn windows_programs_are_the_program_folders_that_are_set() {
    let windows = env(&[
        ("ProgramFiles", r"C:\Program Files"),
        ("LOCALAPPDATA", r"C:\Users\a\AppData\Local"),
    ]);
    assert_eq!(
        windows_programs_in(windows),
        [
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\Users\a\AppData\Local")
        ]
    );
}

#[test]
fn the_home_variable_is_home_then_windows_profile() {
    let windows = env(&[("USERPROFILE", r"C:\Users\a")]);
    assert_eq!(home_variable_in(&windows), None);
    assert_eq!(home_or_profile_in(&windows), Some(PathBuf::from(r"C:\Users\a")));
    let both = env(&[("HOME", "/home/a"), ("USERPROFILE", r"C:\Users\a")]);
    assert_eq!(home_or_profile_in(both), Some(PathBuf::from("/home/a")));
}
