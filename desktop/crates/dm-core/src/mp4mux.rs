// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Joins a video's picture and sound into one MP4: `dm_media::mp4mux` (gaps row A6), with the
//! engine's calls of it off the async runtime.

use std::path::PathBuf;

pub use dm_media::mp4mux::*;

use crate::shape::Shape;
use crate::subtitles::Subtitle;

/// [`join_cut`], off the async runtime.
pub async fn join_cut_async(
    inputs: Vec<PathBuf>,
    texts: Vec<Subtitle>,
    out: PathBuf,
    cut: Option<(u64, u64)>,
    shape: Option<Shape>,
) -> Result<u64, String> {
    join_cut_tagged_async(inputs, texts, out, cut, shape, Tags::default()).await
}

/// [`join_cut_placed`], off the async runtime: `place` times the subtitles from when the
/// output starts on the inputs' own timeline, in 90 kHz ticks (gaps row Y20).
pub async fn join_cut_placed_async<F>(
    inputs: Vec<PathBuf>,
    place: F,
    out: PathBuf,
    cut: Option<(u64, u64)>,
    shape: Option<Shape>,
) -> Result<u64, String>
where
    F: FnOnce(i64) -> Vec<Subtitle> + Send + 'static,
{
    tokio::task::spawn_blocking(move || join_cut_placed(&inputs, place, &out, cut, shape))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}

/// [`join_cut_tagged`], off the async runtime.
pub async fn join_cut_tagged_async(
    inputs: Vec<PathBuf>,
    texts: Vec<Subtitle>,
    out: PathBuf,
    cut: Option<(u64, u64)>,
    shape: Option<Shape>,
    tags: Tags,
) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join_cut_tagged(&inputs, &texts, &out, cut, shape, &tags))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}

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
    tokio::task::spawn_blocking(move || join_with_text(&inputs, &texts, &out))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}
