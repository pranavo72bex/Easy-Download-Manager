// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! For the tests alone: RAR 4 packed streams built in code from the format's description, so
//! the decompressor runs on bits whose every field is known. Like RAR 5's (`../writer.rs`)
//! the encoder is plain: one code length per table, a greedy matcher, no effort to pack well.
//! It is not RAR's own packer, and nothing here proves dm against archives that packer made.

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::collections::HashMap;

use super::super::writer::{codes, BitWriter};
use super::tables::{
    ALL, DIST, DIST_BASE, DIST_BITS, LENGTH_BASE, LENGTH_BITS, LOW, MAIN, REP, SHORT_BASE,
    SHORT_BITS,
};

/// One code of a RAR 4 LZ stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Token {
    Lit(u8),
    /// A length and a distance, in the long form.
    Match(u64, u64),
    /// As `Match`, its lowest four distance bits given by the repeat code: they must be the
    /// last ones.
    MatchLowRepeat(u64, u64),
    /// A two-byte match at a short distance (1 to 256).
    Short(u64),
    /// One of the last four distances, by its place, with a length.
    Repeat(usize, u64),
    /// The last match again.
    Last,
    /// A filter's record: its first byte (its lowest three bits are the writer's) and the
    /// bytes after it.
    Filter(u8, Vec<u8>),
}

const T_MAIN: usize = 0;
const T_DIST: usize = MAIN;
const T_LOW: usize = MAIN + DIST;
const T_REP: usize = MAIN + DIST + LOW;

/// A length's slot and extra bits for `v`, the length less its base (3, or 2 after a repeat).
fn length_slot(v: u64) -> (usize, u64, u32) {
    for slot in (0..REP).rev() {
        let base = u64::from(LENGTH_BASE[slot]);
        if v >= base {
            let bits = u32::from(LENGTH_BITS[slot]);
            assert!(v - base < 1 << bits, "length {v} too long");
            return (slot, v - base, bits);
        }
    }
    unreachable!()
}

/// A distance's slot and extra bits.
fn dist_slot(dist: u64) -> (usize, u64, u32) {
    let d = dist - 1;
    for slot in (0..DIST).rev() {
        if d >= u64::from(DIST_BASE[slot]) {
            return (
                slot,
                d - u64::from(DIST_BASE[slot]),
                u32::from(DIST_BITS[slot]),
            );
        }
    }
    unreachable!()
}

/// The length a long match writes, less its base, after a far distance's additions.
fn long_length(len: u64, dist: u64) -> u64 {
    let adj = u64::from(dist >= 0x2000) + u64::from(dist >= 0x4_0000);
    len - adj - 3
}

/// Code lengths for `tokens` (and the end-of-block code): every symbol used in a table gets
/// the same length.
pub(super) fn lengths_for(tokens: &[Token]) -> [u8; ALL] {
    let mut used = [false; ALL];
    used[T_MAIN + 256] = true;
    for t in tokens {
        match *t {
            Token::Lit(b) => used[usize::from(b)] = true,
            Token::Match(len, dist) | Token::MatchLowRepeat(len, dist) => {
                used[271 + length_slot(long_length(len, dist)).0] = true;
                let (slot, extra, _) = dist_slot(dist);
                used[T_DIST + slot] = true;
                if slot > 9 {
                    used[T_LOW + (extra & 15) as usize] = true;
                    used[T_LOW + 16] = true;
                }
            }
            Token::Short(dist) => {
                let i = (0..8)
                    .rev()
                    .find(|&i| dist - 1 >= u64::from(SHORT_BASE[i]))
                    .unwrap();
                used[263 + i] = true;
            }
            Token::Repeat(n, len) => {
                used[259 + n] = true;
                used[T_REP + length_slot(len - 2).0] = true;
            }
            Token::Last => used[258] = true,
            Token::Filter(..) => used[257] = true,
        }
    }
    let mut lengths = [0u8; ALL];
    for (start, size) in [(T_MAIN, MAIN), (T_DIST, DIST), (T_LOW, LOW), (T_REP, REP)] {
        let count = used[start..start + size].iter().filter(|&&u| u).count();
        let bits = (1..16).find(|&b| 1usize << b >= count).unwrap() as u8;
        for i in start..start + size {
            lengths[i] = if used[i] { bits } else { 0 };
        }
    }
    lengths
}

/// A RAR 4 stream being written.
pub(super) struct Writer {
    pub w: BitWriter,
    lengths: [u8; ALL],
    codes: Vec<u32>,
    low_last: u64,
    low_repeats: u32,
}

impl Writer {
    pub(super) fn new() -> Writer {
        Writer {
            w: BitWriter::default(),
            lengths: [0; ALL],
            codes: vec![0; ALL],
            low_last: 0,
            low_repeats: 0,
        }
    }

