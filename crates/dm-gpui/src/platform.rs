// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the window asks of the system beside drawing: its own window calls, the tray, notifications, the clipboard, drags and drops, and the desktop's services (gaps row A13).
//! On a phone the notifier and the clipboard are the app's (`phone.rs`, gaps row N17), and there is no tray.

/// Whether dm runs on a phone: no browser extension reaches it there, and what only a
/// computer has (the browser's bar) is not offered.
pub(crate) const ON_A_PHONE: bool = cfg!(target_os = "android");

pub(crate) mod clipwatch;
pub(crate) mod dragout;
pub(crate) mod drops;
pub(crate) mod droptarget;
#[cfg(not(any(windows, target_os = "android")))]
pub(crate) mod notify;
// On a phone the notifier is the app's.
#[cfg(target_os = "android")]
pub(crate) use phone as notify;
#[cfg(target_os = "android")]
pub(crate) mod phone;
#[cfg(target_os = "macos")]
pub(crate) mod popmenu;
pub(crate) mod services;
#[cfg(target_os = "macos")]
pub(crate) mod sheets;
pub(crate) mod syswindow;
pub(crate) mod tray;
