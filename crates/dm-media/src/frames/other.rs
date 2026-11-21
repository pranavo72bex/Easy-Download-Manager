// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Any system but macOS, Windows and Linux: no video decoder dm uses, so no
//! GIF.

use std::path::Path;

use super::Frame;
use crate::Error;

pub(super) fn read(_: &Path, _: u32, _: u32) -> Result<(u16, u16, Vec<Frame>), Error> {
    Err(
        "saving as GIF needs the system's video decoder, which dm uses on macOS and Windows only so far"
            .into(),
    )
}

pub(super) fn possible() -> bool {
    false
}