    fn align(&mut self) {
        while self.w.bits % 8 != 0 {
            self.w.put(0, 1);
        }
    }

    fn sym(&mut self, i: usize) {
        assert!(self.lengths[i] > 0, "symbol {i} has no code");
        self.w
            .put(u64::from(self.codes[i]), u32::from(self.lengths[i]));
    }

    /// New tables at the next byte; `keep` writes each length as a change from the last.
    pub(super) fn tables(&mut self, lengths: [u8; ALL], keep: bool) {
        self.align();
        self.w.put(0, 1);
        self.w.put(u64::from(keep), 1);
        // Each length a code of its own (as a change, under `keep`), zeros in runs.
        let mut syms: Vec<(u8, u64, u32)> = Vec::new();
        let mut i = 0;
        while i < ALL {
            let zeros = lengths[i..].iter().take_while(|&&l| l == 0).count();
            if zeros >= 11 {
                let n = zeros.min(138);
                syms.push((19, n as u64 - 11, 7));
                i += n;
            } else if zeros >= 3 {
                syms.push((18, zeros as u64 - 3, 3));
                i += zeros;
            } else {
                let old = if keep { self.lengths[i] } else { 0 };
                syms.push((lengths[i].wrapping_sub(old) & 15, 0, 0));
                i += 1;
            }
        }
        // Every level code, four bits each.
        let levels = [5u8; 20];
        for &l in &levels {
            self.w.put(u64::from(l), 4);
        }
        let level_codes = codes(&levels);
        for (s, extra, n) in syms {
            self.w.put(u64::from(level_codes[usize::from(s)]), 5);
            if n > 0 {
                self.w.put(extra, n);
            }
        }
        self.lengths = lengths;
        self.codes = codes(&lengths);
        self.low_last = 0;
        self.low_repeats = 0;
    }

    pub(super) fn token(&mut self, t: &Token) {
        match *t {
            Token::Lit(b) => self.sym(usize::from(b)),
            Token::Match(len, dist) => self.long(len, dist, false),
            Token::MatchLowRepeat(len, dist) => self.long(len, dist, true),
            Token::Short(dist) => {
                let i = (0..8)
                    .rev()
                    .find(|&i| dist - 1 >= u64::from(SHORT_BASE[i]))
                    .unwrap();
                self.sym(263 + i);
                self.w.put(
                    dist - 1 - u64::from(SHORT_BASE[i]),
                    u32::from(SHORT_BITS[i]),
                );
            }
            Token::Repeat(n, len) => {
                self.sym(259 + n);
                let (slot, extra, bits) = length_slot(len - 2);
                self.sym(T_REP + slot);
                self.w.put(extra, bits);
            }
            Token::Last => self.sym(258),
            Token::Filter(first, ref body) => {
                self.sym(257);
                let n = body.len();
                let (low, more): (u8, Vec<(u64, u32)>) = match n {
                    1..=6 => (n as u8 - 1, vec![]),
                    7..=262 => (6, vec![(n as u64 - 7, 8)]),
                    _ => (7, vec![(n as u64, 16)]),
                };
                self.w.put(u64::from((first & !7) | low), 8);
                for (v, bits) in more {
                    self.w.put(v, bits);
                }
                for &b in body {
                    self.w.put(u64::from(b), 8);
                }
            }
        }
    }

    fn long(&mut self, len: u64, dist: u64, low_repeat: bool) {
        let (slot, extra, bits) = length_slot(long_length(len, dist));
        self.sym(271 + slot);
        self.w.put(extra, bits);
        let (slot, extra, bits) = dist_slot(dist);
        self.sym(T_DIST + slot);
        if slot <= 9 {
            self.w.put(extra, bits);
            return;
        }
        self.w.put(extra >> 4, bits - 4);
        let low = extra & 15;
        if self.low_repeats > 0 {
            assert_eq!(low, self.low_last, "a repeated low part must be the last");
            self.low_repeats -= 1;
        } else if low_repeat {
            assert_eq!(low, self.low_last, "a repeated low part must be the last");
            self.sym(T_LOW + 16);
            self.low_repeats = 15;
        } else {
            self.sym(T_LOW + low as usize);
            self.low_last = low;
        }
    }

    /// Codes under `lengths` without writing them: a solid file's tables carried on.
    pub(super) fn carry(&mut self, lengths: [u8; ALL]) {
        self.lengths = lengths;
        self.codes = codes(&lengths);
    }

    /// The end of a block with new tables after it.
    pub(super) fn new_tables(&mut self, lengths: [u8; ALL], keep: bool) {
        self.sym(256);
        self.w.put(1, 1);
        self.tables(lengths, keep);
    }

    /// The end of the file; `next_tables` says the next one starts with its own.
    pub(super) fn end(&mut self, next_tables: bool) {
        self.sym(256);
        self.w.put(0, 1);
        self.w.put(u64::from(next_tables), 1);
    }

