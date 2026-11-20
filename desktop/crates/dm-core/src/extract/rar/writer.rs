// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! For the tests alone: RAR 5 archives and packed streams built in code, from the format's
//! description, so the reader and the decompressor can be run on bytes whose every field is
//! known. The encoder is deliberately plain (canonical codes of one length per table, a
//! greedy matcher); it is not dm's and does not try to pack well.

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::collections::HashMap;

use super::huffman::{ALIGN, ALL, DIST, LEN, MAIN};

/// One code of a packed stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Token {
    Lit(u8),
    /// A length and a distance.
    Match(u64, u64),
    /// One of the last four distances, by its place, with a length.
    Repeat(usize, u64),
    /// The last match again.
    Last,
    /// A filter: its start from here, its length, its kind, DELTA's channels.
    Filter(u32, u32, u8, u32),
}

/// Bits written from the highest of each byte.
#[derive(Default)]
pub(super) struct BitWriter {
    pub bytes: Vec<u8>,
    pub bits: u64,
}

impl BitWriter {
    pub(super) fn put(&mut self, value: u64, n: u32) {
        for i in (0..n).rev() {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                *self.bytes.last_mut().unwrap() |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
        }
    }
}

/// Which table a symbol is of: its offset in the 430 lengths.
#[derive(Debug, Clone, Copy)]
enum Piece {
    Sym(usize, usize),
    Raw(u64, u32),
}

const T_MAIN: usize = 0;
const T_DIST: usize = MAIN;
const T_ALIGN: usize = MAIN + DIST;
const T_LEN: usize = MAIN + DIST + ALIGN;

/// A length's slot and extra bits (`v` is the length less 2).
fn length_slot(v: u64) -> (usize, u64, u32) {
    if v < 8 {
        return (v as usize, 0, 0);
    }
    for slot in 8..LEN as u32 {
        let n = slot / 4 - 1;
        let base = u64::from(4 | (slot & 3)) << n;
        if v >= base && v < base + (1 << n) {
            return (slot as usize, v - base, n);
        }
    }
    panic!("length {v} too long");
}

fn length_pieces(len: u64, table: usize, out: &mut Vec<Piece>) {
    let (slot, extra, n) = length_slot(len - 2);
    out.push(Piece::Sym(table, slot));
    if n > 0 {
        out.push(Piece::Raw(extra, n));
    }
}

fn number(v: u32, out: &mut Vec<Piece>) {
    let count = (1..=4u32).find(|&c| c == 4 || v >> (8 * c) == 0).unwrap();
    out.push(Piece::Raw(u64::from(count) - 1, 2));
    for i in 0..count {
        out.push(Piece::Raw(u64::from((v >> (8 * i)) & 0xff), 8));
    }
}

fn pieces(token: Token, out: &mut Vec<Piece>) {
    match token {
        Token::Lit(b) => out.push(Piece::Sym(T_MAIN, usize::from(b))),
        Token::Filter(start, len, kind, channels) => {
            out.push(Piece::Sym(T_MAIN, 256));
            number(start, out);
            number(len, out);
            out.push(Piece::Raw(u64::from(kind), 3));
            if kind == 0 {
                out.push(Piece::Raw(u64::from(channels) - 1, 5));
            }
        }
        Token::Last => out.push(Piece::Sym(T_MAIN, 257)),
        Token::Repeat(n, len) => {
            out.push(Piece::Sym(T_MAIN, 258 + n));
            length_pieces(len, T_LEN, out);
        }
        Token::Match(len, dist) => {
            let adj = [0x100, 0x2000, 0x40000]
                .iter()
                .filter(|&&f| dist > f)
                .count() as u64;
            let (slot, extra, n) = length_slot(len - adj - 2);
            out.push(Piece::Sym(T_MAIN, 262 + slot));
            if n > 0 {
                out.push(Piece::Raw(extra, n));
            }
            let d = dist - 1;
            if d < 4 {
                out.push(Piece::Sym(T_DIST, d as usize));
                return;
            }
            let slot = (4..DIST as u32)
                .find(|&s| {
                    let n = s / 2 - 1;
                    let base = u64::from(2 | (s & 1)) << n;
                    d >= base && d < base + (1 << n)
                })
                .unwrap();
            let n = slot / 2 - 1;
            let extra = d - (u64::from(2 | (slot & 1)) << n);
            out.push(Piece::Sym(T_DIST, slot as usize));
            if n >= 4 {
                if n > 4 {
                    out.push(Piece::Raw(extra >> 4, n - 4));
                }
                out.push(Piece::Sym(T_ALIGN, (extra & 15) as usize));
            } else {
                out.push(Piece::Raw(extra, n));
            }
        }
    }
}

