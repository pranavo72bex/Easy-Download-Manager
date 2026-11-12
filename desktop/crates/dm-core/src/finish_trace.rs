// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A trace of a download's finish, for finding where the time after its last byte goes (gaps
//! rows 177 and S5): with `DM_FINISH_TRACE` naming a file, each stage appends a line, its name
//! and the wall clock in milliseconds, which the benchmark's own times (the server's last byte)
//! line up with. Without it, a stage costs one look at a value read once.

use std::io::Write as _;

/// The file the stages are written to, read once.
fn file() -> Option<&'static std::path::Path> {
    static FILE: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    FILE.get_or_init(|| {
        std::env::var_os("DM_FINISH_TRACE")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
    })
    .as_deref()
}

/// Writes down that `stage` is reached now.
pub(crate) fn mark(stage: &str) {
    let Some(path) = file() else {
        return;
    };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    if let Ok(mut out) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(out, "{ms} {stage}");
    }
}
