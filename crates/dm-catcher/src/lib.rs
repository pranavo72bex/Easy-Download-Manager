// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's browser extension: it catches downloads and hands them to the
//! application, and downloads nothing itself.
//!
//! - A download the browser starts that matches the app's catch rules is
//!   paused, handed to the app over native messaging, and — once the app has
//!   taken it — cancelled and erased from the browser's list. If the app is
//!   not installed, does not answer, or refuses, the download is left to the
//!   browser: a download manager must never lose one.
//! - "Download with dm" on a link and on a video or audio element, and
//!   "Download all/selected links"; the "Download video" panel over
//!   players, which also offers the streams a page's player loaded.
//! - The toolbar popup: whether the app answers, switches for catching and
//!   for the panel on this site, and a button that brings the app up.
//!
//! [`decide`] is every decision, in plain Rust tested on the host. `web` is
//! the browser side, compiled only for wasm32: it reads events into those
//! types and does what they say. It registers the background's listeners
//! itself (`web/listen.rs`): a Manifest V3 background must have them on its
//! first turn, so `scripts/package.sh` writes the WebAssembly into the
//! background's script, which starts it at once (`initSync`).

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
#![warn(missing_docs)]
pub mod decide;
pub mod panel;
pub mod words;

#[cfg(target_arch = "wasm32")]
mod content;
#[cfg(target_arch = "wasm32")]
mod popup;
#[cfg(target_arch = "wasm32")]
mod web;
