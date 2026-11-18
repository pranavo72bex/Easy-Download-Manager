// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A video's sound saved as MP3: dm's own encoder in `dm_media::mp3` (gaps row A6); here what
//! the engine adds, the list line, the file written and the work off the async runtime.

use std::path::{Path, PathBuf};

pub use dm_media::mp3::*;

/// The list line for the sound as MP3, offered after an M4A's: its size
/// about half as much again (192 kbit/s against AAC's usual 128).
pub fn choice_after(m4a_size: Option<u64>) -> Option<crate::videopage::Choice> {
    possible().then(|| crate::videopage::Choice {
        label: "MP3 audio".into(),
        quality: "audio/mp3".into(),
        size: m4a_size.map(|s| s * 3 / 2),
    })
}

/// Saves the sound of `source` (an M4A, an MP4's sound, or a WebM's Opus where [`takes`]) as an
/// MP3 at `target`, at `kbps` kbit/s (`None`: 192 for stereo, as always; [`convert_bytes`]).
/// Returns the MP3's length.
pub fn convert(source: &Path, target: &Path, kbps: Option<u32>) -> Result<u64, String> {
    let bytes = convert_bytes(source, kbps)?;
    crate::write_atomic(target, &bytes)
        .map_err(|e| format!("the MP3 could not be written: {e}"))?;
    Ok(bytes.len() as u64)
}

/// [`convert`], off the async runtime, with a turn at the computer's heavy work (`heavy`, gaps
/// row 185).
pub async fn convert_async(
    source: PathBuf,
    target: PathBuf,
    kbps: Option<u32>,
) -> Result<u64, String> {
    crate::heavy::run(move || convert(&source, &target, kbps))
        .await
        .map_err(|e| format!("making the MP3 stopped: {e}"))?
}
