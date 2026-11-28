// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Archives in parts (gaps row C3a): `film.part1.rar`, `film.part2.rar`…, `film.7z.001`,
//! `film.7z.002`… and `film.zip.001`… are the parts of one archive, known by their names, so
//! that their rows can be taken as one and the archive unpacked once all its parts are in.

/// The archive `name` is a part of, as that archive's own name (`film.rar`, `film.7z`,
/// `film.zip`), and which part it is (1 for the first); `None` for a name that is no part.
pub(crate) fn part_of(name: &str) -> Option<(String, u32)> {
    let lower = name.to_ascii_lowercase();
    // `film.part2.rar`: the number before `.rar`, after `.part`.
    if let Some(stem) = lower.strip_suffix(".rar") {
        let (before, number) = stem.rsplit_once(".part")?;
        return numbered(number)
            .filter(|_| !before.is_empty())
            .map(|n| (format!("{}.rar", &name[..before.len()]), n));
    }
    // `film.7z.002`, `film.zip.002`: the number is the last extension.
    let (archive, number) = lower.rsplit_once('.')?;
    let kind_is_split = archive.ends_with(".7z") || archive.ends_with(".zip");
    let n = numbered(number).filter(|_| kind_is_split && number.len() >= 3)?;
    let stem = archive.rsplit_once('.').map_or("", |(stem, _)| stem);
    (!stem.is_empty()).then(|| (name[..archive.len()].to_string(), n))
}

/// Whether `names` hold every part of `set` from the first on with none missing between
/// (the last cannot be known by its name: where a part beyond is still to come, its number is
/// not yet among them).
pub(crate) fn all_present(set: &str, names: &[&str]) -> bool {
    let mut numbers: Vec<u32> = names
        .iter()
        .filter_map(|name| part_of(name))
        .filter(|(of, _)| of.eq_ignore_ascii_case(set))
        .map(|(_, n)| n)
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    !numbers.is_empty() && numbers.iter().zip(1..).all(|(&n, want)| n == want)
}

/// A part's number: digits only, from 1.
fn numbered(digits: &str) -> Option<u32> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|&n| n >= 1)
}

#[cfg(test)]
#[path = "volume_tests.rs"]
mod tests;
