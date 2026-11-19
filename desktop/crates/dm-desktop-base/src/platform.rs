// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The system's services, asked of the computer dm runs on: `desktop`, the
//! application's `Platform` made of them; `quiet` (whether the person is
//! busy), `awake` (no sleep while a download runs), `wake` (the schedule's
//! wake timer), `power` (When done, and the sounds), the login item
//! (`autostart` on Linux, `login` on macOS and Windows), `toast` (Windows'
//! notifications), `signals` (a logout or `kill` quits dm as Exit does),
//! `system_theme`, `contrast`, `accent` and `mica` (dark or light, High
//! Contrast's colours, the accent colour and Windows 11's backdrop),
//! `region` (how the computer writes numbers and dates), Windows' taskbar
//! button (`taskbar`'s progress, `jumplist`), `drops` (what the system's
//! drop target takes), `opened` (links and files macOS opens with dm) and
//! `a11y_tree` (what VoiceOver is given of each window). Each is reached
//! from the crate's root as before (`dm_desktop_base::quiet`).

pub mod a11y_tree;
pub mod accent;
pub mod autostart;
pub mod awake;
pub mod contrast;
pub mod desktop;
pub mod drops;
pub mod jumplist;
pub mod login;
pub mod mica;
#[cfg(target_os = "macos")]
pub mod opened;
pub mod power;
pub mod quiet;
pub mod region;
pub mod signals;
pub mod system_theme;
pub mod taskbar;
#[cfg(windows)]
pub mod toast;
pub mod wake;
