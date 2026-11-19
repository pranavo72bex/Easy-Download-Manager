// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A transport stream put into an MP4: `dm_media::tsmux` (gaps row A6), with the engine's
//! calls of it off the async runtime.

use std::path::PathBuf;

pub use dm_media::tsmux::*;

/// [`to_mp4`], off the async runtime.
pub async fn to_mp4_async(input: PathBuf, out: PathBuf) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || to_mp4(&input, &out))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

/// [`to_mp4_from_with_text`], off the async runtime: a stream put into an MP4 from its segments
/// where they lie (gaps row Y21), `place` timing its subtitles as [`to_mp4_with_text_async`]'s
/// does.
pub async fn to_mp4_from_async<F>(
    inputs: Vec<PathBuf>,
    out: PathBuf,
    segments: Segments,
    place: F,
) -> Result<u64, String>
where
    F: FnOnce(Clock) -> Vec<crate::subtitles::Subtitle> + Send + 'static,
{
    tokio::task::spawn_blocking(move || to_mp4_from_with_text(&inputs, &out, segments, place))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

/// [`to_mp4_with_text`], off the async runtime: `place` times the subtitles from when the MP4
/// starts on the stream's 90 kHz clock, and how its picture's clock was moved at each
/// discontinuity ([`Clock`], gaps row Y20).
pub async fn to_mp4_with_text_async<F>(
    input: PathBuf,
    out: PathBuf,
    place: F,
) -> Result<u64, String>
where
    F: FnOnce(Clock) -> Vec<crate::subtitles::Subtitle> + Send + 'static,
{
    tokio::task::spawn_blocking(move || to_mp4_with_text(&input, &out, place))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

/// [`join_packed_with_text`], off the async runtime.
pub async fn join_packed_with_text_async<F>(
    ts: PathBuf,
    aac: PathBuf,
    out: PathBuf,
    place: F,
) -> Result<u64, String>
where
    F: FnOnce(Clock) -> Vec<crate::subtitles::Subtitle> + Send + 'static,
{
    tokio::task::spawn_blocking(move || join_packed_with_text(&ts, &aac, &out, place))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

/// [`join_ts_with_text`], off the async runtime.
pub async fn join_ts_with_text_async<F>(
    picture: PathBuf,
    sound: PathBuf,
    out: PathBuf,
    place: F,
) -> Result<u64, String>
where
    F: FnOnce(Clock) -> Vec<crate::subtitles::Subtitle> + Send + 'static,
{
    tokio::task::spawn_blocking(move || join_ts_with_text(&picture, &sound, &out, place))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

/// [`join_packed`], off the async runtime.
pub async fn join_packed_async(ts: PathBuf, aac: PathBuf, out: PathBuf) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join_packed(&ts, &aac, &out))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

/// [`join_ts`], off the async runtime.
pub async fn join_ts_async(picture: PathBuf, sound: PathBuf, out: PathBuf) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join_ts(&picture, &sound, &out))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}
