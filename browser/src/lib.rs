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
//! types and does what they say. The listeners themselves are registered by
//! `extension/background.js`, a loader of a few lines, because a Manifest V3
//! background must register them before any WebAssembly can have loaded.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
pub mod decide;
pub mod panel;

#[cfg(target_arch = "wasm32")]
mod content;
#[cfg(target_arch = "wasm32")]
mod popup;
#[cfg(target_arch = "wasm32")]
mod web;
