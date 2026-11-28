// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The folder an archive is extracted into.

use std::path::{Path, PathBuf};

/// The folder an archive extracts into: its name without its last extension, made
/// unique beside it.
pub fn destination_for(archive: &Path) -> PathBuf {
    destination_in(archive, archive.parent().unwrap_or_else(|| Path::new(".")))
}

/// [`destination_for`] inside a chosen `parent` folder instead. The first part of an archive
/// in parts goes into a folder of the archive's name (`film.7z.001`, `film.part1.rar` into
/// `film`).
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
            } else if let Some(set) = rar_set(archive, &s) {
                set
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

/// The name of the RAR set whose first part is `archive`, its stem `stem` (`film.part1`, or
/// `film.part01`, gives `film`); `None` for any other name.
fn rar_set(archive: &Path, stem: &str) -> Option<String> {
    if !archive
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rar"))
    {
        return None;
    }
    let digits_at = stem.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let before = stem.get(..digits_at)?;
    let first = stem.get(digits_at..)?.parse::<u32>().ok()? == 1;
    let at = before.len().checked_sub(5)?;
    (first && before.is_char_boundary(at) && before[at..].eq_ignore_ascii_case(".part"))
        .then(|| before[..at].to_string())
}
