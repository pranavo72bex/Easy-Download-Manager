// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The folder an archive is extracted into.

use std::path::{Path, PathBuf};

/// The folder an archive extracts into: its name without its last extension, made
/// unique beside it.
pub fn destination_for(archive: &Path) -> PathBuf {
    destination_in(archive, archive.parent().unwrap_or_else(|| Path::new(".")))
}

/// [`destination_for`] inside a chosen `parent` folder instead. The first part of a 7z in
/// parts goes into a folder of the archive's name (`film.7z.001` into `film`).
pub fn destination_in(archive: &Path, parent: &Path) -> PathBuf {
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .map(|s| {
            let parts = archive
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".7z.001");
            if parts {
                s[..s.len() - 3].to_string()
            } else {
                s
            }
        })
        .filter(|s| !s.is_empty() && s != "." && s != "..")
        .unwrap_or_else(|| "extracted".into());
    let mut candidate = parent.join(&stem);
    let mut n = 1;
    while candidate.exists() {
        candidate = parent.join(format!("{stem} ({n})"));
        n += 1;
    }
    candidate
}
