// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Linux (and any system but macOS and Windows): the built-in AAC decoder,
//! `crate::aac`; Opus is read by dm's own decoder, `crate::opus`.

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

/// No Opus decoder of the system's here: dm's own reads it (gaps row Y25).
pub(super) fn opus_possible() -> bool {
    false
}
