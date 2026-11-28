// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A sound file read to 16-bit samples by the system's own decoder, for the
//! MP3 encoder: on macOS AudioToolbox's ExtAudioFile (`macos.rs`), on
//! Windows Media Foundation's source reader (`win.rs`), which both read AAC
//! in an M4A or MP4 (and turn another sample rate into one MP3 has);
//! Windows' also Opus in a WebM. Elsewhere (Linux) the built-in AAC
//! decoder, `crate::aac` (`linux.rs`). Opus in a WebM where the system has
//! no decoder for it (macOS, Linux, a Windows without Media Foundation's) is
//! read by EDM's own, `crate::opus` (gaps row Y25).

use std::path::Path;

use crate::Error;

// Each system's decoder, in a file of its own, the same three calls in each.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as system;
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as system;
#[cfg(not(any(target_os = "macos", windows)))]
mod linux;
#[cfg(not(any(target_os = "macos", windows)))]
use linux as system;

/// Interleaved 16-bit samples, their channels and their rate.
#[derive(Debug)]
pub struct Pcm {
    /// The samples, left and right interleaved.
    pub samples: Vec<i16>,
    /// How many channels the samples have.
    pub channels: usize,
    /// Samples per second, per channel.
    pub rate: u32,
}

/// `path`'s sound as samples, at most two channels, at a rate MP3 has.
pub fn read(path: &Path) -> Result<Pcm, Error> {
    if crate::mkvmux::is_webm(path) && !system::opus_possible() {
        return crate::opus::read(path);
    }
    system::read(path)
}

/// Whether the system's decoder here reads AAC (`super::possible`).
pub(crate) fn possible() -> bool {
    system::possible()
}

/// Whether the Opus of the WebM at `path` is read here: always by the
/// system's decoder where it has one; by EDM's own where every packet is one it
/// reads (no SILK yet).
pub(crate) fn opus_decodable(path: &Path) -> bool {
    system::opus_possible() || crate::opus::decodable(path)
}

/// Whether Opus in a WebM is read here (`super::opus_possible`): by the
/// system's decoder where it has one, else by EDM's own, which every system
/// has.
pub(crate) fn opus_possible() -> bool {
    true
}