/// Code lengths for `tokens`: in each table, every symbol used gets the same length.
pub(super) fn lengths_for(tokens: &[Token]) -> [u8; ALL] {
    let mut all = Vec::new();
    for &t in tokens {
        pieces(t, &mut all);
    }
    let mut used = [false; ALL];
    for p in &all {
        if let Piece::Sym(table, sym) = p {
            used[table + sym] = true;
        }
    }
    let mut lengths = [0u8; ALL];
    for (start, size) in [
        (T_MAIN, MAIN),
        (T_DIST, DIST),
        (T_ALIGN, ALIGN),
        (T_LEN, LEN),
    ] {
        uniform(
            &used[start..start + size],
            &mut lengths[start..start + size],
        );
    }
    lengths
}

/// Gives every used symbol the same, shortest length that holds them all.
fn uniform(used: &[bool], lengths: &mut [u8]) {
    let count = used.iter().filter(|&&u| u).count();
    let bits = (1..16).find(|&b| 1usize << b >= count).unwrap() as u8;
    for (l, &u) in lengths.iter_mut().zip(used) {
        *l = if u { bits } else { 0 };
    }
}

/// Canonical codes: shorter first, within one length by symbol.
pub(super) fn codes(lengths: &[u8]) -> Vec<u32> {
    let mut order: Vec<usize> = (0..lengths.len()).filter(|&i| lengths[i] > 0).collect();
    order.sort_by_key(|&i| (lengths[i], i));
    let mut out = vec![0u32; lengths.len()];
    let (mut code, mut prev) = (0u32, 0u8);
    for i in order {
        code <<= lengths[i] - prev;
        prev = lengths[i];
        out[i] = code;
        code += 1;
    }
    out
}

/// The 430 lengths, run-length coded under the 20 level codes, with those codes' own lengths
/// before them.
pub(super) fn write_tables(w: &mut BitWriter, lengths: &[u8; ALL]) {
    let mut syms: Vec<(u8, u64, u32)> = Vec::new();
    let mut i = 0;
    while i < ALL {
        let v = lengths[i];
        let run = lengths[i..].iter().take_while(|&&l| l == v).count();
        if v == 0 && run >= 11 {
            let n = run.min(138);
            syms.push((19, n as u64 - 11, 7));
            i += n;
        } else if v == 0 && run >= 3 {
            let n = run.min(10);
            syms.push((18, n as u64 - 3, 3));
            i += n;
        } else if i > 0 && lengths[i - 1] == v && run >= 11 {
            let n = run.min(138);
            syms.push((17, n as u64 - 11, 7));
            i += n;
        } else if i > 0 && lengths[i - 1] == v && run >= 3 {
            let n = run.min(10);
            syms.push((16, n as u64 - 3, 3));
            i += n;
        } else {
            syms.push((v, 0, 0));
            i += 1;
        }
    }
    let mut used = [false; 20];
    for &(s, _, _) in &syms {
        used[usize::from(s)] = true;
    }
    let mut levels = [0u8; 20];
    uniform(&used, &mut levels);
    // The level codes' lengths: four bits each, a run of three or more zeros as 15 and a
    // count.
    let mut j = 0;
    while j < 20 {
        let zeros = levels[j..].iter().take_while(|&&l| l == 0).count();
        if zeros >= 3 {
            let n = zeros.min(17);
            w.put(15, 4);
            w.put(n as u64 - 2, 4);
            j += n;
        } else {
            w.put(u64::from(levels[j]), 4);
            j += 1;
        }
    }
    let level_codes = codes(&levels);
    for (s, extra, n) in syms {
        let s = usize::from(s);
        w.put(u64::from(level_codes[s]), u32::from(levels[s]));
        if n > 0 {
            w.put(extra, n);
        }
    }
}

