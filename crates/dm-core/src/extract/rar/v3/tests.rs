// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The RAR 4 decompressor against streams built by the tests' own encoder (`writer.rs`):
//! literals, long and short matches, distances of every range with their low bits and the
//! repeat of them, repeated distances, tables given as changes, new tables inside a file, a
//! solid stream across files, the standard filters, and damaged streams. Each output is
//! compared with a plain model of what the codes mean. None of it is an archive made by
//! RAR's own program.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::super::filters;
use super::super::writer::BitWriter;
use super::writer::{greedy, lengths_for, number, pack, program, Token, Writer};
use super::*;

/// What `tokens` unpack to, by a model kept apart from the decoder; `before` is what the
/// stream holds already. Filters are not the model's.
fn model(before: &[u8], tokens: &[Token]) -> Vec<u8> {
    let mut out = before.to_vec();
    let (mut old, mut last) = ([0u64; 4], 0u64);
    let copy = |out: &mut Vec<u8>, len: u64, dist: u64| {
        for _ in 0..len {
            out.push(out[out.len() - dist as usize]);
        }
    };
    for t in tokens {
        match *t {
            Token::Lit(b) => out.push(b),
            Token::Match(len, dist) | Token::MatchLowRepeat(len, dist) => {
                old = [dist, old[0], old[1], old[2]];
                last = len;
                copy(&mut out, len, dist);
            }
            Token::Short(dist) => {
                old = [dist, old[0], old[1], old[2]];
                last = 2;
                copy(&mut out, 2, dist);
            }
            Token::Repeat(n, len) => {
                let dist = old[n];
                let mut rest = old.to_vec();
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
        &mut Unpacker::new(4 << 20),
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
fn literals_short_matches_and_a_match_that_overlaps_itself() {
    let mut t = lits(b"ab");
    t.push(Token::Match(7, 2));
    assert_eq!(round_trip(&t), b"ababababa");
    t.extend([Token::Short(1), Token::Short(9), Token::Short(4)]);
    assert_eq!(round_trip(&t), b"ababababaaaabaa");
}

#[test]
fn repeated_distances_and_the_last_match_again() {
    let mut t = lits(b"abcd");
    t.extend([
        Token::Match(3, 4),
        Token::Last,
        Token::Repeat(0, 2),
        Token::Short(1),
        Token::Repeat(1, 3),
        Token::Repeat(1, 2),
    ]);
    assert_eq!(round_trip(&t), b"abcdabcdabcdddcdddd");
}

#[test]
fn distances_of_every_range_their_low_bits_and_the_repeat_of_them() {
    let mut t = lits(&noise(0x5_0000, 7));
    for (len, dist) in [
        (20, 0x4_8000),
        (10, 0x3000),
        (8, 0x150),
        (6, 40),
        (5, 20),
        (4, 100),
        (3, 3),
        (255, 0x4_1234),
        (257, 0x4_0000),
    ] {
        t.push(Token::Match(len, dist));
    }
    // Lowest four bits 0, then the repeat code: the next fifteen long distances take them
    // with no code of their own, and the one after has its own again.
    t.push(Token::Match(9, 0x3001));
    t.push(Token::MatchLowRepeat(7, 0x1001));
    for k in 1..=15 {
        t.push(Token::Match(4, 0x100 * k + 1));
    }
    t.extend([
        Token::Match(4, 0x777),
        Token::Short(200),
        Token::Repeat(3, 100),
    ]);
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
    assert_eq!(round_trip(&greedy(&[], &text)), text);
}

#[test]
fn new_tables_inside_a_file_whole_or_as_changes() {
    let first = greedy(&[], b"one block, then another block");
    let second = lits(b" and more, under tables of its own");
    let all: Vec<Token> = first.iter().chain(&second).cloned().collect();
    let want = model(&[], &all);
    for keep in [false, true] {
        let mut w = Writer::new();
        w.tables(lengths_for(&first), false);
        for t in &first {
            w.token(t);
        }
        w.new_tables(lengths_for(&second), keep);
        for t in &second {
            w.token(t);
        }
        w.end(false);
        let got = unpack(&mut Unpacker::new(1 << 20), &w.bytes(), want.len() as u64);
        assert_eq!(got.unwrap(), want);
    }
}

#[test]
fn a_solid_stream_carries_its_window_and_tables_from_file_to_file() {
    let one = b"solid archives pack many files as one stream".to_vec();
    let two = b"many files as one stream, packed solid".to_vec();
    let three = b"and a third with tables of its own".to_vec();
    let (t1, t2) = (greedy(&[], &one), greedy(&one, &two));
    assert!(t2
        .iter()
        .any(|t| matches!(t, Token::Match(_, d) if *d as usize > 10)));
    let both: Vec<Token> = t1.iter().chain(&t2).cloned().collect();
    let lengths = lengths_for(&both);
    let mut w = Writer::new();
    w.tables(lengths, false);
    for t in &t1 {
        w.token(t);
    }
    w.end(false);
    // The second file has no tables: it takes the first's, and says the third has its own.
    let mut w2 = Writer::new();
    w2.carry(lengths);
    for t in &t2 {
        w2.token(t);
    }
    w2.end(true);
    let p2 = w2.bytes();
    let mut u = Unpacker::new(1 << 20);
    assert_eq!(unpack(&mut u, &w.bytes(), one.len() as u64).unwrap(), one);
    u.solid(1 << 20).unwrap();
    assert_eq!(unpack(&mut u, &p2, two.len() as u64).unwrap(), two);
    u.solid(1 << 20).unwrap();
    let p3 = pack(&greedy(&[one.clone(), two.clone()].concat(), &three));
    assert_eq!(unpack(&mut u, &p3, three.len() as u64).unwrap(), three);
    // Alone, the second file has no tables to decode with.
    assert!(unpack(&mut Unpacker::new(1 << 20), &p2, two.len() as u64).is_err());
}

/// A filter's record: a new filter's number (`Some`), its block's start from here, its
/// length (`Some`: given), registers by number, and a new filter's program.
fn record(
    filter: Option<u32>,
    start: u32,
    len: Option<u32>,
    regs: &[(usize, u32)],
    code: Option<&[u8]>,
) -> Token {
    let (mut w, mut first) = (BitWriter::default(), 0u8);
    if let Some(n) = filter {
        first |= 0x80;
        number(&mut w, n);
    }
    number(&mut w, start);
    if let Some(l) = len {
        first |= 0x20;
        number(&mut w, l);
    }
    if !regs.is_empty() {
        first |= 0x10;
        w.put(regs.iter().fold(0, |m, &(i, _)| m | 1 << i), 7);
        for &(_, v) in regs {
            number(&mut w, v);
        }
    }
    if let Some(p) = code {
        number(&mut w, p.len() as u32);
        for &b in p {
            w.put(u64::from(b), 8);
        }
    }
    Token::Filter(first, w.bytes)
}

fn e8_program() -> Vec<u8> {
    program(53, 0xad57_6887)
}

fn delta_program() -> Vec<u8> {
    program(29, 0x0e06_077d)
}

#[test]
fn standard_programs_are_known_by_their_length_and_crc() {
    assert_eq!(vm::recognise(&e8_program()).unwrap(), vm::Kind::E8);
    assert_eq!(
        vm::recognise(&program(57, 0x3cd7_e57e)).unwrap(),
        vm::Kind::E8E9
    );
    assert_eq!(vm::recognise(&delta_program()).unwrap(), vm::Kind::Delta);
    assert_eq!(
        vm::recognise(&program(149, 0x1c2c_5dc8)).unwrap(),
        vm::Kind::Rgb
    );
    assert_eq!(
        vm::recognise(&program(216, 0xbc85_e701)).unwrap(),
        vm::Kind::Audio
    );
    let itanium = vm::recognise(&program(120, 0x3769_893f)).unwrap_err();
    assert!(itanium.to_string().contains("Itanium"), "{itanium}");
    // The right CRC at another length, or a first byte that is not the XOR: no program.
    let mut wrong = e8_program();
    wrong[0] ^= 1;
    assert!(matches!(vm::recognise(&wrong), Err(Error::Method(_))));
    assert!(vm::recognise(&[1, 2, 3, 0]).is_err());
}

fn with_filters(tokens: &[Token]) -> Result<Vec<u8>, Error> {
    let size = model(&[], tokens).len() as u64;
    unpack(&mut Unpacker::new(1 << 20), &pack(tokens), size)
}

#[test]
fn filters_are_undone_at_their_place_in_the_file() {
    // E8 five bytes into the file: the call's place is counted from the file's start.
    let call = [0xe8, 0x0b, 0x01, 0, 0, 0x90, 0x90, 0x90, 0x90];
    let mut t = lits(b"12345");
    t.push(record(Some(0), 0, Some(9), &[], Some(&e8_program())));
    t.extend(lits(&call));
    let got = with_filters(&t).unwrap();
    assert_eq!(got[5..], [0xe8, 0x05, 0x01, 0, 0, 0x90, 0x90, 0x90, 0x90]);
    // DELTA over six bytes in two channels, one byte after where it is read; then the same
    // filter again, by its last length, with no program.
    let mut t = lits(b"xyz");
    t.push(record(
        Some(0),
        1,
        Some(6),
        &[(0, 2)],
        Some(&delta_program()),
    ));
    t.extend(lits(&[b'w', 246, 246, 246, 255, 255, 255]));
    t.push(record(None, 0, None, &[(0, 2)], None));
    t.extend(lits(&[246, 246, 246, 255, 255, 255, b'q']));
    let got = with_filters(&t).unwrap();
    let once = [10, 1, 20, 2, 30, 3];
    assert_eq!(got, [&b"xyzw"[..], &once, &once, b"q"].concat());
}

#[test]
fn two_filters_on_one_block_are_undone_one_over_the_other() {
    let block = noise(64, 5);
    let mut t = vec![record(Some(0), 0, Some(64), &[], Some(&e8_program()))];
    t.push(record(
        Some(2),
        0,
        Some(64),
        &[(0, 3)],
        Some(&delta_program()),
    ));
    t.extend(lits(&block));
    let (mut want, mut spare) = (block.clone(), Vec::new());
    filters::x86_rar4(&mut want, 0, false);
    filters::undo(filters::DELTA, 3, 0, &mut want, &mut spare);
    assert_eq!(with_filters(&t).unwrap(), want);
}

#[test]
fn unknown_programs_and_itanium_are_refused_in_words() {
    let mut t = lits(b"abc");
    t.push(record(Some(0), 0, Some(3), &[], Some(&[7, 1, 2, 4])));
    t.extend(lits(b"def"));
    let e = with_filters(&t).unwrap_err();
    assert!(e.to_string().contains("none of the standard ones"), "{e}");
    let mut t = lits(b"abc");
    t.push(record(
        Some(0),
        0,
        Some(3),
        &[],
        Some(&program(120, 0x3769_893f)),
    ));
    t.extend(lits(b"def"));
    assert!(with_filters(&t)
        .unwrap_err()
        .to_string()
        .contains("Itanium"));
    // A filter given by a number never defined. Bytes follow it: a code past the file's size
    // is the padding of its last byte, never read as a record.
    let mut t = lits(b"abc");
    t.push(record(Some(5), 0, Some(3), &[], None));
    t.extend(lits(b"def"));
    assert!(with_filters(&t).is_err());
}

#[test]
fn damaged_streams_are_errors_not_panics() {
    // A match before the first byte.
    let mut t = lits(b"a");
    t.push(Token::Match(3, 5));
    assert!(unpack(&mut Unpacker::new(1 << 20), &pack(&t), 4).is_err());
    let good = pack(&lits(b"hello"));
    // Cut short, a size that is not the file's, nothing at all.
    assert!(unpack(&mut Unpacker::new(1 << 20), &good[..good.len() / 2], 5).is_err());
    assert!(unpack(&mut Unpacker::new(1 << 20), &good, 6).is_err());
    assert!(unpack(&mut Unpacker::new(1 << 20), &[], 5).is_err());
    // A size one short of what it holds: the bytes past it are not handed on.
    let short = unpack(&mut Unpacker::new(1 << 20), &good, 4).unwrap();
    assert_eq!(short, b"hell");
    // Bytes that are no stream at all.
    for seed in 0..40 {
        let junk = noise(400, seed);
        let _ = unpack(&mut Unpacker::new(1 << 20), &junk, 1000);
    }
}
