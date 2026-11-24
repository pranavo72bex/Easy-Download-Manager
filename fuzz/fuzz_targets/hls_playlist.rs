// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! An HLS playlist a server sends, master or media (gaps row H11).
use dm_core::{kind_of, media_playlist, pick_quality, stream_variants, Quality};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let base = "https://example.com/v/index.m3u8";
    let offered = stream_variants(&text, base);
    let _ = pick_quality(offered, Quality::UpTo(720));
    let _ = media_playlist(&text, base, "name", Quality::Best);
    let _ = kind_of(base, Some(&text));
});
