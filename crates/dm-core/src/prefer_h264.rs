// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Whether a video's picture is taken in H.264 where it is offered so (gaps row Y8c): above
//! 1080p YouTube's tallest pictures are AV1 or VP9, which many players cannot open in an MP4.
//! On, the tallest H.264 picture within the quality asked is taken, and AV1 or VP9 only for a
//! video with no H.264 at all; off, the tallest picture, as before. Put in force by
//! [`crate::configure`].

use std::sync::atomic::{AtomicBool, Ordering};

static WANTED: AtomicBool = AtomicBool::new(false);

/// Puts the choice in force for every video read from now on.
#[doc(hidden)]
pub fn set_prefer_h264(on: bool) {
    WANTED.store(on, Ordering::Relaxed);
}

/// Whether H.264 is preferred.
pub fn wanted() -> bool {
    WANTED.load(Ordering::Relaxed)
}
