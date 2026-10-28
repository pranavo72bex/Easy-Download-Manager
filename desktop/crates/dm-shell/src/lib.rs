//! The part of the desktop app that is not the window.
//!
//! Everything a download manager is judged on lives in dm-core, which has no
//! idea a screen exists. This crate is what sits between it and whatever
//! draws the window: the settings file, the browser's native host, the lock
//! that keeps dm to one copy, startup, and the inbox a browser hands
//! downloads through. It knows nothing about any window toolkit, so the
//! window can be replaced without any of this being written twice.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
// From Google's Rust style guide, checked in CI: an `unsafe` block says why it
// is sound (`// SAFETY:`; not on Windows, whose blocks are not all written up
// yet), and a string is built with `format!` rather than `+`, and a loop is a
// loop rather than `for_each`.
#![cfg_attr(not(windows), warn(clippy::undocumented_unsafe_blocks))]
#![warn(clippy::string_add, clippy::needless_for_each)]
pub mod host;
pub mod instance;
pub mod lists;
pub mod register;
pub mod settings;
pub mod shell;
/// The tests' logins and secrets, never written in the code.
#[cfg(test)]
#[path = "../../../test-support/secrets.rs"]
pub(crate) mod test_secrets;

pub use settings::{home, Settings};
