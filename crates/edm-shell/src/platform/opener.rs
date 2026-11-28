// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Opening a finished file and showing it in its folder, each system its own way: one file for
//! each, the same three calls in each.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::{open, reveal, short_form};
#[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
mod unix;
// The tests' alone: the unix opener calls it within its own file.
#[cfg(all(test, unix, not(any(target_os = "macos", target_os = "android"))))]
pub(crate) use unix::linux_open_in;
#[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
pub(crate) use unix::{open, reveal, short_form};
// The phone's app opens files, not its engine (gaps row N2).
#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub(crate) use android::{open, reveal, short_form};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{open, reveal, short_form};

/// Whether "Open with" hands the program a long path's short form, which every program reads,
/// where one written before long paths would not open it: Windows.
pub(crate) const SHORT_FORM_FOR_PROGRAMS: bool = cfg!(windows);

/// Whether "Open with" goes through `open -a`, which takes an application bundle: macOS.
pub(crate) const OPEN_WITH_THROUGH_OPEN: bool = cfg!(target_os = "macos");
