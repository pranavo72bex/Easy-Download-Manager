// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's program on a desktop: GPUI's application on the computer's own
//! platform, handed to dm's window (`dm_gpui::run_app`), which does the rest.

// A release opens no console window beside dm's own on Windows; the
// command-line modes attach to the terminal they were started from
// (`dm_desktop_base::modes`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "android"))]
fn main() {
    // The start's stages, where DM_FINISH_TRACE names a file (gaps row S16).
    dm_core::finish_trace::mark("start: main");
    // A portable copy's home, before any thread starts.
    dm_shell::settings::enter_portable();

    let args = dm_shell::shell::args();
    dm_gpui::run_app(&args, gpui_platform::application);
}

/// On a phone dm starts from the app (`dm-android`'s `android_main`), never
/// as a program of its own.
#[cfg(target_os = "android")]
fn main() {
    eprintln!("dm: on a phone dm starts from its app");
    std::process::exit(1);
}
