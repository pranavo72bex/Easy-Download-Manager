// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! RAR 5 compression's bits and codes: the packed bytes read as bits from the highest of each
//! byte, and the four Huffman tables each block may give (the main one of literals, matches
//! and repeats; distances; a distance's lowest four bits; the lengths of repeated matches),
//! themselves coded by lengths under a table of 20 codes with runs of repeats and zeros.
//!
//! The codes are canonical: shorter first, and within one length in symbol order. A table
//! need not be complete; a code it does not give decodes as its first symbol, so damaged data
//! decodes to wrong bytes (caught by the file's CRC), never out of bounds.

use super::super::Error;
use super::header::damaged;

/// Symbols of each table: 256 literals, a filter, a repeat of the last match, four repeated
/// distances, 44 lengths; 64 distances; 16 low distance bits; 44 lengths.
pub(super) const MAIN: usize = 306;
pub(super) const DIST: usize = 64;
pub(super) const ALIGN: usize = 16;
pub(super) const LEN: usize = 44;
/// The four tables' code lengths, read one after another.
pub(super) const ALL: usize = MAIN + DIST + ALIGN + LEN;
/// The codes the table lengths are themselves written with.
const LEVELS: usize = 20;

/// The packed bytes of one block, read as bits. Reading past the end gives zero bits: the
/// block's end is the caller's to watch.
pub(super) struct Bits {
    buf: Vec<u8>,
    /// Bits read so far.
    at: u64,
}

impl Bits {
    pub(super) fn new(buf: Vec<u8>) -> Bits {
        Bits { buf, at: 0 }
    }

    /// Bits read so far.
    pub(super) fn at(&self) -> u64 {
        self.at
    }

    fn byte(&self, i: u64) -> u64 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.buf.get(i))
            .map_or(0, |&b| u64::from(b))
    }

    /// The next 16 bits, the first the highest, without taking them.
    pub(super) fn peek16(&self) -> u32 {
        let b = self.at >> 3;
        let x = (self.byte(b) << 16) | (self.byte(b + 1) << 8) | self.byte(b + 2);
        ((x >> (8 - (self.at & 7))) & 0xffff) as u32
    }

    /// The next 32 bits, the first the highest, without taking them.
    pub(super) fn peek32(&self) -> u32 {
        let b = self.at >> 3;
        let mut x = 0u64;
        for i in 0..5 {
            x = (x << 8) | self.byte(b + i);
        }
        ((x >> (8 - (self.at & 7))) & 0xffff_ffff) as u32
    }

    pub(super) fn skip(&mut self, n: u32) {
        self.at += u64::from(n);
    }

    /// The next `n` bits (at most 16) as a number, taken.
    pub(super) fn take(&mut self, n: u32) -> u32 {
        let v = self.peek16() >> (16 - n);
        self.skip(n);
        v
    }
}

/// One Huffman table, ready to decode.
pub(super) struct Decoder {
    /// For each length, the first code past those of that length or shorter, its bits at the
    /// top of 16.
    limit: [u32; 16],
    /// For each length, how many codes are shorter.
    first: [u32; 16],
    /// The symbols, shortest code first.
    symbols: Vec<u16>,
    /// Codes of up to `quick` bits, looked up at once by their bits: length and symbol.
    quick: u32,
    quick_len: Vec<u8>,
    quick_sym: Vec<u16>,
}

