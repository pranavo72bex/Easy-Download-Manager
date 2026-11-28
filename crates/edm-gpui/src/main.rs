// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM's program on a desktop: GPUI's application on the computer's own
//! platform, handed to EDM's window (`edm_gpui::run_app`), which does the rest.

// A release opens no console window beside EDM's own on Windows; the
// command-line modes attach to the terminal they were started from
// (`edm_desktop_base::modes`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "android"))]
fn main() {
    // The start's stages, where EDM_FINISH_TRACE names a file (gaps row S16).
    edm_core::finish_trace::mark("start: main");
    // A portable copy's home, before any thread starts.
    edm_shell::settings::enter_portable();

    let args = edm_shell::shell::args();
    edm_gpui::run_app(&args, gpui_platform::application);
}

/// On a phone EDM starts from the app (`edm-android`'s `android_main`), never
/// as a program of its own.
#[cfg(target_os = "android")]
fn main() {
    eprintln!("EDM: on a phone EDM starts from its app");
    std::process::exit(1);
}
