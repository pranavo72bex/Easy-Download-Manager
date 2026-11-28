// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM on Android, all in Rust (gaps rows N1, N3): the desktop's own window, edm-gpui's
//! (`edm_gpui::run_app`), on GPUI's Android platform (gpui-android, gaps row N16). edm-gpui's
//! start brings up the engine and the application's rules as on a computer, so nothing a
//! download does is written here: a feature works on the phone as it does on the desktop, and a
//! fix to it is made once.
//!
//! What only the phone does for EDM's window (a notification, opening a file, the clipboard,
//! keeping awake) is handed to edm-desktop-base's seam (`edm_desktop_base::phone`) as
//! [`phone::Phone`], made of the calls into Android that `android/` makes through JNI; the link
//! a share hands over goes to the window as a browser's does. On a phone `android_main`
//! (`android.rs`) starts it all; on a computer the crate's tests check the services and what a
//! share hands over.

#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

#[cfg(target_os = "android")]
mod android;
pub mod phone;
#[cfg(any(target_os = "android", test))]
mod sealed;
#[cfg(any(target_os = "android", test))]
mod shared;
#[cfg(any(target_os = "android", test))]
mod storage;
pub mod wakeup;
