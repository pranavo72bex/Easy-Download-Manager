// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Pictures of a video, for a part saved as GIF: its H.264 samples decoded
//! by the system's own decoder (VideoToolbox on macOS, called directly,
//! `frames/macos.rs`; on Windows Media Foundation's, `frames/win.rs`), a few
//! a second, made small as each is decoded. On Linux, which has no decoder
//! dm can use, the part is taken from the video's AV1 stream and decoded by
//! rav1d (`frames/av1.rs`, through `frames/linux.rs`).

use std::path::Path;

use crate::gif::Frame;

#[cfg(any(target_os = "linux", test))]
mod av1;

// Each system's decoder, in a file of its own, the same two calls in each.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as system;
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as system;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as system;
#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod other;
#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
use other as system;

/// Pictures `fps` a second of the MP4 at `path`, at most `width` wide,
/// within the part its edit list plays: their size, and the pictures.
pub fn read(path: &Path, fps: u32, width: u32) -> Result<(u16, u16, Vec<Frame>), String> {
    system::read(path, fps.max(1), width.max(16)).map_err(String::from)
}

/// Whether this system has a video decoder [`read`] reads H.264 (or, on
/// Linux, AV1) with (`gif::possible`).
pub(crate) fn possible() -> bool {
    system::possible()
}

/// RGB at `w` by `h` from BGRA rows (`stride` bytes each) of the rectangle
/// `(x, y, cw, ch)`: each output pixel the average of the source pixels it
/// covers, so small text and edges do not shimmer.
#[cfg_attr(
    not(any(target_os = "macos", windows, target_os = "linux", test)),
    allow(dead_code)
)]
fn shrink(
    bgra: &[u8],
    stride: usize,
    rect: (usize, usize, usize, usize),
    w: usize,
    h: usize,
) -> Vec<u8> {
    let (x0, y0, cw, ch) = rect;
    let mut out = Vec::with_capacity(w * h * 3);
    for oy in 0..h {
        let (sy0, sy1) = (
            y0 + oy * ch / h,
            (y0 + (oy + 1) * ch / h).max(y0 + oy * ch / h + 1),
        );
        for ox in 0..w {
            let (sx0, sx1) = (
                x0 + ox * cw / w,
                (x0 + (ox + 1) * cw / w).max(x0 + ox * cw / w + 1),
            );
            let mut sum = [0u32; 3];
            let mut n = 0u32;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let at = sy * stride + sx * 4;
                    if let Some(px) = bgra.get(at..at + 3) {
                        sum[0] += u32::from(px[2]);
                        sum[1] += u32::from(px[1]);
                        sum[2] += u32::from(px[0]);
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            out.extend_from_slice(&[(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]);
        }
    }
    out
}

/// The part of a picture track that plays, in seconds of its own time: what its first edit names,
/// or, with no edit list or an edit of no length, every picture from the first shown to the end of
/// the last (by when each is shown, its decode time and its composition offset: a stream with
/// B-frames shows its first picture after its first decode). Taken as "to the end of time", every
/// such video would be longer than a GIF may be and refused, a one-second one too (coverage
/// tests).
#[cfg_attr(
    not(any(target_os = "macos", windows, target_os = "linux", test)),
    allow(dead_code)
)]
pub(crate) fn played(video: &crate::mp4mux::Track) -> (f64, f64) {
    let scale = f64::from(video.timescale.max(1));
    let mut decoded = i64::try_from(video.start.unwrap_or(0)).unwrap_or(i64::MAX);
    let (mut first, mut last) = (i64::MAX, i64::MIN);
    for sample in &video.samples {
        let shown = decoded.saturating_add(i64::from(sample.cts));
        first = first.min(shown);
        last = last.max(shown.saturating_add(i64::from(sample.duration)));
        decoded = decoded.saturating_add(i64::from(sample.duration));
    }
    let (first, last) = if video.samples.is_empty() {
        (0.0, 0.0)
    } else {
        (first as f64 / scale, last as f64 / scale)
    };
    match video.edits.iter().find(|e| e.media_time >= 0) {
        Some(edit) => {
            let from = edit.media_time as f64 / scale;
            let long = edit.duration as f64 / f64::from(video.edits_timescale.max(1));
            (
                from,
                if long > 0.0 {
                    from + long
                } else {
                    last.max(from)
                },
            )
        }
        None => (first, last),
    }
}

#[cfg(test)]
mod tests;
