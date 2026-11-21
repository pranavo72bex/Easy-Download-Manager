// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The RAR reader against archives these tests build field by field (`writer.rs`), with
//! their CRCs: stored, packed and solid RAR 5 archives, an old-format one, and each thing dm
//! refuses, through the same `extract` a ZIP goes through. No archive made by RAR's own
//! program is among them (none is in the repository).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::super::{extract, is_archive, Limits};
use super::writer::{greedy, pack, rar4, rar4_files, rar5, rar5_block, vint, File4, File5};
use super::*;
use std::sync::atomic::AtomicBool;

fn run(dir: &Path, name: &str, bytes: &[u8]) -> Result<Extracted, Error> {
    let archive = dir.join(name);
    fs::write(&archive, bytes).unwrap();
    extract(
        &archive,
        &Limits::default(),
        &AtomicBool::new(false),
        None,
        &|_, _| {},
    )
}

fn noise(n: usize) -> Vec<u8> {
    let mut x = 5u32;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (x >> 16) as u8
        })
        .collect()
}

fn text(n: usize) -> Vec<u8> {
    b"a download manager fetches ranges; "
        .iter()
        .cycle()
        .take(n)
        .copied()
        .collect()
}

/// A folder entry.
fn folder(name: &str) -> File5<'_> {
    File5 {
        dir: true,
        crc: None,
        attributes: 0x10,
        ..File5::stored(name, &[])
    }
}

/// `bytes`, refused as `check` says, with nothing made.
fn refused(bytes: &[u8], check: impl Fn(&Error) -> bool) {
    let dir = tempfile::tempdir().unwrap();
    let got = run(dir.path(), "x.rar", bytes);
    match &got {
        Err(e) => assert!(check(e), "{e}"),
        Ok(x) => panic!("extracted {x:?}"),
    }
    assert!(!dir.path().join("x").exists(), "nothing is made");
}

#[test]
fn rar_names_are_offered() {
    assert!(is_rar("a.RAR") && is_rar("x.part1.rar") && is_archive("b.rar"));
    assert!(!is_rar("a.rar.txt") && !is_rar("rar"));
}

#[test]
fn stored_files_and_folders_are_extracted_into_a_folder_of_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let bin: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    let archive = rar5(
        0,
        &[
            folder("docs"),
            File5::stored("docs/a.txt", b"hello, rar\n"),
            File5::stored("b.bin", &bin),
            File5::stored("empty.txt", b""),
        ],
    );
    let got = run(dir.path(), "pack.rar", &archive).unwrap();
    assert_eq!(got.folder, dir.path().join("pack"));
    assert_eq!((got.files, got.bytes), (3, 11 + 3000));
    assert_eq!(
        fs::read(got.folder.join("docs/a.txt")).unwrap(),
        b"hello, rar\n"
    );
    assert_eq!(fs::read(got.folder.join("b.bin")).unwrap(), bin);
    assert_eq!(fs::read(got.folder.join("empty.txt")).unwrap(), b"");
    // Again: beside the first, nothing replaced.
    let again = run(dir.path(), "pack.rar", &archive).unwrap();
    assert_eq!(again.folder, dir.path().join("pack (1)"));
}

#[test]
fn packed_and_solid_files_are_unpacked_and_checked() {
    let dir = tempfile::tempdir().unwrap();
    let one = text(40_000);
    let two: Vec<u8> = text(30_000).into_iter().rev().collect();
    let p1 = pack(&greedy(&[], &one, 1 << 20));
    // The second carries on the first's stream; a stored file between them does not.
    let p2 = pack(&greedy(&one, &two, 1 << 20));
    let archive = rar5(
        0x04,
        &[
            File5::packed("one.txt", &one, &p1, false),
            File5::stored("between.txt", b"stored"),
            File5::packed("two.txt", &two, &p2, true),
        ],
    );
    assert!(p1.len() < one.len() / 4, "it packs");
    let got = run(dir.path(), "solid.rar", &archive).unwrap();
    assert_eq!(fs::read(got.folder.join("one.txt")).unwrap(), one);
    assert_eq!(fs::read(got.folder.join("between.txt")).unwrap(), b"stored");
    assert_eq!(fs::read(got.folder.join("two.txt")).unwrap(), two);
}

