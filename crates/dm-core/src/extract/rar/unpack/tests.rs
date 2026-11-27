// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The RAR 5 decompressor against streams built by the tests' own encoder (`writer.rs`):
//! literals, matches near and far, repeated distances, tables kept from one block to the
//! next, filters at their places, a solid stream across two files, a window that wraps, and
//! damaged streams. Each output is compared with a plain model of what the codes mean.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::super::writer::{block, greedy, lengths_for, pack, Token};
use super::*;

/// What `tokens` unpack to, by a model kept apart from the decoder: `before` is what the
/// stream holds already. Filters are not the model's.
fn model(before: &[u8], tokens: &[Token]) -> Vec<u8> {
    let mut out = before.to_vec();
    let (mut old, mut last) = ([0u64; 4], 0u64);
    let copy = |out: &mut Vec<u8>, len: u64, dist: u64| {
        for _ in 0..len {
            out.push(out[out.len() - dist as usize]);
        }
    };
    for &t in tokens {
        match t {
            Token::Lit(b) => out.push(b),
            Token::Match(len, dist) => {
                old = [dist, old[0], old[1], old[2]];
                last = len;
                copy(&mut out, len, dist);
            }
            Token::Repeat(n, len) => {
                let dist = old[n];
                let mut rest: Vec<u64> = old.to_vec();
                rest.remove(n);
                old = [dist, rest[0], rest[1], rest[2]];
                last = len;
                copy(&mut out, len, dist);
            }
            Token::Last if last > 0 => copy(&mut out, last, old[0]),
            Token::Last | Token::Filter(..) => {}
        }
    }
    out.split_off(before.len())
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

fn round_trip(tokens: &[Token]) -> Vec<u8> {
    let want = model(&[], tokens);
    let got = unpack(
        &mut Unpacker::new(128 << 10).unwrap(),
        &pack(tokens),
        want.len() as u64,
    )
    .unwrap();
    assert_eq!(got.len(), want.len());
    assert!(got == want, "the output differs from the model");
    got
}

fn lits(bytes: &[u8]) -> Vec<Token> {
    bytes.iter().map(|&b| Token::Lit(b)).collect()
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

#[test]
fn literals_and_a_match_that_overlaps_itself() {
    let mut t = lits(b"ab");
    t.push(Token::Match(7, 2));
    assert_eq!(round_trip(&t), b"ababababa");
}

#[test]
fn repeated_distances_and_the_last_match_again() {
    let mut t = lits(b"abcd");
    t.extend([
        Token::Match(3, 4),
        Token::Last,
        Token::Repeat(0, 2),
        Token::Match(2, 1),
        Token::Repeat(1, 3),
        Token::Repeat(1, 2),
    ]);
    assert_eq!(round_trip(&t), b"abcdabcdabcdddcdddd");
}

#[test]
fn distances_of_every_range_and_their_longer_lengths() {
    // Far enough back for each of the length's three steps, and each way of writing a
    // distance's bits: none, a few, low bits alone, high and low bits.
    let mut t = lits(&noise(0x50000, 7));
    for (len, dist) in [
        (20, 0x48000),
        (10, 0x3000),
        (8, 0x150),
        (6, 40),
        (5, 20),
        (4, 100),
        (3, 3),
        (4000, 0x4_1234),
    ] {
        t.push(Token::Match(len, dist));
    }
    round_trip(&t);
}

#[test]
fn a_greedy_stream_of_text_unpacks_to_its_text() {
    let text: Vec<u8> = b"the range is fetched again when a mirror lags; "
        .iter()
        .cycle()
        .take(50_000)
        .copied()
        .chain(noise(5000, 3))
        .collect();
    assert_eq!(round_trip(&greedy(&[], &text, 1 << 20)), text);
}

#[test]
fn a_block_without_tables_keeps_the_last_ones() {
    let first = greedy(&[], b"one block, then another block", 1 << 16);
    let second = lits(b" and more");
    let all: Vec<Token> = first.iter().chain(&second).copied().collect();
    let lengths = lengths_for(&all);
    let mut packed = block(&first, &lengths, true, false);
    packed.extend(block(&second, &lengths, false, true));
    let want = model(&[], &all);
    let got = unpack(
        &mut Unpacker::new(1 << 20).unwrap(),
        &packed,
        want.len() as u64,
    )
    .unwrap();
    assert_eq!(got, b"one block, then another block and more");
    // A first block with no tables has nothing to decode with.
    let bare = block(&second, &lengths, false, true);
    assert!(unpack(&mut Unpacker::new(1 << 20).unwrap(), &bare, 9).is_err());
}

#[test]
fn a_solid_file_carries_on_the_stream_before_it() {
    let one = b"solid archives pack many files as one stream".to_vec();
    let two = b"many files as one stream, packed solid".to_vec();
    let first = greedy(&[], &one, 1 << 20);
    let second = greedy(&one, &two, 1 << 20);
    assert!(
        second
            .iter()
            .any(|t| matches!(t, Token::Match(_, d) if *d as usize > 10)),
        "the second file reaches back into the first"
    );
    let all: Vec<Token> = first.iter().chain(&second).copied().collect();
    let lengths = lengths_for(&all);
    let mut u = Unpacker::new(1 << 20).unwrap();
    let a = block(&first, &lengths, true, true);
    assert_eq!(unpack(&mut u, &a, one.len() as u64).unwrap(), one);
    u.solid(1 << 20).unwrap();
    let b = block(&second, &lengths, false, true);
    assert_eq!(unpack(&mut u, &b, two.len() as u64).unwrap(), two);
    // Alone, the second file reaches before its first byte, with no tables either.
    assert!(unpack(&mut Unpacker::new(1 << 20).unwrap(), &b, two.len() as u64).is_err());
}

#[test]
fn filters_are_undone_at_their_place_in_the_file() {
    // DELTA over six bytes, one byte after where it is read.
    let mut t = lits(b"xyz");
    t.push(Token::Filter(1, 6, filters::DELTA, 2));
    t.extend(lits(&[b'w', 246, 246, 246, 255, 255, 255, b'q']));
    assert_eq!(
        unpack(&mut Unpacker::new(1 << 20).unwrap(), &pack(&t), 11).unwrap(),
        [b'x', b'y', b'z', b'w', 10, 1, 20, 2, 30, 3, b'q']
    );
    // E8 five bytes into the file: the call's place is counted from the file's start.
    let mut t = lits(b"12345");
    t.push(Token::Filter(0, 9, filters::E8, 0));
    t.extend(lits(&[0xe8, 0x0b, 0x01, 0, 0, 0x90, 0x90, 0x90, 0x90]));
    let got = unpack(&mut Unpacker::new(1 << 20).unwrap(), &pack(&t), 14).unwrap();
    assert_eq!(got[5..], [0xe8, 0x05, 0x01, 0, 0, 0x90, 0x90, 0x90, 0x90]);
    // A filter whose block runs past the file's end is damage.
    let mut t = lits(b"ab");
    t.push(Token::Filter(0, 10, filters::E8, 0));
    t.extend(lits(b"cd"));
    assert!(unpack(&mut Unpacker::new(1 << 20).unwrap(), &pack(&t), 4).is_err());
}

#[test]
fn a_stream_longer_than_its_window_wraps_and_still_matches() {
    // 9 MiB through an 8 MiB window, then a match 3 MB back.
    let mut t = lits(&noise(5000, 11));
    t.extend(std::iter::repeat_n(Token::Match(4000, 4999), 2400));
    t.push(Token::Match(500, 3_000_000));
    let out = round_trip(&t);
    assert!(out.len() > 9 << 20);
}

#[test]
fn damaged_streams_are_errors_not_panics() {
    // A match before the first byte.
    let mut t = lits(b"a");
    t.push(Token::Match(3, 5));
    assert!(unpack(&mut Unpacker::new(1 << 20).unwrap(), &pack(&t), 4).is_err());
    let good = pack(&lits(b"hello"));
    // The block's check byte, its size cut short, a size that is not the file's.
    let mut bad = good.clone();
    bad[1] ^= 1;
    assert!(unpack(&mut Unpacker::new(1 << 20).unwrap(), &bad, 5).is_err());
    assert!(unpack(
        &mut Unpacker::new(1 << 20).unwrap(),
        &good[..good.len() - 1],
        5
    )
    .is_err());
    assert!(unpack(&mut Unpacker::new(1 << 20).unwrap(), &good, 6).is_err());
    assert!(unpack(&mut Unpacker::new(1 << 20).unwrap(), &[], 5).is_err());
    // Bytes that are no stream at all.
    for seed in 0..20 {
        let junk = noise(300, seed);
        let _ = unpack(&mut Unpacker::new(1 << 20).unwrap(), &junk, 1000);
    }
}

/// A RAR 5 file asking for a dictionary over the largest dm unpacks with (the format allows
/// 4 GiB) is refused before any window is made, and so is a solid file after it (gaps row H35e).
#[test]
fn a_file_asking_for_a_larger_dictionary_than_dm_allows_is_refused() {
    let most = crate::extract::MOST_DICTIONARY;
    assert!(Unpacker::new(most).is_ok());
    for asked in [most + 1, 2 << 30, 4 << 30] {
        let said = Unpacker::new(asked).err().expect("refused").to_string();
        assert!(said.contains("dictionary is over 1.5 GiB"), "{said}");
    }
    let mut u = Unpacker::new(1 << 20).unwrap();
    assert!(u.solid(4 << 30).is_err());
}
