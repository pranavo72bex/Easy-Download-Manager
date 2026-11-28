// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM's network, taken out of the engine (gaps row A6).
//!
//! How EDM reaches a server, whatever it then fetches: the HTTP client and its TLS, proxies and
//! the proxy configuration script (run by edm-js), names and where a redirect may go, sign-ins
//! (Basic, Digest, NTLM) and the browser's cookies, the shared speed ceiling, a body's
//! decoding, the second network path and the one network downloads may be held to, the stop
//! flag a row's work checks, the clock and calendar (`clock`), and EDM's [`Error`].
//! edm-core builds downloads on it and keeps its paths (`edm_core::Error`, `edm_core::client`,
//! `edm_core::pac` and the rest).

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod agents;

pub mod auth;

pub mod body;

pub mod clock;

pub mod connect_first;

pub mod control;

pub mod cookies;

pub mod decode;

pub mod dns;

pub mod error;

pub mod hex;

pub mod http_client;

pub mod javascript;

pub mod limit;

pub mod log;

pub mod ntlm;

pub mod only_through;

pub mod pac;

mod platform;

pub mod processes;

pub mod proxy;

pub mod redirects;

pub mod second_path;

pub mod trust;

pub mod wipe;

/// The tests' logins and secrets, never written in the code.
#[cfg(test)]
pub(crate) use edm_test_support::secrets as test_secrets;

pub use agents::*;
pub use control::{Control, Wanted};
pub use error::*;
pub use http_client::*;
pub use processes::*;

/// A lock that is taken even if a thread that held it panicked.
///
/// The data under EDM's plain mutexes — the problem list, the schedule, the
/// window's settings — stays meaningful after a panic elsewhere, and a
/// second panic over a poisoned lock would turn one failure into a crash.
pub fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
