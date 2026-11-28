// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! YouTube's framed stream and an MMS header, part by part (gaps row H11).
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let mut rest = data;
    while let Ok(Some((_, _, taken))) = edm_core::youtube::framed::ump::part(rest) {
        match rest.get(taken..) {
            Some(next) if next.len() < rest.len() => rest = next,
            _ => break,
        }
    }
    let _ = edm_core::mms::header_facts(data);
});
