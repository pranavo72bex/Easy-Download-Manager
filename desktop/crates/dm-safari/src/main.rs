// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's Safari extension's program (`dm Safari Extension.appex`, inside dm.app): Safari sends
//! the extension's native messages to it, and it hands each to dm's own native host and gives
//! Safari the answer (`relay`, `bridge`). Safari is macOS's alone; elsewhere it only says so.

#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

#[cfg(target_os = "macos")]
mod bridge;
// Used by the bridge on macOS, and tested everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod relay;

fn main() {
    #[cfg(target_os = "macos")]
    std::process::exit(bridge::run());
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("dm-safari is the program of dm's Safari extension, on macOS alone");
        std::process::exit(1);
    }
}