impl Decoder {
    /// The table whose symbol `i` has a code `lengths[i]` bits long (0: none); codes of up to
    /// `quick` bits are found in one look.
    pub(super) fn new(lengths: &[u8], quick: u32) -> Decoder {
        let mut count = [0u32; 16];
        for &l in lengths {
            count[usize::from(l & 0x0f)] += 1;
        }
        count[0] = 0;
        let (mut limit, mut first) = ([0u32; 16], [0u32; 16]);
        let mut upper = 0u32;
        for i in 1..16 {
            upper += count[i];
            limit[i] = upper << (16 - i);
            upper *= 2;
            first[i] = first[i - 1] + count[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        let mut next = first;
        for (symbol, &l) in lengths.iter().enumerate() {
            let l = usize::from(l & 0x0f);
            if l != 0 {
                if let Some(slot) = symbols.get_mut(next[l] as usize) {
                    *slot = symbol as u16;
                }
                next[l] += 1;
            }
        }
        let mut d = Decoder {
            limit,
            first,
            symbols,
            quick,
            quick_len: vec![0; 1 << quick],
            quick_sym: vec![0; 1 << quick],
        };
        let mut len = 1usize;
        for code in 0..1u32 << quick {
            let field = code << (16 - quick);
            while len < 16 && field >= d.limit[len] {
                len += 1;
            }
            let (l, s) = (len as u8, d.symbol(field, len));
            if let (Some(ql), Some(qs)) = (
                d.quick_len.get_mut(code as usize),
                d.quick_sym.get_mut(code as usize),
            ) {
                (*ql, *qs) = (l, s);
            }
        }
        d
    }

    /// The symbol whose code of `len` bits starts `field`, or the first where there is none.
    fn symbol(&self, field: u32, len: usize) -> u16 {
        let below = self.limit.get(len - 1).copied().unwrap_or(0);
        let offset = field.wrapping_sub(below) >> (16 - len.min(16));
        let at = self.first.get(len).map(|f| f.wrapping_add(offset) as usize);
        at.and_then(|i| self.symbols.get(i))
            .or(self.symbols.first())
            .copied()
            .unwrap_or(0)
    }

    /// The next symbol, its bits taken.
    pub(super) fn decode(&self, bits: &mut Bits) -> u16 {
        let field = bits.peek16() & 0xfffe;
        let quick = self.quick as usize;
        if field < self.limit[quick] {
            let code = (field >> (16 - self.quick)) as usize;
            bits.skip(u32::from(self.quick_len.get(code).copied().unwrap_or(1)));
            return self.quick_sym.get(code).copied().unwrap_or(0);
        }
        let len = (quick + 1..15)
            .find(|&i| field < self.limit[i])
            .unwrap_or(15);
        bits.skip(len as u32);
        self.symbol(field, len)
    }
}

/// The four tables of a block.
pub(super) struct Tables {
    pub main: Decoder,
    pub dist: Decoder,
    pub align: Decoder,
    pub len: Decoder,
}

/// The repeat count after a length-table code of 16 to 19: three bits and 3, or seven bits
/// and 11.
fn run(bits: &mut Bits, short: bool) -> usize {
    if short {
        bits.take(3) as usize + 3
    } else {
        bits.take(7) as usize + 11
    }
}

/// The code lengths of all four tables, as a block that gives new tables starts.
pub(super) fn read_lengths(bits: &mut Bits) -> Result<[u8; ALL], Error> {
    // The 20 level codes' own lengths: four bits each, 15 followed by a count of zeros
    // (none: the length 15 itself).
    let mut levels = [0u8; LEVELS];
    let mut i = 0;
    while i < LEVELS {
        let l = bits.take(4) as u8;
        if l == 15 {
            let zeros = bits.take(4) as usize;
            if zeros == 0 {
                levels[i] = 15;
                i += 1;
            } else {
                // Zero is what the array holds already.
                i += zeros + 2;
            }
        } else {
            levels[i] = l;
            i += 1;
        }
    }
    let level = Decoder::new(&levels, 7);
    let mut table = [0u8; ALL];
    let mut i = 0;
    while i < ALL {
        match level.decode(bits) {
            n @ 0..=15 => {
                table[i] = n as u8;
                i += 1;
            }
            // A run of the length before.
            n @ (16 | 17) => {
                let count = run(bits, n == 16);
                let Some(&prev) = i.checked_sub(1).and_then(|p| table.get(p)) else {
                    return Err(damaged("a table repeats a length before its first"));
                };
                let end = (i + count).min(ALL);
                table[i..end].fill(prev);
                i = end;
            }
            // A run of zeros.
            n => {
                let count = run(bits, n == 18);
                i = (i + count).min(ALL);
            }
        }
    }
    Ok(table)
}

/// The four tables a block gives.
pub(super) fn read_tables(bits: &mut Bits) -> Result<Tables, Error> {
    let t = read_lengths(bits)?;
    Ok(Tables {
        main: Decoder::new(&t[..MAIN], 10),
        dist: Decoder::new(&t[MAIN..MAIN + DIST], 7),
        align: Decoder::new(&t[MAIN + DIST..MAIN + DIST + ALIGN], 7),
        len: Decoder::new(&t[MAIN + DIST + ALIGN..], 7),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_read_from_the_top_of_each_byte() {
        let mut b = Bits::new(vec![0b1011_0011, 0xff, 0x00, 0x12, 0x34, 0x56]);
        assert_eq!(b.take(3), 0b101);
        assert_eq!(b.peek16(), 0x9ff8);
        b.skip(5);
        assert_eq!(b.peek32(), 0xff00_1234);
        b.skip(40);
        // Past the end: zeros.
        assert_eq!((b.peek16(), b.peek32()), (0, 0));
    }

    #[test]
    fn a_canonical_table_decodes_shorter_codes_first_then_by_symbol() {
        // Symbols 0..4 with lengths 2, 1, 3, 3, 0: codes 1 -> "0", 0 -> "10", 2 -> "110",
        // 3 -> "111".
        let d = Decoder::new(&[2, 1, 3, 3, 0], 2);
        let mut b = Bits::new(vec![0b0101_1011, 0b1000_0000]);
        let got: Vec<u16> = (0..5).map(|_| d.decode(&mut b)).collect();
        assert_eq!(got, [1, 0, 2, 3, 1]);
        assert_eq!(b.at(), 1 + 2 + 3 + 3 + 1);
        // A table of nothing decodes as its first symbol, never out of bounds.
        let empty = Decoder::new(&[0; 8], 7);
        assert_eq!(empty.decode(&mut Bits::new(vec![0xff; 4])), 0);
    }
}
