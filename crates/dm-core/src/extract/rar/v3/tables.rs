// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! RAR 4's Huffman tables and the numbers its codes stand for, from the format's public
//! description: the main table (256 literals, the end of a block, a filter, the last match
//! again, four repeated distances, eight short distances and 28 lengths), the distance table
//! (60), the table of a long distance's lowest four bits (16 and a repeat of the last ones),
//! and the table of a repeated distance's length (28).
//!
//! Their 404 code lengths are written as RAR 5 writes its own (`huffman.rs`), under 20 level
//! codes with runs, but each length may be given as a change, modulo 16, from the length the
//! same symbol had in the tables before; a block that does not keep them starts from zeros.

use super::super::super::Error;
use super::super::header::damaged;
use super::super::huffman::{read_levels, run, Bits, Decoder};

/// Symbols of each table.
pub(super) const MAIN: usize = 299;
pub(super) const DIST: usize = 60;
pub(super) const LOW: usize = 17;
pub(super) const REP: usize = 28;
/// The four tables' code lengths, read one after another.
pub(super) const ALL: usize = MAIN + DIST + LOW + REP;

/// A length's base and extra bits, by its slot (of 28).
pub(super) const LENGTH_BASE: [u8; REP] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128,
    160, 192, 224,
];
pub(super) const LENGTH_BITS: [u8; REP] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5,
];

/// A short match's distance base and extra bits (main symbols 263 to 270).
pub(super) const SHORT_BASE: [u8; 8] = [0, 4, 8, 16, 32, 64, 128, 192];
pub(super) const SHORT_BITS: [u8; 8] = [2, 2, 3, 4, 5, 6, 6, 6];

/// How many distance slots have each number of extra bits: four with none, two each with 1
/// to 15, fourteen with 16, none with 17, twelve with 18 (up to 4 MiB).
const DIST_COUNTS: [usize; 19] = [4, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 14, 0, 12];

/// Each distance slot's base (less one) and extra bits.
const fn dist_slots() -> ([u32; DIST], [u8; DIST]) {
    let (mut base, mut bits) = ([0u32; DIST], [0u8; DIST]);
    let (mut dist, mut slot, mut n) = (0u32, 0usize, 0usize);
    while n < DIST_COUNTS.len() {
        let mut j = 0;
        while j < DIST_COUNTS[n] {
            base[slot] = dist;
            bits[slot] = n as u8;
            dist += 1 << n;
            slot += 1;
            j += 1;
        }
        n += 1;
    }
    (base, bits)
}

const SLOTS: ([u32; DIST], [u8; DIST]) = dist_slots();
pub(super) const DIST_BASE: [u32; DIST] = SLOTS.0;
pub(super) const DIST_BITS: [u8; DIST] = SLOTS.1;

/// The four tables of a block.
pub(super) struct Tables {
    pub main: Decoder,
    pub dist: Decoder,
    pub low: Decoder,
    pub rep: Decoder,
}

/// Reads the code lengths after a block's two flag bits; `old` holds the lengths before
/// (zeros where the block does not keep them) and is given the new ones.
pub(super) fn read(bits: &mut Bits, old: &mut [u8; ALL]) -> Result<Tables, Error> {
    let level = read_levels(bits);
    let mut table = [0u8; ALL];
    let mut i = 0;
    while i < ALL {
        match level.decode(bits) {
            // A change from the length before.
            n @ 0..=15 => {
                table[i] = (n as u8).wrapping_add(old[i]) & 0x0f;
                i += 1;
            }
            // A run of the length before this one.
            n @ (16 | 17) => {
                let count = run(bits, n == 16);
                let Some(&prev) = i.checked_sub(1).and_then(|p| table.get(p)) else {
                    return Err(damaged("a table repeats a length before its first"));
                };
                let end = (i + count).min(ALL);
                table[i..end].fill(prev);
                i = end;
            }
            // A run of zeros, as they are.
            n => {
                let count = run(bits, n == 18);
                i = (i + count).min(ALL);
            }
        }
    }
    *old = table;
    Ok(Tables {
        main: Decoder::new(&table[..MAIN], 10),
        dist: Decoder::new(&table[MAIN..MAIN + DIST], 7),
        low: Decoder::new(&table[MAIN + DIST..MAIN + DIST + LOW], 7),
        rep: Decoder::new(&table[MAIN + DIST + LOW..], 7),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_slots_double_up_to_four_mebibytes() {
        assert_eq!(DIST_BASE[..6], [0, 1, 2, 3, 4, 6]);
        assert_eq!(DIST_BITS[..6], [0, 0, 0, 0, 1, 1]);
        // Slot 10 is the first with four extra bits, whose lowest four have a table.
        assert_eq!((DIST_BASE[10], DIST_BITS[10]), (32, 4));
        assert_eq!((DIST_BASE[59], DIST_BITS[59]), (0x3c_0000, 18));
        assert_eq!(DIST_BASE[59] + (1 << 18), 4 << 20);
    }
}