#[test]
fn a_file_that_does_not_add_up_fails_and_removes_the_folder() {
    let files = [
        File5::stored("a.txt", b"first"),
        File5::stored("b.txt", &[9; 500]),
    ];
    let mut archive = rar5(0, &files);
    let at = archive.windows(8).rposition(|w| w == [9u8; 8]).unwrap();
    archive[at] = 1;
    refused(&archive, |e| e.to_string().contains("does not add up"));
    // A packed file's bytes changed (literals of eight bits each, so any bit is part of a
    // byte): its CRC fails.
    let one = noise(5000);
    let mut packed = pack(&greedy(&[], &one, 1 << 20));
    let middle = packed.len() / 2;
    packed[middle] ^= 0x20;
    refused(
        &rar5(0, &[File5::packed("one.txt", &one, &packed, false)]),
        |e| e.to_string().contains("does not add up"),
    );
}

#[test]
fn a_damaged_or_cut_short_archive_is_unreadable() {
    let archive = rar5(0, &[File5::stored("a.txt", &[7; 2000])]);
    let mut bad = archive.clone();
    let name = bad.windows(5).position(|w| w == b"a.txt").unwrap();
    bad[name] = b'b';
    refused(&bad, |e| e.to_string().contains("damaged"));
    refused(&archive[..archive.len() - 1000], |e| {
        e.to_string().contains("cut short")
    });
    refused(b"Rar!\x1a\x07\x02\x00 not one", |e| {
        matches!(e, Error::Unreadable(_))
    });
    refused(b"", |e| matches!(e, Error::Unreadable(_)));
}

#[test]
fn entries_that_leave_the_folder_links_and_repeats_are_refused() {
    for bad in [
        "../evil.txt",
        "a/../../evil.txt",
        "/etc/evil.txt",
        "C:\\evil.txt",
    ] {
        refused(&rar5(0, &[File5::stored(bad, b"x")]), |e| {
            matches!(e, Error::Escapes(_))
        });
    }
    // A redirection record (a link), and a Unix mode that is a link.
    let mut link = File5::stored("link", b"");
    link.records
        .push([vint(0x05), vint(0), vint(0), vint(3), b"abc".to_vec()].concat());
    refused(&rar5(0, &[link]), |e| matches!(e, Error::Special(_)));
    let mut unix = File5::stored("link", b"/etc/passwd");
    (unix.host, unix.attributes) = (1, 0o120_777);
    refused(&rar5(0, &[unix]), |e| matches!(e, Error::Special(_)));
    let twice = [File5::stored("A.txt", b"1"), File5::stored("a.TXT", b"2")];
    refused(&rar5(0, &twice), |e| matches!(e, Error::Duplicate(_)));
    let inside = [File5::stored("x", b"1"), File5::stored("x/y", b"2")];
    refused(&rar5(0, &inside), |e| matches!(e, Error::Duplicate(_)));
}

#[test]
fn passwords_parts_and_methods_dm_does_not_unpack_are_refused_in_words() {
    // A file encrypted, and the whole list of files encrypted.
    let mut locked = File5::stored("a.txt", b"x");
    locked.records.push([vint(0x01), vec![0; 20]].concat());
    refused(&rar5(0, &[locked]), |e| matches!(e, Error::Encrypted(_)));
    let mut whole = header::SIGNATURE.to_vec();
    whole.extend(rar5_block(4, &[0, 0, 15], &[], None));
    whole.extend(vec![0u8; 64]);
    refused(&whole, |e| matches!(e, Error::Encrypted(_)));
    // One volume of several.
    refused(&rar5(0x01, &[File5::stored("a", b"x")]), |e| {
        e.to_string().contains("one part of several")
    });
    // A later compression, a file checked by BLAKE2 alone.
    let later = File5 {
        info: (3 << 7) | 1,
        ..File5::stored("a", b"x")
    };
    refused(&rar5(0, &[later]), |e| matches!(e, Error::Method(_)));
    let mut hashed = File5 {
        crc: None,
        ..File5::stored("a", b"x")
    };
    hashed
        .records
        .push([vint(0x02), vint(0), vec![0; 32]].concat());
    refused(
        &rar5(0, &[hashed]),
        |e| matches!(e, Error::Method(m) if m.contains("BLAKE2")),
    );
    // A solid file with nothing before it.
    let one = text(100);
    let p = pack(&greedy(&[], &one, 1 << 20));
    refused(&rar5(0, &[File5::packed("a", &one, &p, true)]), |e| {
        e.to_string().contains("damaged")
    });
}

