// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! A Metalink a server sends in a file's place (gaps row H11).
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = edm_core::parse_metalink(&String::from_utf8_lossy(data));
});
