// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the window asks of the system beside drawing: its own window calls, the tray, notifications, the clipboard, drags and drops, and the desktop's services (gaps row A13).

pub(crate) mod clipwatch;
pub(crate) mod dragout;
pub(crate) mod drops;
pub(crate) mod droptarget;
#[cfg(not(windows))]
pub(crate) mod notify;
pub(crate) mod services;
#[cfg(target_os = "macos")]
pub(crate) mod sheets;
pub(crate) mod syswindow;
pub(crate) mod tray;
