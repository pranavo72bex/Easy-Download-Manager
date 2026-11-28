// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#![no_main]

//! A ZIP's list of files, read from its end as Download details reads it (gaps row H11).
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let Ok(dir) = tempfile::tempdir() else {
        return;
    };
    let file = dir.path().join("a.zip");
    if std::fs::write(&file, data).is_ok() {
        let _ = edm_core::zip_contents_of_file(&file);
    }
});
