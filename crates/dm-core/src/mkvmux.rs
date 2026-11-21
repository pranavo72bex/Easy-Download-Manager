// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Joins a video's picture and sound into one WebM or Matroska file: `dm_media::mkvmux` (gaps
//! row A6), with the engine's calls of it off the async runtime.

use std::path::PathBuf;

pub use dm_media::mkvmux::*;

use crate::mp4mux::Tags;
use crate::subtitles::Subtitle;

/// [`join`], off the async runtime.
pub async fn join_async(inputs: Vec<PathBuf>, out: PathBuf) -> Result<u64, String> {
    join_with_text_async(inputs, Vec::new(), out).await
}

/// [`join_with_text`], off the async runtime.
pub async fn join_with_text_async(
    inputs: Vec<PathBuf>,
    texts: Vec<Subtitle>,
    out: PathBuf,
) -> Result<u64, String> {
    join_tagged_async(inputs, texts, out, Tags::default()).await
}

/// [`join_tagged`], off the async runtime.
pub async fn join_tagged_async(
    inputs: Vec<PathBuf>,
    texts: Vec<Subtitle>,
    out: PathBuf,
    tags: Tags,
) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join_tagged(&inputs, &texts, &out, &tags))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}

/// [`join_placed`], off the async runtime: `place` times the subtitles from when the output
/// starts on the inputs' own timeline, in 90 kHz ticks (gaps row Y20).
pub async fn join_placed_async<F>(
    inputs: Vec<PathBuf>,
    place: F,
    out: PathBuf,
) -> Result<u64, String>
where
    F: FnOnce(i64) -> Vec<Subtitle> + Send + 'static,
{
    tokio::task::spawn_blocking(move || join_placed(&inputs, place, &out))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}
