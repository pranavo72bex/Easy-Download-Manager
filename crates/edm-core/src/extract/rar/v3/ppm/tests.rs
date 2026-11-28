// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! PPMd against streams coded by the tests' own encoder: the range coder's coding side and
//! a choice of symbol for each of the decoder's three cases, over the model's own update
//! code. A round trip proves the decoder reads what that encoder wrote, through escapes, SEE,
//! binary contexts and the model's restarts when its memory fills; it does not prove the
//! model is the one RAR's own program builds, which no archive here was made by.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::super::writer::{lengths_for, Token, Writer};
use super::super::Unpacker;
use super::*;
use crate::extract::Error;

/// The range coder's coding side.
struct Enc {
    low: u32,
    range: u32,
    out: Vec<u8>,
}

impl Enc {
    fn new() -> Enc {
        Enc {
            low: 0,
            range: u32::MAX,
            out: Vec::new(),
        }
    }

    fn encode(&mut self, low: u32, high: u32, scale: u32) {
        self.range /= scale;
        self.low = self.low.wrapping_add(self.range.wrapping_mul(low));
        self.range = self.range.wrapping_mul(high - low);
        loop {
            if self.low ^ self.low.wrapping_add(self.range) >= TOP {
                if self.range >= BOT {
                    return;
                }
                self.range = self.low.wrapping_neg() & (BOT - 1);
            }
            self.out.push((self.low >> 24) as u8);
            self.range <<= 8;
            self.low <<= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        for _ in 0..4 {
            self.out.push((self.low >> 24) as u8);
            self.low <<= 8;
        }
        self.out
    }
}

fn encode_bin(m: &mut Model, e: &mut Enc, sym: u8) {
    let (row, col) = m.bin_context();
    let bs = u32::from(m.bin_summ[row][col]);
    if m.sym(m.min_ctx + 2) == sym {
        e.encode(0, bs, BIN_SCALE);
        m.bin_hit(row, col);
    } else {
        e.encode(bs, BIN_SCALE, BIN_SCALE);
        m.bin_miss(row, col);
    }
}

fn encode1(m: &mut Model, e: &mut Enc, sym: u8) {
    let c = m.min_ctx;
    let (ns, scale) = (m.ns(c), m.summ(c));
    let mut p = m.stats(c);
    let mut hi = m.freq(p);
    if m.sym(p) == sym {
        e.encode(0, hi, scale);
        m.first_hit(c, p);
        return;
    }
    m.prev_success = 0;
    for _ in 1..ns {
        p += STATE;
        let f = m.freq(p);
        if m.sym(p) == sym {
            e.encode(hi, hi + f, scale);
            m.update1(c, p);
            return;
        }
        hi += f;
    }
    e.encode(hi, scale, scale);
    m.escape1(c);
}

fn encode2(m: &mut Model, e: &mut Enc, sym: u8) {
    let c = m.min_ctx;
    let diff = m.ns(c) - m.num_masked;
    let (ps, hi) = m.unmasked(c).unwrap();
    let see = m.esc_freq(c, diff);
    m.coder.scale += hi;
    let scale = m.coder.scale;
    let mut acc = 0;
    for &s in &ps {
        let f = m.freq(s);
        if m.sym(s) == sym {
            e.encode(acc, acc + f, scale);
            m.found2(c, s, see);
            return;
        }
        acc += f;
    }
    e.encode(hi, scale, scale);
    m.escape2(c, &ps, see);
}

/// Codes `sym` as `decode_char` reads it, the model taught it the same way.
fn encode_char(m: &mut Model, e: &mut Enc, sym: u8) {
    if m.ns(m.min_ctx) != 1 {
        encode1(m, e, sym);
    } else {
        encode_bin(m, e, sym);
    }
    while m.found == 0 {
        loop {
            m.order_fall += 1;
            m.min_ctx = m.suffix(m.min_ctx);
            assert!(m.valid(m.min_ctx), "the root holds every byte");
            if m.ns(m.min_ctx) != m.num_masked {
                break;
            }
        }
        encode2(m, e, sym);
    }
    let succ = m.succ(m.found);
    if m.order_fall == 0 && succ > m.a.text {
        m.min_ctx = succ;
        m.max_ctx = succ;
    } else {
        m.update_model();
        if m.esc_count == 0 {
            m.clear_mask();
        }
    }
}

/// `data` coded by a model of `size` bytes and `order`; how many times it started again.
fn encode(m: &mut Model, data: &[u8]) -> (Vec<u8>, usize) {
    let (mut e, mut restarts) = (Enc::new(), 0);
    for &b in data {
        let text = m.a.text;
        encode_char(m, &mut e, b);
        restarts += usize::from(m.a.text < text);
    }
    (e.finish(), restarts)
}

fn round_trip(data: &[u8], size: u32, order: u32) -> usize {
    let mut m = Model::new(size);
    m.start(order);
    let (packed, restarts) = encode(&mut m, data);
    let mut d = Model::new(size);
    d.start(order);
    let mut bits = Bits::new(packed);
    d.init_coder(&mut bits);
    let got: Vec<u8> = (0..data.len())
        .map(|_| d.decode_char(&mut bits).unwrap())
        .collect();
    assert!(got == data, "the bytes differ from those coded");
    restarts
}

fn noise(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (x >> 16) as u8
        })
        .collect()
}

