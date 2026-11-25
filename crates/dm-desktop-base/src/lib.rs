// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's desktop program does that is not drawing, with no window
//! toolkit, so that any window can use it: `boot`, dm started as every
//! window starts it, and the rest below.
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
//!
//! In three folders, each saying what is in it: `platform` (the system's
//! services), `view` (what the windows and dialogs say and decide) and `run`
//! (the ways dm runs). Every module is reached from the crate's root, as
//! `dm_desktop_base::rows`, wherever its folder is. What goes wrong is said
//! in the crate's one error type, [`Error`] (gaps row A9).

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

mod error;
pub mod platform;
mod run;
mod view;

pub use error::Error;

// One system's alone (`platform::opened` on macOS, `platform::toast` on Windows) are reached
// through `platform` (gaps row A10).
pub use platform::{
    a11y_tree, accent, autostart, awake, contrast, desktop, drops, jumplist, login, metered, mica,
    motion, phone, power, quiet, region, signals, system_theme, taskbar, wake,
};
pub use run::{boot, cli, control, headless, mcp, modes, probe, session, wakes};
pub use view::{
    add_url, asks, columns, details, file_info, find, format, grabber, held, help, i18n,
    links_import, options, programs, progress, project, row_edits, rows, save_to, scheduler,
    selection, toolbar, tray, zip_list,
};
// View › Statistics and a progress window's speed line (gaps row W15).
pub use view::statistics;
