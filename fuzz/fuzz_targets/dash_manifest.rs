// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! A DASH manifest a server sends (gaps row H11).
use edm_core::Quality;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = edm_core::dash::plan(&text, "https://example.com/v/m.mpd", Quality::Best, None);
    let _ = edm_core::dash::plan(
        &text,
        "https://example.com/v/m.mpd",
        Quality::Audio,
        Some("mp4"),
    );
});
