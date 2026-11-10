// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's window, in Rust.
//!
//! `commands` is every action by name; `app` holds the window's state and
//! draws the frame, `table` the download list and `dialogs` the window's
//! dialogs; `backend` re-exports the application's `Backend`
//! (`dm_shell::app::backend`), through which the window asks the queue, `format` how it says numbers, `icons` the
//! toolbar's drawings and `update` the version check. The binary in
//! `main.rs` adds what only a real desktop has: the tray, the login item,
//! notifications.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
// From Google's Rust style guide, checked in CI: an `unsafe` block says why it
// is sound (`// SAFETY:`, on every platform, Windows included), and a string is
// built with `format!` rather than `+`, and a loop is a loop rather than
// `for_each`.
#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::string_add, clippy::needless_for_each)]
pub mod a11y_tree;
pub mod accent;
pub mod app;
pub mod autostart;
pub mod awake;
pub mod backend;
pub mod bidi;
pub mod chrome;
pub mod cli;
pub mod commands;
pub mod contrast;
pub mod control;
pub mod details;
pub mod dialogs;
pub mod drops;
pub mod format;
pub mod i18n;
pub mod icons;
pub mod jumplist;
pub mod mcp;
pub mod motion;
pub mod power;
pub use dm_desktop_base::quiet;
pub mod region;
pub mod system_theme;
pub mod table;
pub mod taskbar;
pub mod theme;
#[cfg(windows)]
pub mod toast;
pub mod update;
pub mod wake;
#[cfg(target_os = "linux")]
pub mod xdnd;
