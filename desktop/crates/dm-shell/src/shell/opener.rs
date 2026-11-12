// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Opening a finished file and showing it in its folder, each system its own way: one file for
//! each, the same three calls in each.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(super) use macos::{open, reveal, short_form};
#[cfg(all(unix, not(target_os = "macos")))]
mod unix;
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) use unix::linux_open_in;
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) use unix::{open, reveal, short_form};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{open, reveal, short_form};