/// One block: its header, then, where `tables`, the tables of `lengths`, then `tokens`.
pub(super) fn block(tokens: &[Token], lengths: &[u8; ALL], tables: bool, last: bool) -> Vec<u8> {
    let mut w = BitWriter::default();
    if tables {
        write_tables(&mut w, lengths);
    }
    // Each table's codes are canonical on their own.
    let mut codes_of = vec![0u32; ALL];
    for (start, size) in [
        (T_MAIN, MAIN),
        (T_DIST, DIST),
        (T_ALIGN, ALIGN),
        (T_LEN, LEN),
    ] {
        codes_of[start..start + size].copy_from_slice(&codes(&lengths[start..start + size]));
    }
    let codes = codes_of;
    let mut all = Vec::new();
    for &t in tokens {
        pieces(t, &mut all);
    }
    for p in all {
        match p {
            Piece::Sym(table, sym) => {
                let i = table + sym;
                assert!(lengths[i] > 0, "symbol {sym} of table {table} has no code");
                w.put(u64::from(codes[i]), u32::from(lengths[i]));
            }
            Piece::Raw(v, n) => w.put(v, n),
        }
    }
    let size = w.bytes.len();
    let bit_size = if size == 0 {
        1
    } else {
        w.bits - (size as u64 - 1) * 8
    };
    let count = if size < 0x100 {
        1
    } else if size < 0x1_0000 {
        2
    } else {
        3
    };
    let mut flags = (bit_size as u8 - 1) | ((count - 1) << 3);
    if last {
        flags |= 0x40;
    }
    if tables {
        flags |= 0x80;
    }
    let s = (size as u32).to_le_bytes();
    let mut out = vec![flags, 0x5a ^ flags ^ s[0] ^ s[1] ^ s[2]];
    out.extend_from_slice(&s[..usize::from(count)]);
    out.extend_from_slice(&w.bytes);
    out
}

/// A whole file's packed bytes: one block with its tables.
pub(super) fn pack(tokens: &[Token]) -> Vec<u8> {
    block(tokens, &lengths_for(tokens), true, true)
}

