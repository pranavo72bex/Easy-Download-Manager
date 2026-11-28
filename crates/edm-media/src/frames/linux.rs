// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Linux: no system decoder EDM can use, so the part is taken from the
//! video's AV1 stream and decoded by EDM's own reader (`av1.rs`).

use std::path::Path;

use super::Frame;
use crate::Error;

pub(super) fn read(path: &Path, fps: u32, width: u32) -> Result<(u16, u16, Vec<Frame>), Error> {
    super::av1::read(path, fps, width)
}

/// From the video's AV1 stream, decoded by EDM's own reader (`av1.rs`).
pub(super) fn possible() -> bool {
    true
}
