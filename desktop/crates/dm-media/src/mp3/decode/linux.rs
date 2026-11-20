// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Linux (and any system but macOS and Windows): the built-in AAC decoder,
//! `crate::aac`; no Opus decoder in dm yet.

use std::path::Path;

use super::Pcm;
use crate::Error;

pub(super) fn read(path: &Path) -> Result<Pcm, Error> {
    crate::aac::read(path)
}

/// The built-in AAC decoder is there on every system.
pub(super) fn possible() -> bool {
    true
}

/// No Opus decoder in dm here (gaps row Y25).
pub(super) fn opus_possible() -> bool {
    false
}