    pub(super) fn bytes(self) -> Vec<u8> {
        self.w.bytes
    }
}

/// A file's whole packed stream: tables for `tokens`, the tokens, the end of the file.
pub(super) fn pack(tokens: &[Token]) -> Vec<u8> {
    let mut w = Writer::new();
    w.tables(lengths_for(tokens), false);
    for t in tokens {
        w.token(t);
    }
    w.end(false);
    w.bytes()
}

/// Tokens for `data` from a greedy matcher over its three-byte prefixes (matches of at most
/// 255 bytes); `before` is what a solid stream unpacked already.
pub(super) fn greedy(before: &[u8], data: &[u8]) -> Vec<Token> {
    let all: Vec<u8> = before.iter().chain(data).copied().collect();
    let mut last: HashMap<[u8; 3], usize> = HashMap::new();
    for (i, w) in all
        .windows(3)
        .enumerate()
        .take(before.len().saturating_sub(2))
    {
        last.insert([w[0], w[1], w[2]], i);
    }
    let mut out = Vec::new();
    let mut i = before.len();
    while i < all.len() {
        let key = (i + 3 <= all.len()).then(|| [all[i], all[i + 1], all[i + 2]]);
        let found = key.and_then(|k| last.get(&k).copied());
        let mut taken = 1;
        match found {
            Some(p) if i - p < 0x3f_0000 => {
                let len = (0..255)
                    .take_while(|&k| i + k < all.len() && all[p + k] == all[i + k])
                    .count();
                let dist = (i - p) as u64;
                let adj = usize::from(dist >= 0x2000) + usize::from(dist >= 0x4_0000);
                if len >= 3 + adj {
                    out.push(Token::Match(len as u64, dist));
                    taken = len;
                } else {
                    out.push(Token::Lit(all[i]));
                }
            }
            _ => out.push(Token::Lit(all[i])),
        }
        for (k, w) in all.windows(3).enumerate().take(i + taken).skip(i) {
            last.insert([w[0], w[1], w[2]], k);
        }
        i += taken;
    }
    out
}

/// A number in a filter's record, as `vm::number` reads it.
pub(super) fn number(w: &mut BitWriter, v: u32) {
    if v < 16 {
        w.put(0, 2);
        w.put(u64::from(v), 4);
    } else if v < 256 {
        w.put(1, 2);
        w.put(u64::from(v), 8);
    } else if v < 0x1_0000 {
        w.put(2, 2);
        w.put(u64::from(v), 16);
    } else {
        w.put(3, 2);
        w.put(u64::from(v >> 16), 16);
        w.put(u64::from(v & 0xffff), 16);
    }
}

/// A program of `len` bytes whose first byte is the XOR of the others and whose CRC32 is
/// `crc`: what a standard filter's program is known by. Six of its bytes are solved for,
/// over GF(2), the CRC and the XOR both being linear in the bits.
pub(super) fn program(len: usize, crc: u32) -> Vec<u8> {
    let mark = |p: &[u8]| -> u64 {
        let xor = p.iter().fold(0u8, |a, &b| a ^ b);
        u64::from(crc32fast::hash(p)) | (u64::from(xor) << 32)
    };
    let mut base: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
    let start = mark(&base);
    // Column j: what flipping bit j of bytes 1 to 6 does to the mark.
    let cols: Vec<u64> = (0..48)
        .map(|j| {
            let mut q = base.clone();
            q[1 + j / 8] ^= 1 << (j % 8);
            mark(&q) ^ start
        })
        .collect();
    let want = start ^ u64::from(crc);
    // Gaussian elimination on 40 equations (the mark's bits) in 48 unknowns.
    let mut rows: Vec<(u64, bool)> = (0..40)
        .map(|r| {
            let mut m = 0u64;
            for (j, c) in cols.iter().enumerate() {
                m |= ((c >> r) & 1) << j;
            }
            (m, (want >> r) & 1 == 1)
        })
        .collect();
    let mut pivot_of = vec![usize::MAX; 48];
    let mut r = 0;
    for j in 0..48 {
        if r == 40 {
            break;
        }
        let Some(p) = (r..40).find(|&i| (rows[i].0 >> j) & 1 == 1) else {
            continue;
        };
        rows.swap(r, p);
        for i in 0..40 {
            if i != r && (rows[i].0 >> j) & 1 == 1 {
                rows[i].0 ^= rows[r].0;
                rows[i].1 ^= rows[r].1;
            }
        }
        pivot_of[j] = r;
        r += 1;
    }
    assert!(
        rows[r..].iter().all(|row| !row.1),
        "no program has this mark"
    );
    for j in 0..48 {
        if pivot_of[j] != usize::MAX && rows[pivot_of[j]].1 {
            base[1 + j / 8] ^= 1 << (j % 8);
        }
    }
    assert_eq!(mark(&base), u64::from(crc));
    base
}
