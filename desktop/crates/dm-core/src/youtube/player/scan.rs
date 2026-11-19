// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where a regular expression and a number end, for `tokens`.

use super::*;

/// Where the regular expression whose `/` is at `i` ends, its flags too. One that does not
/// end on its line is an error.
pub(super) fn pattern_end(b: &[u8], i: usize) -> Result<usize, String> {
    let at = |k: usize| b.get(k).copied().unwrap_or(0);
    let mut j = i + 1;
    let mut class = false;
    loop {
        if j >= b.len() || at(j) == b'\n' {
            return Err(format!("a regular expression that never ends, at byte {i}"));
        }
        match at(j) {
            b'\\' => j += 2,
            b'[' => {
                class = true;
                j += 1;
            }
            b']' => {
                class = false;
                j += 1;
            }
            b'/' if !class => break,
            _ => j += 1,
        }
    }
    j += 1;
    while name_byte(at(j)) {
        j += 1;
    }
    Ok(j)
}

/// Where the number that starts at `i` ends.
pub(super) fn number_end(b: &[u8], i: usize) -> usize {
    let at = |k: usize| b.get(k).copied().unwrap_or(0);
    let c = at(i);
    let hex = c == b'0' && (at(i + 1) | 0x20) == b'x';
    let mut j = i + 1;
    loop {
        let d = at(j);
        let part = d.is_ascii_alphanumeric() || d == b'_' || d == b'.';
        // The sign of an exponent (`1E-6`), which a hex digit `e` is not.
        let sign = (d == b'+' || d == b'-') && !hex && (at(j - 1) | 0x20) == b'e';
        if !(part || sign) {
            break;
        }
        j += 1;
    }
    j
}
