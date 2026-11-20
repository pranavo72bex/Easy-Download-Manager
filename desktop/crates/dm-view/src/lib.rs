// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's windows and dialogs say and decide, with no window toolkit and
//! none of the desktop's system services, so the desktop's window and the
//! phone's say it in the same words (gaps row N15): `view` (the words, a
//! row's numbers, the list's rows and columns, the dialogs' models) and
//! `region` (how the computer writes numbers and dates). Every module is
//! reached from the crate's root, as `dm_view::rows`; dm-desktop-base
//! re-exports each under its old name (`dm_desktop_base::rows`), and
//! [`Error`], the one error type both crates say what went wrong in.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

mod error;
pub mod region;
mod view;

pub use error::Error;

pub use view::{
    add_url, asks, columns, details, file_info, find, format, grabber, held, i18n, links_import,
    options, progress, project, row_edits, rows, save_to, scheduler, selection, statistics,
    toolbar, tray, zip_list,
};
