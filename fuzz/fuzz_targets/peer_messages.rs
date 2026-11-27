// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! What a torrent's peer sends, read as the library reads every peer's message (gaps rows
//! H11a, H11b): an error for anything malformed, never a panic.
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = dm_torrent::read_peer_message(data);
});