#[test]
fn an_old_format_archive_has_its_stored_files_extracted_and_older_packing_refused() {
    let dir = tempfile::tempdir().unwrap();
    let archive = rar4(&[
        ("docs\\a.txt", &b"old and stored"[..], 0x30),
        ("b.txt", &b"two"[..], 0x30),
    ]);
    let got = run(dir.path(), "old.rar", &archive).unwrap();
    assert_eq!(
        fs::read(got.folder.join("docs/a.txt")).unwrap(),
        b"old and stored"
    );
    assert_eq!(fs::read(got.folder.join("b.txt")).unwrap(), b"two");
    // RAR 2.x's compression (version 20).
    let older = File4 {
        version: 20,
        method: 0x33,
        ..file4("a.txt", b"packed?", b"packed?", 0)
    };
    refused(
        &rar4_files(&[older]),
        |e| matches!(e, Error::Method(m) if m.contains("RAR 2.x")),
    );
    let mut bad = rar4(&[("a.txt", &b"crc"[..], 0x30)]);
    let at = bad.windows(3).position(|w| w == b"crc").unwrap();
    bad[at] = b'x';
    refused(&bad, |e| e.to_string().contains("does not add up"));
}

fn file4<'a>(name: &'a str, plain: &'a [u8], packed: &'a [u8], flags: u16) -> File4<'a> {
    File4 {
        name,
        plain,
        packed,
        method: 0x33,
        version: 29,
        flags,
    }
}

#[test]
fn a_rar4_archive_has_its_packed_and_solid_files_unpacked_and_checked() {
    use super::v3::writer as w4;
    let one = text(3000);
    let two: Vec<u8> = one.iter().rev().copied().collect();
    let three = noise(500);
    let p1 = w4::pack(&w4::greedy(&[], &one));
    let p2 = w4::pack(&w4::greedy(&one, &two));
    let p3 = w4::pack(&w4::greedy(&[], &three));
    let dir = tempfile::tempdir().unwrap();
    let archive = rar4_files(&[
        file4("one.txt", &one, &p1, 0),
        file4("two.txt", &two, &p2, 0x10),
        file4("three.bin", &three, &p3, 0),
    ]);
    let got = run(dir.path(), "four.rar", &archive).unwrap();
    assert_eq!(got.files, 3);
    assert_eq!(fs::read(got.folder.join("one.txt")).unwrap(), one);
    assert_eq!(fs::read(got.folder.join("two.txt")).unwrap(), two);
    assert_eq!(fs::read(got.folder.join("three.bin")).unwrap(), three);
    // A wrong CRC fails the archive; a solid file first has nothing before it.
    let bad = rar4_files(&[file4("one.txt", &three, &p1, 0)]);
    refused(&bad, |e| e.to_string().contains("does not add up"));
    let alone = rar4_files(&[file4("two.txt", &two, &p2, 0x10)]);
    refused(&alone, |e| e.to_string().contains("damaged"));
}

#[test]
fn a_cancel_stops_and_removes_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("c.rar");
    fs::write(&archive, rar5(0, &[File5::stored("a", b"x")])).unwrap();
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        extract(&archive, &Limits::default(), &cancel, None, &|_, _| {}),
        Err(Error::Cancelled)
    ));
    assert!(!dir.path().join("c").exists());
    // Totals past the limits are refused before anything is written.
    let few = Limits {
        max_bytes: 10,
        ..Limits::default()
    };
    fs::write(&archive, rar5(0, &[File5::stored("a", &[1; 11])])).unwrap();
    let no = AtomicBool::new(false);
    assert!(matches!(
        extract(&archive, &few, &no, None, &|_, _| {}),
        Err(Error::TooBig(_))
    ));
}