fn text(n: usize) -> Vec<u8> {
    b"PPMd codes each byte under the longest context that has seen it; "
        .iter()
        .cycle()
        .take(n)
        .copied()
        .collect()
}

#[test]
fn text_and_noise_round_trip_at_low_and_high_orders() {
    let data = [text(6000), noise(3000, 9), text(3000)].concat();
    for order in [2, 6, 16, 64] {
        round_trip(&data, 1 << 20, order);
    }
}

#[test]
fn a_model_whose_memory_fills_starts_again_and_still_round_trips() {
    let data = [noise(6000, 4), text(8000), noise(6000, 5)].concat();
    assert!(round_trip(&data, 64 << 10, 8) > 0);
}

/// A PPMd block for a RAR 4 stream: its flags (a new model of `order`, 1 MiB, escape byte
/// `esc`), then `chars` coded, then the coder's last bytes.
fn ppm_block(m: &mut Model, flags: Option<(u32, u8)>, chars: &[u8]) -> Vec<u8> {
    let mut out = match flags {
        Some((order, esc)) => {
            m.start(order);
            vec![0x80 | 0x40 | 0x20 | (order - 1) as u8, 0, esc]
        }
        None => vec![0x80],
    };
    out.extend(encode(m, chars).0);
    out
}

fn unpack(u: &mut Unpacker, packed: &[u8], size: u64) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut input = packed;
    u.file(&mut input, size, &mut |b: &[u8]| {
        out.extend_from_slice(b);
        Ok(())
    })?;
    Ok(out)
}

#[test]
fn a_ppmd_block_says_its_escapes_matches_runs_and_end() {
    let esc = 0xab;
    let mut m = Model::new(1 << 20);
    let mut chars = b"abc".to_vec();
    chars.extend([esc, 1]);
    chars.extend(b"abcabc");
    chars.extend([esc, 5, 3]);
    chars.extend([esc, 4, 0, 0, 5, 0]);
    chars.extend([esc, 2]);
    let packed = ppm_block(&mut m, Some((8, esc)), &chars);
    let mut want = b"abc".to_vec();
    want.push(esc);
    want.extend(b"abcabc");
    for (len, dist) in [(7, 1), (32, 7)] {
        for _ in 0..len {
            want.push(want[want.len() - dist]);
        }
    }
    let got = unpack(&mut Unpacker::new(1 << 20), &packed, want.len() as u64);
    assert_eq!(got.unwrap(), want);
}

#[test]
fn a_stream_switches_between_lz_and_ppmd_both_ways() {
    // PPMd, then its escape and 0: LZ tables follow at the next byte.
    let mut m = Model::new(1 << 20);
    let mut packed = ppm_block(&mut m, Some((6, 2)), b"from ppmd \x02\x00");
    let lz = [Token::Lit(b'l'), Token::Lit(b'z')];
    let mut w = Writer::new();
    w.tables(lengths_for(&lz), false);
    for t in &lz {
        w.token(t);
    }
    w.end(false);
    packed.extend(w.bytes());
    let got = unpack(&mut Unpacker::new(1 << 20), &packed, 12).unwrap();
    assert_eq!(got, b"from ppmd lz");
    // LZ, then the end of its block and new "tables" that are PPMd.
    let lz = [Token::Lit(b'x')];
    let mut w = Writer::new();
    w.tables(lengths_for(&lz), false);
    w.token(&lz[0]);
    let mut m = Model::new(1 << 20);
    w.switch(&ppm_block(&mut m, Some((4, 2)), b"yz\x02\x02"));
    let got = unpack(&mut Unpacker::new(1 << 20), &w.bytes(), 3).unwrap();
    assert_eq!(got, b"xyz");
}

#[test]
fn a_solid_ppmd_stream_carries_its_model_into_the_next_file() {
    let mut m = Model::new(1 << 20);
    let one = ppm_block(&mut m, Some((8, 2)), b"one file, then another file\x02\x02");
    let two = ppm_block(&mut m, None, b"another file, and one more\x02\x02");
    let mut u = Unpacker::new(1 << 20);
    assert_eq!(
        unpack(&mut u, &one, 27).unwrap(),
        b"one file, then another file"
    );
    u.solid(1 << 20).unwrap();
    assert_eq!(
        unpack(&mut u, &two, 26).unwrap(),
        b"another file, and one more"
    );
    // Alone, the second file carries on a model that is not there.
    assert!(unpack(&mut Unpacker::new(1 << 20), &two, 26).is_err());
}

#[test]
fn damaged_ppmd_data_is_an_error_not_a_panic() {
    for seed in 0..30 {
        let mut packed = vec![0x80 | 0x20 | 7, 0];
        packed.extend(noise(300, seed));
        let _ = unpack(&mut Unpacker::new(1 << 20), &packed, 2000);
        let mut d = Model::new(64 << 10);
        d.start(16);
        let mut bits = Bits::new(noise(500, seed + 100));
        d.init_coder(&mut bits);
        for _ in 0..2000 {
            if d.decode_char(&mut bits).is_none() {
                break;
            }
        }
    }
    // An order of one is no model.
    let packed = [0x80 | 0x20, 0, 0, 0, 0, 0];
    assert!(unpack(&mut Unpacker::new(1 << 20), &packed, 1).is_err());
}