/// Tokens for `data` from a greedy matcher over its three-byte prefixes, reaching back at
/// most `window` bytes; `before` is what a solid stream unpacked already.
pub(super) fn greedy(before: &[u8], data: &[u8], window: usize) -> Vec<Token> {
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
            Some(p) if i - p <= window => {
                let dist = (i - p) as u64;
                let len = (0..4000)
                    .take_while(|&k| i + k < all.len() && all[p + k] == all[i + k])
                    .count() as u64;
                let adj = [0x100, 0x2000, 0x40000]
                    .iter()
                    .filter(|&&f| dist > f)
                    .count() as u64;
                if len >= 3 + adj {
                    out.push(Token::Match(len, dist));
                    taken = len as usize;
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

/// A variable-length number, as RAR 5 writes them.
pub(super) fn vint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

/// One RAR 5 block: CRC32, size, type, flags, the sizes of its extra and data areas, its
/// `fields`, its `extra` area, then `data` where it has a data area.
pub(super) fn rar5_block(kind: u64, fields: &[u8], extra: &[u8], data: Option<&[u8]>) -> Vec<u8> {
    let mut body = vint(kind);
    let flags = u64::from(!extra.is_empty()) | (u64::from(data.is_some()) << 1);
    body.extend(vint(flags));
    if !extra.is_empty() {
        body.extend(vint(extra.len() as u64));
    }
    if let Some(d) = data {
        body.extend(vint(d.len() as u64));
    }
    body.extend_from_slice(fields);
    body.extend_from_slice(extra);
    let mut hashed = vint(body.len() as u64);
    hashed.extend(body);
    let mut out = crc32fast::hash(&hashed).to_le_bytes().to_vec();
    out.extend(hashed);
    out.extend_from_slice(data.unwrap_or_default());
    out
}

/// A RAR 5 file as the tests describe it.
pub(super) struct File5<'a> {
    pub name: &'a str,
    pub size: u64,
    pub packed: &'a [u8],
    pub crc: Option<u32>,
    /// The compression information: version, solid, method, dictionary.
    pub info: u64,
    pub dir: bool,
    /// Host system (0 Windows, 1 Unix) and attributes.
    pub host: u64,
    pub attributes: u64,
    /// Extra records, each already its type and data.
    pub records: Vec<Vec<u8>>,
}

impl<'a> File5<'a> {
    /// A file stored as it is.
    pub(super) fn stored(name: &'a str, data: &'a [u8]) -> File5<'a> {
        File5 {
            name,
            size: data.len() as u64,
            packed: data,
            crc: Some(crc32fast::hash(data)),
            info: 0,
            dir: false,
            host: 0,
            attributes: 0x20,
            records: Vec::new(),
        }
    }

    /// A file of `plain` packed as `packed` by method 3, with a 4 MiB dictionary.
    pub(super) fn packed(name: &'a str, plain: &[u8], packed: &'a [u8], solid: bool) -> Self {
        File5 {
            info: (3 << 7) | (5 << 10) | if solid { 0x40 } else { 0 },
            size: plain.len() as u64,
            crc: Some(crc32fast::hash(plain)),
            packed,
            ..File5::stored(name, &[])
        }
    }

    pub(super) fn bytes(&self) -> Vec<u8> {
        let mut f = vint(u64::from(self.dir) | if self.crc.is_some() { 0x04 } else { 0 });
        f.extend(vint(self.size));
        f.extend(vint(self.attributes));
        if let Some(crc) = self.crc {
            f.extend_from_slice(&crc.to_le_bytes());
        }
        f.extend(vint(self.info));
        f.extend(vint(self.host));
        f.extend(vint(self.name.len() as u64));
        f.extend_from_slice(self.name.as_bytes());
        let mut extra = Vec::new();
        for r in &self.records {
            extra.extend(vint(r.len() as u64));
            extra.extend_from_slice(r);
        }
        rar5_block(2, &f, &extra, Some(self.packed))
    }
}

/// A RAR 5 archive: its signature, a main header with `archive_flags`, the files, its end.
pub(super) fn rar5(archive_flags: u64, files: &[File5]) -> Vec<u8> {
    let mut out = super::header::SIGNATURE.to_vec();
    out.extend(rar5_block(1, &vint(archive_flags), &[], None));
    for f in files {
        out.extend(f.bytes());
    }
    out.extend(rar5_block(5, &vint(0), &[], None));
    out
}

/// A RAR 4 archive of `files` (name, data, method byte: 0x30 stored), each with its CRC.
pub(super) fn rar4(files: &[(&str, &[u8], u8)]) -> Vec<u8> {
    let files: Vec<File4> = files
        .iter()
        .map(|&(name, data, method)| File4 {
            name,
            plain: data,
            packed: data,
            method,
            version: 29,
            flags: 0,
        })
        .collect();
    rar4_files(&files)
}

/// One file of a RAR 4 archive: what it unpacks to, its packed bytes, its method byte, the
/// version needed to unpack it, and its header flags (0x10: solid).
pub(super) struct File4<'a> {
    pub name: &'a str,
    pub plain: &'a [u8],
    pub packed: &'a [u8],
    pub method: u8,
    pub version: u8,
    pub flags: u16,
}

/// A RAR 4 archive of `files`, each header with its CRC.
pub(super) fn rar4_files(files: &[File4]) -> Vec<u8> {
    let mut out = super::header::OLD_SIGNATURE.to_vec();
    out.extend_from_slice(&[0, 0, 0x73, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0]);
    for f in files {
        let mut h = vec![0x74u8];
        h.extend_from_slice(&(0x8000 | f.flags).to_le_bytes());
        h.extend_from_slice(&((32 + f.name.len()) as u16).to_le_bytes());
        h.extend_from_slice(&(f.packed.len() as u32).to_le_bytes());
        h.extend_from_slice(&(f.plain.len() as u32).to_le_bytes());
        h.push(2);
        h.extend_from_slice(&crc32fast::hash(f.plain).to_le_bytes());
        h.extend_from_slice(&[0, 0, 0, 0, f.version, f.method]);
        h.extend_from_slice(&(f.name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0x20u32.to_le_bytes());
        h.extend_from_slice(f.name.as_bytes());
        let crc = (crc32fast::hash(&h) & 0xffff) as u16;
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend(h);
        out.extend_from_slice(f.packed);
    }
    out.extend_from_slice(&[0x3d, 0x7b, 0x7b, 0x00, 0x40, 0x07, 0x00]);
    out
}
