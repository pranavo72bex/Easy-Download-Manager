// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's desktop program does that is not drawing, with no window
//! toolkit, so that any window can use it: `boot`, dm started as every
//! window starts it, and the rest below. `dm_desktop` re-exports each
//! module at its old path.
//!
//! The system's services: `quiet` (whether the person is busy), `awake`
//! (no sleep while a download runs), `wake` (the schedule's wake timer),
//! `power` (When done, and the sounds), `autostart` (the Linux login item)
//! and `toast` (Windows' notifications), with `desktop`, the application's
//! `Platform` made of them (the window supplying what its crates do), and
//! `login` (the macOS and Windows login item), `a11y_tree` (what VoiceOver
//! is given of each window), `signals` (a logout or
//! `kill` quits dm as Exit does), `system_theme` and `contrast` (dark or
//! light, and Windows' High Contrast colours, where the window library cannot
//! say), and Windows' taskbar button (`taskbar`'s progress, `jumplist`); and
//! `control`, the control port's
//! server, which hands each request to the window through a callback, and
//! `probe`, the window methods it asks as a contract any window answers;
//! `mcp` (the MCP server an AI assistant talks to, over the control port),
//! `cli` (the command line, over the control port too) and `region` (how
//! the computer writes numbers and dates); `i18n` (the words in the
//! language picked, and the catalogs), `format` (how a row's numbers are
//! said) and `details` (the text Copy details puts on the clipboard).

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
// As in dm-desktop: an `unsafe` block says why it is sound, a string is built
// with `format!` rather than `+`, and a loop is a loop rather than `for_each`.
#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::string_add, clippy::needless_for_each)]
pub mod a11y_tree;
pub mod add_url;
pub mod asks;
pub mod autostart;
pub mod awake;
pub mod boot;
pub mod cli;
pub mod columns;
pub mod contrast;
pub mod control;
pub mod desktop;
pub mod details;
pub mod file_info;
pub mod format;
pub mod held;
pub mod i18n;
pub mod jumplist;
pub mod login;
pub mod mcp;
pub mod power;
pub mod probe;
pub mod quiet;
pub mod region;
pub mod rows;
pub mod signals;
pub mod system_theme;
pub mod taskbar;
#[cfg(windows)]
pub mod toast;
pub mod wake;
