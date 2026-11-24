// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! An MP4's boxes, as joining a picture and its sound reads them (gaps row H11).
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = dm_media::mp4mux::init_shape(data);
    let _ = dm_media::mp4mux::picture_kind(data);
});
