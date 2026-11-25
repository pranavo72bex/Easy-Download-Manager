// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's windows and dialogs say and decide, with no window toolkit, so
//! every window says it in the same words: `i18n` (the words in the language
//! picked, and the catalogs), `format` (how a row's numbers are said), `rows`
//! and `columns` (the download list's rows and columns), `selection` and
//! `find` (stepping and finding through it), `asks` (dm's questions), the
//! dialogs' models (`add_url`, `file_info`, `row_edits`, `options`,
//! `scheduler`, `grabber`, `links_import`, `save_to`, `progress`, `statistics`,
//! `zip_list`), `held` (what holds a download back), `details` (the text
//! Copy details puts on the clipboard), `project` (About's and Help's
//! pages), `toolbar` and `tray` (their words and styles). How each is drawn,
//! and where, is the window's. Each is reached from the crate's root as
//! before (`dm_desktop_base::rows`).

pub mod add_url;
pub mod asks;
pub mod columns;
pub mod details;
pub mod file_info;
pub mod find;
pub mod format;
pub mod grabber;
pub mod held;
pub mod help;
pub mod i18n;
pub mod links_import;
pub mod options;
/// A finished program: which files run, and what Open asks first (gaps row H1).
pub mod programs;
pub mod progress;
pub mod project;
pub mod row_edits;
pub mod rows;
pub mod save_to;
pub mod scheduler;
pub mod selection;
pub mod statistics;
pub mod toolbar;
pub mod tray;
pub mod zip_list;
