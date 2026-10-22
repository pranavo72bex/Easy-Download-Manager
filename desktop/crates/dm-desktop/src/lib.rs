//! dm's window, in Rust.
//!
//! `commands` is every action by name; `app` holds the window's state and
//! draws the frame, `table` the download list and `dialogs` the window's
//! dialogs; `backend` is what
//! the window asks of the queue, `format` how it says numbers, `icons` the
//! toolbar's drawings and `update` the version check. The binary in
//! `main.rs` adds what only a real desktop has: the tray, the login item,
//! notifications.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
pub mod accent;
pub mod app;
pub mod autostart;
pub mod backend;
pub mod bidi;
pub mod commands;
pub mod control;
pub mod dialogs;
pub mod drops;
pub mod format;
pub mod i18n;
pub mod icons;
pub mod mcp;
pub mod motion;
pub mod power;
pub mod region;
pub mod system_theme;
pub mod table;
pub mod theme;
pub mod update;
#[cfg(target_os = "linux")]
pub mod xdnd;
