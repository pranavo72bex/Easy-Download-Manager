// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's own media code, taken out of the engine (gaps row A6).
//!
//! Files in, files out: the MP4, Matroska and transport-stream writers that join a video's
//! picture and sound, the MP3 and GIF encoders, the AAC and AV1 readers and the subtitles. It
//! opens no connection, runs no async runtime and uses none of dm's other crates; dm-core calls
//! it, off its runtime, and keeps the same paths (`dm_core::mp4mux::join` and the rest).
//! What these modules need from the engine is handed in: the MP3 and the GIF come back as
//! bytes, which dm-core writes.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod aac;

mod error;
pub use error::Error;

pub mod fmp4;

pub mod frames;

pub mod gif;

#[cfg(windows)]
pub(crate) mod mf;

pub mod mkvmux;

pub mod mp3;

pub mod mp4mux;

pub mod shape;

pub mod sealed;

pub mod subtitles;

pub mod tsmux;

/// Where the media fixtures dm-core's tests share are (`desktop/crates/dm-core/tests/fixtures`):
/// kept there once, for both crates' tests.
#[cfg(test)]
pub(crate) fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dm-core/tests/fixtures")
}
