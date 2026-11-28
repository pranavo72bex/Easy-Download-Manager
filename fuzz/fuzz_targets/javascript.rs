// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! A page's JavaScript, as EDM's own interpreter reads a player's (gaps row H11): held to a
//! short time, as EDM holds it.
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let program = String::from_utf8_lossy(data);
    let _ = edm_js::evaluate(&program, "f(1)", std::time::Duration::from_millis(50));
});
