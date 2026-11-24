// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! An archive unpacked by dm's own readers: the first byte picks ZIP, 7z, RAR or tar, the
//! rest is the file (gaps row H11). Written to a folder of its own and unpacked under the
//! default limits, as Extract here does.
use std::sync::atomic::AtomicBool;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let Some((&pick, body)) = data.split_first() else {
        return;
    };
    let name = ["a.zip", "a.7z", "a.rar", "a.tar"][usize::from(pick) % 4];
    let Ok(dir) = tempfile::tempdir() else {
        return;
    };
    let archive = dir.path().join(name);
    if std::fs::write(&archive, body).is_err() {
        return;
    }
    let _ = dm_core::extract::extract_in(
        &archive,
        Some(dir.path()),
        &dm_core::extract::Limits::default(),
        &AtomicBool::new(false),
        None,
        &|_, _| {},
    );
});
