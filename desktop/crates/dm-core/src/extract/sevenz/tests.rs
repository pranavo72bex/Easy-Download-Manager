// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The 7z reader against real archives (made by libarchive's 7z writer), raw LZMA and LZMA2
//! streams (made by Python's lzma module) wrapped in archives these tests write, archives
//! written here by hand, and damaged and cut-short copies of all of them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::path::PathBuf;

/// The fixtures: `tests/fixtures/archives/7z`, made by its `make.py`.
fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/archives/7z")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

// The generator `make.py` uses, repeated so the bytes can be compared.
const WORDS: [&[u8]; 8] = [
    b"alpha ",
    b"beta ",
    b"gamma ",
    b"delta ",
    b"download ",
    b"manager ",
    b"range ",
    b"stream\n",
];

fn step(x: &mut u32) -> u32 {
    *x = (x.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7FFF_FFFF;
    *x >> 16
}

fn text(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    let mut out = Vec::new();
    while out.len() < n {
        out.extend_from_slice(WORDS[(step(&mut x) & 7) as usize]);
    }
    out.truncate(n);
    out
}

fn noise(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..n).map(|_| (step(&mut x) & 0xFF) as u8).collect()
}

fn pattern(n: usize) -> Vec<u8> {
    let mut unit: Vec<u8> = (0..=255u8).collect::<Vec<_>>().repeat(3);
    unit.extend_from_slice(b"dm repeats itself\n");
    unit.iter().copied().cycle().take(n).collect()
}

fn run(dir: &Path, name: &str, bytes: &[u8]) -> Result<Extracted, Error> {
    run_with(
        dir,
        name,
        bytes,
        &Limits::default(),
        &AtomicBool::new(false),
    )
}

fn run_with(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<Extracted, Error> {
    let archive = dir.join(name);
    std::fs::write(&archive, bytes).unwrap();
    super::super::extract(&archive, limits, cancel, None, &|_, _| {})
}

/// The files `make.py` put in each libarchive fixture.
fn check_fixture_tree(out: &Path) {
    assert_eq!(
        std::fs::read(out.join("docs/a.txt")).unwrap(),
        b"hello, 7z\n"
    );
    assert_eq!(
        std::fs::read(out.join("docs/b.txt")).unwrap(),
        text(20000, 1)
    );
    assert_eq!(std::fs::read(out.join("c.bin")).unwrap(), noise(3000, 2));
    assert_eq!(
        std::fs::read(out.join("docs/na\u{ef}ve \u{65e5}\u{672c}.txt")).unwrap(),
        b"unicode\n"
    );
    assert_eq!(std::fs::read(out.join("zero.txt")).unwrap(), b"");
    assert!(out.join("docs/empty").is_dir(), "an empty folder is made");
}

#[test]
fn real_stored_lzma_and_lzma2_archives_are_unpacked_whole() {
    // Stored: a folder per file. LZMA and LZMA2: solid, every file in one stream, the list
    // of files itself packed.
    for name in ["store.7z", "lzma1.7z", "lzma2.7z"] {
        let dir = tempfile::tempdir().unwrap();
        let got = run(dir.path(), name, &fixture(name)).unwrap();
        let stem = name.trim_end_matches(".7z");
        assert_eq!(got.folder, dir.path().join(stem), "{name}");
        assert_eq!(got.files, 5, "{name}: four files with bytes and one empty");
        assert_eq!(got.bytes, 10 + 20000 + 3000 + 8, "{name}");
        check_fixture_tree(&got.folder);
        assert!(dir.path().join(name).exists(), "the archive is kept");
    }
}

#[test]
fn the_libarchive_fixtures_are_what_the_tests_say() {
    // The stored one has a folder for each file; the packed ones one folder for all, and a
    // packed list of files.
    let store = read_fixture("store.7z");
    assert!(store.folders.len() > 1);
    assert!(store.folders.iter().all(|f| f.method == Ok(Method::Copy)));
    let lzma = read_fixture("lzma1.7z");
    assert!(matches!(lzma.folders.as_slice(), [f] if matches!(f.method, Ok(Method::Lzma(_)))));
    let lzma2 = read_fixture("lzma2.7z");
    assert!(matches!(lzma2.folders.as_slice(), [f] if matches!(f.method, Ok(Method::Lzma2(_)))));
    assert_eq!(
        lzma2.folders[0].streams.len(),
        4,
        "solid: four files in one stream"
    );
    let raw = fixture("lzma2.7z");
    let next = u64::from_le_bytes(raw[12..20].try_into().unwrap()) as usize;
    assert_eq!(raw[32 + next], 0x17, "the list of files is itself packed");
}

fn read_fixture(name: &str) -> Archive {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, fixture(name)).unwrap();
    read_archive(&path, 1000).unwrap()
}

#[test]
fn methods_dm_does_not_unpack_are_refused_by_name_before_anything_is_made() {
    for (name, said) in [("bzip2.7z", "BZip2"), ("ppmd.7z", "PPMd")] {
        let dir = tempfile::tempdir().unwrap();
        match run(dir.path(), name, &fixture(name)) {
            Err(Error::Method(m)) => assert_eq!(m, said),
            other => panic!("{name}: {other:?}"),
        }
        assert!(!dir.path().join(name.trim_end_matches(".7z")).exists());
        let text = Error::Method(said.into()).to_string();
        assert!(
            text.contains(said) && text.contains("does not unpack"),
            "{text}"
        );
    }
}

#[test]
fn a_symbolic_link_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        run(dir.path(), "link.7z", &fixture("link.7z")),
        Err(Error::Special(_))
    ));
    assert!(!dir.path().join("link").exists());
}

// ---- Archives written here ----

/// The format's number, written as one byte where it fits and as nine where it does not,
/// so both forms are read.
fn number(out: &mut Vec<u8>, v: u64) {
    if v < 0x80 {
        out.push(v as u8);
    } else {
        out.push(0xFF);
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn bits(out: &mut Vec<u8>, flags: &[bool]) {
    let mut bytes = vec![0u8; flags.len().div_ceil(8)];
    for (i, &f) in flags.iter().enumerate() {
        if f {
            bytes[i / 8] |= 0x80 >> (i % 8);
        }
    }
    out.extend_from_slice(&bytes);
}

/// A folder to write: its packed bytes, its coders (id, properties), its unpacked size, and
/// the sizes of the files in it.
struct TestFolder {
    packed: Vec<u8>,
    coders: Vec<(Vec<u8>, Vec<u8>)>,
    unpacked: Vec<u8>,
    files: Vec<u64>,
}

/// An entry to write: its name, and whether it has bytes (in folder order), is an empty
/// file, or is a folder; and its attributes if any.
#[derive(Clone)]
enum Kind {
    Data,
    Empty,
    Dir,
}

fn copy_folder(files: &[&[u8]]) -> TestFolder {
    let unpacked: Vec<u8> = files.concat();
    TestFolder {
        packed: unpacked.clone(),
        coders: vec![(vec![0x00], vec![])],
        unpacked,
        files: files.iter().map(|f| f.len() as u64).collect(),
    }
}

/// A whole 7z archive: signature header, packed streams, and a plain header listing them.
fn seven(folders: &[TestFolder], entries: &[(&str, Kind)], attributes: Option<&[u32]>) -> Vec<u8> {
    let mut h = vec![0x01, 0x04];
    if !folders.is_empty() {
        // Packed streams.
        h.push(0x06);
        number(&mut h, 0);
        number(&mut h, folders.len() as u64);
        h.push(0x09);
        for f in folders {
            number(&mut h, f.packed.len() as u64);
        }
        h.push(0x00);
        // Folders: a chain is written with bind pairs, each coder reading the next one's
        // output, the last reading the packed stream.
        h.extend_from_slice(&[0x07, 0x0B]);
        number(&mut h, folders.len() as u64);
        h.push(0x00);
        for f in folders {
            number(&mut h, f.coders.len() as u64);
            for (id, props) in &f.coders {
                let flags = id.len() as u8 | if props.is_empty() { 0 } else { 0x20 };
                h.push(flags);
                h.extend_from_slice(id);
                if !props.is_empty() {
                    number(&mut h, props.len() as u64);
                    h.extend_from_slice(props);
                }
            }
            for i in 1..f.coders.len() {
                number(&mut h, i as u64 - 1);
                number(&mut h, i as u64);
            }
        }
        h.push(0x0C);
        for f in folders {
            for _ in &f.coders {
                number(&mut h, f.unpacked.len() as u64);
            }
        }
        h.push(0x00);
        // Files' streams, each with its checksum.
        h.extend_from_slice(&[0x08, 0x0D]);
        for f in folders {
            number(&mut h, f.files.len() as u64);
        }
        h.push(0x09);
        for f in folders {
            for size in f.files.iter().take(f.files.len().saturating_sub(1)) {
                number(&mut h, *size);
            }
        }
        h.extend_from_slice(&[0x0A, 0x01]);
        for f in folders {
            let mut at = 0usize;
            for &size in &f.files {
                let part = &f.unpacked[at..at + size as usize];
                h.extend_from_slice(&crc32fast::hash(part).to_le_bytes());
                at += size as usize;
            }
        }
        h.extend_from_slice(&[0x00, 0x00]);
    } else {
        h.push(0x00);
    }
    // Files.
    h.push(0x05);
    number(&mut h, entries.len() as u64);
    let empty: Vec<bool> = entries
        .iter()
        .map(|(_, k)| !matches!(k, Kind::Data))
        .collect();
    if empty.iter().any(|&e| e) {
        let mut p = Vec::new();
        bits(&mut p, &empty);
        h.push(0x0E);
        number(&mut h, p.len() as u64);
        h.extend_from_slice(&p);
        let files: Vec<bool> = entries
            .iter()
            .filter(|(_, k)| !matches!(k, Kind::Data))
            .map(|(_, k)| matches!(k, Kind::Empty))
            .collect();
        let mut p = Vec::new();
        bits(&mut p, &files);
        h.push(0x0F);
        number(&mut h, p.len() as u64);
        h.extend_from_slice(&p);
    }
    let mut p = vec![0x00];
    for (name, _) in entries {
        for u in name.encode_utf16() {
            p.extend_from_slice(&u.to_le_bytes());
        }
        p.extend_from_slice(&[0, 0]);
    }
    h.push(0x11);
    number(&mut h, p.len() as u64);
    h.extend_from_slice(&p);
    if let Some(attrs) = attributes {
        let mut p = vec![0x01, 0x00];
        for a in attrs {
            p.extend_from_slice(&a.to_le_bytes());
        }
        h.push(0x15);
        number(&mut h, p.len() as u64);
        h.extend_from_slice(&p);
    }
    h.extend_from_slice(&[0x00, 0x00]);
    wrap(folders.iter().flat_map(|f| f.packed.clone()).collect(), &h)
}

/// The signature header, the packed bytes, then the header.
fn wrap(packed: Vec<u8>, header: &[u8]) -> Vec<u8> {
    let mut start = Vec::new();
    start.extend_from_slice(&(packed.len() as u64).to_le_bytes());
    start.extend_from_slice(&(header.len() as u64).to_le_bytes());
    start.extend_from_slice(&crc32fast::hash(header).to_le_bytes());
    let mut out = header::SIGNATURE.to_vec();
    out.extend_from_slice(&[0, 4]);
    out.extend_from_slice(&crc32fast::hash(&start).to_le_bytes());
    out.extend_from_slice(&start);
    out.extend_from_slice(&packed);
    out.extend_from_slice(header);
    out
}

/// Puts the checksums of the signature header and the header right after `bytes` was
/// changed, so a damaged header is read rather than stopped at its checksum.
fn fix_checksums(bytes: &mut [u8]) {
    if bytes.len() < 32 {
        return;
    }
    let next = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
    let size = u64::from_le_bytes(bytes[20..28].try_into().unwrap());
    let at = 32u64.saturating_add(next);
    if let Some(end) = at.checked_add(size) {
        if end <= bytes.len() as u64 {
            let crc = crc32fast::hash(&bytes[at as usize..end as usize]);
            bytes[28..32].copy_from_slice(&crc.to_le_bytes());
        }
    }
    let crc = crc32fast::hash(&bytes[12..32]);
    bytes[8..12].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn a_hand_written_solid_stored_archive_with_folders_and_empty_files() {
    let bytes = seven(
        &[copy_folder(&[b"first", b"second file"])],
        &[
            ("top", Kind::Dir),
            ("top/one.txt", Kind::Data),
            ("top/inner/two.txt", Kind::Data),
            ("nothing", Kind::Empty),
        ],
        None,
    );
    let dir = tempfile::tempdir().unwrap();
    let got = run(dir.path(), "h.7z", &bytes).unwrap();
    assert_eq!((got.files, got.bytes), (3, 16));
    assert_eq!(
        std::fs::read(got.folder.join("top/one.txt")).unwrap(),
        b"first"
    );
    assert_eq!(
        std::fs::read(got.folder.join("top/inner/two.txt")).unwrap(),
        b"second file"
    );
    assert_eq!(std::fs::read(got.folder.join("nothing")).unwrap(), b"");
    // A second time goes beside the first, replacing nothing.
    let again = run(dir.path(), "h.7z", &bytes).unwrap();
    assert_eq!(again.folder, dir.path().join("h (1)"));
}

#[test]
fn an_archive_of_nothing_makes_an_empty_folder() {
    let mut bytes = header::SIGNATURE.to_vec();
    bytes.extend_from_slice(&[0, 4]);
    let start = [0u8; 20];
    bytes.extend_from_slice(&crc32fast::hash(&start).to_le_bytes());
    bytes.extend_from_slice(&start);
    let dir = tempfile::tempdir().unwrap();
    let got = run(dir.path(), "e.7z", &bytes).unwrap();
    assert_eq!((got.files, got.bytes), (0, 0));
    assert!(got.folder.is_dir());
}

#[test]
fn entries_that_leave_the_folder_or_repeat_fail_the_whole_archive_before_any_write() {
    for bad in [
        "../evil.txt",
        "a/../../evil.txt",
        "/etc/evil",
        "C:\\evil",
        "x:stream",
    ] {
        let bytes = seven(
            &[copy_folder(&[b"x", b"y"])],
            &[("fine.txt", Kind::Data), (bad, Kind::Data)],
            None,
        );
        let dir = tempfile::tempdir().unwrap();
        assert!(
            matches!(run(dir.path(), "z.7z", &bytes), Err(Error::Escapes(_))),
            "{bad}"
        );
        assert!(!dir.path().join("z").exists(), "{bad}: nothing is made");
    }
    let twice = seven(
        &[copy_folder(&[b"x", b"y"])],
        &[("A.txt", Kind::Data), ("a.TXT", Kind::Data)],
        None,
    );
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        run(dir.path(), "z.7z", &twice),
        Err(Error::Duplicate(_))
    ));
    let file_and_folder = seven(
        &[copy_folder(&[b"x"])],
        &[("x", Kind::Data), ("x", Kind::Dir)],
        None,
    );
    assert!(run(dir.path(), "f.7z", &file_and_folder).is_err());
    assert!(!dir.path().join("f").exists());
}

#[test]
fn a_file_and_a_folder_of_one_name_in_any_case_are_refused_before_any_write() {
    // On a disk that ignores case these would fail only once the first was written, after
    // the folder was made; they are judged with the rest instead.
    let cases: [&[(&str, Kind)]; 3] = [
        &[("a", Kind::Data), ("A", Kind::Dir)],
        &[("A", Kind::Dir), ("a", Kind::Data)],
        &[("a", Kind::Data), ("A/b", Kind::Empty)],
    ];
    for entries in cases {
        let bytes = seven(&[copy_folder(&[b"x"])], entries, None);
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            run(dir.path(), "c.7z", &bytes),
            Err(Error::Duplicate(_))
        ));
        assert!(!dir.path().join("c").exists(), "nothing is made");
    }
    // A folder named twice, in two cases, is still one folder.
    let fine = seven(
        &[copy_folder(&[b"x"])],
        &[("d", Kind::Dir), ("D", Kind::Dir), ("d/f", Kind::Data)],
        None,
    );
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(run(dir.path(), "f.7z", &fine).unwrap().files, 1);
}

#[test]
fn a_header_listing_more_folders_or_packed_streams_than_entries_allowed_is_refused() {
    // A packed list of files can unpack to 64 MiB of three-byte folders, millions of them;
    // past the entries allowed it is refused before they are held in memory.
    let limit = 10usize;
    let mut folders = vec![0x01, 0x04, 0x07, 0x0B];
    number(&mut folders, limit as u64 + 1);
    folders.push(0x00);
    for _ in 0..=limit {
        folders.extend_from_slice(&[0x01, 0x01, 0x00]);
    }
    let mut packed = vec![0x01, 0x04, 0x06, 0x00];
    number(&mut packed, limit as u64 * 4 + 1);
    let dir = tempfile::tempdir().unwrap();
    for (name, mut h) in [("folders", folders), ("packed", packed)] {
        h.resize(h.len() + 200, 0);
        let path = dir.path().join(format!("{name}.7z"));
        std::fs::write(&path, wrap(Vec::new(), &h)).unwrap();
        assert!(
            matches!(read_archive(&path, limit), Err(Error::TooBig(_))),
            "{name}"
        );
        // Within the limit the same lists are only read as far as they go.
        assert!(!matches!(
            read_archive(&path, limit * 4 + 1),
            Err(Error::TooBig(_))
        ));
    }
}

#[test]
fn links_and_special_files_by_their_attributes_are_refused() {
    // A Unix link (mode 0120777 in the high half), a device, and a Windows reparse point.
    for attr in [
        (0o120777 << 16) | 0x8000,
        (0o020644 << 16) | 0x8000,
        0x400 | 0x20,
    ] {
        let bytes = seven(&[copy_folder(&[b"x"])], &[("l", Kind::Data)], Some(&[attr]));
        let dir = tempfile::tempdir().unwrap();
        assert!(
            matches!(run(dir.path(), "l.7z", &bytes), Err(Error::Special(_))),
            "{attr:x}"
        );
        assert!(!dir.path().join("l").exists());
    }
    // A plain file's mode, and a folder's, pass.
    let fine = seven(
        &[copy_folder(&[b"x"])],
        &[("f", Kind::Data), ("d", Kind::Dir)],
        Some(&[(0o100644 << 16) | 0x8020, (0o040755 << 16) | 0x8010]),
    );
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(run(dir.path(), "ok.7z", &fine).unwrap().files, 1);
}

#[test]
fn filters_chains_and_passwords_are_refused_plainly() {
    let one = |coders: Vec<(Vec<u8>, Vec<u8>)>| {
        let mut f = copy_folder(&[b"payload"]);
        f.coders = coders;
        seven(&[f], &[("p.exe", Kind::Data)], None)
    };
    let dir = tempfile::tempdir().unwrap();
    // BCJ (x86) over LZMA2, as 7-Zip packs a program.
    let bcj = one(vec![
        (vec![0x03, 0x03, 0x01, 0x03], vec![]),
        (vec![0x21], vec![0x10]),
    ]);
    match run(dir.path(), "b.7z", &bcj) {
        Err(Error::Method(m)) => assert_eq!(m, "a BCJ filter"),
        other => panic!("{other:?}"),
    }
    // AES: a password would be needed.
    let aes = one(vec![(vec![0x06, 0xF1, 0x07, 0x01], vec![0x01, 0x02])]);
    assert!(matches!(run(dir.path(), "a.7z", &aes), Err(Error::Encrypted(n)) if n == "p.exe"));
    // AES under LZMA, the way 7-Zip encrypts: still a password.
    let chain = one(vec![
        (vec![0x21], vec![0x10]),
        (vec![0x06, 0xF1, 0x07, 0x01], vec![1]),
    ]);
    assert!(matches!(
        run(dir.path(), "c.7z", &chain),
        Err(Error::Encrypted(_))
    ));
    // Two coders dm reads, chained: not a layout dm unpacks.
    let two = one(vec![(vec![0x00], vec![]), (vec![0x00], vec![])]);
    match run(dir.path(), "t.7z", &two) {
        Err(Error::Method(m)) => assert!(m.contains("chain"), "{m}"),
        other => panic!("{other:?}"),
    }
    // A method no one knows.
    let odd = one(vec![(vec![0x7F, 0x01], vec![])]);
    match run(dir.path(), "o.7z", &odd) {
        Err(Error::Method(m)) => assert!(m.contains("7F01"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        5,
        "only the archives"
    );
}

#[test]
fn one_part_of_several_or_a_cut_archive_says_so() {
    let bytes = seven(
        &[copy_folder(&[&[5u8; 4000]])],
        &[("big", Kind::Data)],
        None,
    );
    // The first part of a split archive: the header (at the end) is not in it.
    let dir = tempfile::tempdir().unwrap();
    match run(dir.path(), "part.7z", &bytes[..2000]) {
        Err(Error::Unreadable(m)) => assert!(m.contains("one part of several"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        run(dir.path(), "n.7z", b"7z but not one"),
        Err(Error::Unreadable(_))
    ));
    assert!(matches!(
        run(dir.path(), "e.7z", b""),
        Err(Error::Unreadable(_))
    ));
}

#[test]
fn a_damaged_file_fails_its_checksum_and_removes_the_folder() {
    let mut bytes = seven(
        &[copy_folder(&[b"good", &[9u8; 3000]])],
        &[("first", Kind::Data), ("second", Kind::Data)],
        None,
    );
    // Flip a byte of the second file: its checksum fails after the first was written.
    bytes[32 + 4 + 1000] ^= 0x55;
    let dir = tempfile::tempdir().unwrap();
    match run(dir.path(), "z.7z", &bytes) {
        Err(Error::Unreadable(m)) => assert!(m.contains("second"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(
        !dir.path().join("z").exists(),
        "a failure leaves no half folder"
    );
}

#[test]
fn totals_past_the_limits_and_a_cancel_are_refused() {
    let bytes = seven(
        &[copy_folder(&[&[1u8; 3000], b"a", b"b"])],
        &[("x", Kind::Data), ("y", Kind::Data), ("z", Kind::Data)],
        None,
    );
    let dir = tempfile::tempdir().unwrap();
    let no = AtomicBool::new(false);
    let few = Limits {
        max_entries: 2,
        ..Limits::default()
    };
    assert!(matches!(
        run_with(dir.path(), "a.7z", &bytes, &few, &no),
        Err(Error::TooBig(_))
    ));
    let small = Limits {
        max_bytes: 1000,
        ..Limits::default()
    };
    assert!(matches!(
        run_with(dir.path(), "b.7z", &bytes, &small, &no),
        Err(Error::TooBig(_))
    ));
    let strict = Limits {
        max_ratio: 2,
        ratio_floor: 100,
        ..Limits::default()
    };
    let lzma2 = fixture("lzma2.7z");
    assert!(matches!(
        run_with(dir.path(), "r.7z", &lzma2, &strict, &no),
        Err(Error::TooBig(_))
    ));
    assert!(matches!(
        run_with(
            dir.path(),
            "c.7z",
            &bytes,
            &Limits::default(),
            &AtomicBool::new(true)
        ),
        Err(Error::Cancelled)
    ));
    for made in ["a", "b", "r", "c"] {
        assert!(!dir.path().join(made).exists(), "{made}");
    }
}

// ---- LZMA and LZMA2, from Python's lzma module ----

/// A folder of one LZMA coder over a raw stream; `props` is lc/lp/pb as their packed byte.
fn lzma_folder(
    raw: Vec<u8>,
    props: u8,
    dict: u32,
    unpacked: Vec<u8>,
    files: Vec<u64>,
) -> TestFolder {
    let mut p = vec![props];
    p.extend_from_slice(&dict.to_le_bytes());
    TestFolder {
        packed: raw,
        coders: vec![(vec![0x03, 0x01, 0x01], p)],
        unpacked,
        files,
    }
}

#[test]
fn raw_lzma_streams_with_an_end_marker_and_unusual_properties_unpack() {
    // text.lzma: lc=3, lp=0, pb=2 (the packed byte 0x5D), a 64 KiB dictionary.
    let want = text(50000, 11);
    let f = lzma_folder(
        fixture("text.lzma"),
        0x5D,
        1 << 16,
        want.clone(),
        vec![50000],
    );
    let bytes = seven(&[f], &[("text.txt", Kind::Data)], None);
    let dir = tempfile::tempdir().unwrap();
    let got = run(dir.path(), "t.7z", &bytes).unwrap();
    assert_eq!(std::fs::read(got.folder.join("text.txt")).unwrap(), want);

    // props.lzma: lc=1, lp=2, pb=1 (1 + 2*9 + 1*45 = 64), a 4 KiB dictionary that wraps,
    // split across three files in one solid folder.
    let mut want = noise(3000, 5);
    want.extend(pattern(20000));
    want.extend(text(8000, 6));
    let f = lzma_folder(
        fixture("props.lzma"),
        64,
        1 << 12,
        want.clone(),
        vec![3000, 20000, 8000],
    );
    let bytes = seven(
        &[f],
        &[("a", Kind::Data), ("b", Kind::Data), ("c", Kind::Data)],
        None,
    );
    let got = run(dir.path(), "p.7z", &bytes).unwrap();
    let mut back = std::fs::read(got.folder.join("a")).unwrap();
    back.extend(std::fs::read(got.folder.join("b")).unwrap());
    back.extend(std::fs::read(got.folder.join("c")).unwrap());
    assert_eq!(back, want);
}

fn mixed() -> Vec<u8> {
    let mut want = noise(65600, 7);
    want.extend(text(60000, 8));
    want.extend(pattern(4_500_000));
    want
}

#[test]
fn a_raw_lzma2_stream_of_stored_and_compressed_chunks_unpacks_in_parts() {
    let want = mixed();
    let raw = fixture("mixed.lzma2");
    // The stream holds a stored chunk and several compressed ones, at least one going on
    // from the last without a reset.
    assert!(raw.first() == Some(&0x01), "starts with a stored chunk");
    // 64 KiB: (2 | 0) << (8 / 2 + 11).
    let sizes = vec![1000, 64_600, 200_000, want.len() as u64 - 265_600];
    let f = TestFolder {
        packed: raw,
        coders: vec![(vec![0x21], vec![8])],
        unpacked: want.clone(),
        files: sizes,
    };
    let entries = [
        ("1", Kind::Data),
        ("2", Kind::Data),
        ("3", Kind::Data),
        ("4", Kind::Data),
    ];
    let bytes = seven(&[f], &entries, None);
    let dir = tempfile::tempdir().unwrap();
    let got = run(dir.path(), "m.7z", &bytes).unwrap();
    let mut back = Vec::new();
    for (name, _) in &entries {
        back.extend(std::fs::read(got.folder.join(name)).unwrap());
    }
    assert_eq!(back.len(), want.len());
    assert!(back == want, "the bytes match");
}

#[test]
fn the_decoders_read_in_any_size_of_piece() {
    let want = text(50000, 11);
    for piece in [1usize, 7, 4096, 1 << 20] {
        let mut r = lzma::LzmaReader::new(
            Box::new(std::io::Cursor::new(fixture("text.lzma"))),
            &[0x5D, 0, 0, 1, 0],
            want.len() as u64,
        )
        .unwrap();
        let mut out = Vec::new();
        let mut buf = vec![0u8; piece];
        loop {
            let n = r.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        assert!(out == want, "pieces of {piece}");
    }
    let want = mixed();
    let mut r = lzma::Lzma2Reader::new(
        Box::new(std::io::Cursor::new(fixture("mixed.lzma2"))),
        8,
        want.len() as u64,
    )
    .unwrap();
    let mut out = Vec::new();
    let mut buf = vec![0u8; 333];
    loop {
        let n = r.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    assert!(out == want);
}

#[test]
fn numbers_are_read_in_every_length() {
    let cases: [(&[u8], u64); 6] = [
        (&[0x05], 5),
        (&[0x7F], 127),
        (&[0x80, 0x80], 128),
        (&[0xBF, 0xFF], 0x3FFF),
        (&[0xC0, 0x00, 0x40], 0x4000),
        (&[0xFF, 1, 2, 3, 4, 5, 6, 7, 8], 0x0807_0605_0403_0201),
    ];
    for (bytes, want) in cases {
        assert_eq!(
            header::Cursor::new(bytes).number().unwrap(),
            want,
            "{bytes:x?}"
        );
    }
    assert!(header::Cursor::new(&[0x80]).number().is_err());
}

#[test]
fn only_7z_names_are_offered() {
    assert!(is_7z("a.7Z") && is_7z("x.y.7z"));
    assert!(!is_7z("a.7z.001") && !is_7z("a.zip") && !is_7z("7z"));
    assert!(super::super::is_archive("pack.7z"));
}

// ---- Damage: every one an error, never a panic ----

/// A small generator for the damage, the same every run.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// The archives the damage is done to.
fn victims() -> Vec<(&'static str, Vec<u8>)> {
    let mut want = noise(3000, 5);
    want.extend(pattern(20000));
    want.extend(text(8000, 6));
    let props = seven(
        &[lzma_folder(
            fixture("props.lzma"),
            64,
            1 << 12,
            want,
            vec![3000, 20000, 8000],
        )],
        &[("a", Kind::Data), ("b", Kind::Data), ("c", Kind::Data)],
        None,
    );
    vec![
        ("store.7z", fixture("store.7z")),
        ("lzma1.7z", fixture("lzma1.7z")),
        ("lzma2.7z", fixture("lzma2.7z")),
        ("props.7z", props),
    ]
}

#[test]
fn cut_short_archives_are_errors_not_panics() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in victims() {
        let mut cuts: Vec<usize> = (0..64).collect();
        cuts.extend((1..40).map(|i| bytes.len() * i / 40));
        for (n, cut) in cuts.into_iter().enumerate() {
            let short = &bytes[..cut.min(bytes.len() - 1)];
            let file = format!("{n}-{name}");
            assert!(
                run(dir.path(), &file, short).is_err(),
                "{name} cut at {cut}"
            );
            assert!(!dir.path().join(file.trim_end_matches(".7z")).exists());
        }
    }
}

#[test]
fn damaged_archives_are_errors_or_whole_never_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut dice = Dice(0x9E37_79B9_7F4A_7C15);
    for (name, bytes) in victims() {
        for n in 0..150 {
            let mut bad = bytes.clone();
            for _ in 0..1 + dice.below(4) {
                let at = dice.below(bad.len());
                bad[at] ^= 1 << dice.below(8);
            }
            // Half the time the checksums are put right, so a damaged list of files is read
            // rather than stopped at its checksum.
            if n % 2 == 0 {
                fix_checksums(&mut bad);
            }
            let file = format!("{n}-{name}");
            if run(dir.path(), &file, &bad).is_err() {
                assert!(
                    !dir.path().join(file.trim_end_matches(".7z")).exists(),
                    "{file}: a failure leaves nothing"
                );
            }
        }
    }
}

#[test]
fn damaged_headers_are_errors_never_a_panic() {
    // The list of files alone, damaged and parsed again and again.
    let raw = seven(
        &[copy_folder(&[b"first", b"second"])],
        &[
            ("d", Kind::Dir),
            ("d/a", Kind::Data),
            ("d/b", Kind::Data),
            ("e", Kind::Empty),
        ],
        Some(&[0x10, 0x20, 0x20, 0x20]),
    );
    let header_at = 32 + 11;
    let dir = tempfile::tempdir().unwrap();
    let mut dice = Dice(42);
    for n in 0..3000 {
        let mut bad = raw.clone();
        match n % 3 {
            0 => {
                let at = header_at + dice.below(raw.len() - header_at);
                bad[at] = dice.next() as u8;
            }
            1 => {
                let at = header_at + dice.below(raw.len() - header_at);
                bad[at] ^= 1 << dice.below(8);
            }
            _ => {
                // A run of large numbers where counts are expected.
                let at = header_at + dice.below(raw.len() - header_at);
                for b in bad.iter_mut().skip(at).take(3) {
                    *b = 0xFF;
                }
            }
        }
        fix_checksums(&mut bad);
        let path = dir.path().join("h.7z");
        std::fs::write(&path, &bad).unwrap();
        let _ = read_archive(&path, 1000);
    }
}

#[test]
fn damaged_lzma_and_lzma2_streams_are_errors_never_a_panic() {
    let mut dice = Dice(7);
    let sources: [(&str, usize); 2] = [("text.lzma", 50000), ("mixed.lzma2", mixed().len())];
    for (name, size) in sources {
        let good = fixture(name);
        for n in 0..300 {
            let mut bad = good.clone();
            match n % 3 {
                0 => bad.truncate(dice.below(good.len())),
                _ => {
                    for _ in 0..1 + dice.below(3) {
                        let at = dice.below(bad.len());
                        bad[at] ^= 1 << dice.below(8);
                    }
                }
            }
            let input: Box<dyn Read> = Box::new(std::io::Cursor::new(bad));
            let reader: std::io::Result<Box<dyn Read>> = if name.ends_with("lzma2") {
                lzma::Lzma2Reader::new(input, 8, size as u64).map(|r| Box::new(r) as Box<dyn Read>)
            } else {
                lzma::LzmaReader::new(input, &[0x5D, 0, 0, 1, 0], size as u64)
                    .map(|r| Box::new(r) as Box<dyn Read>)
            };
            let Ok(reader) = reader else { continue };
            let mut out = Vec::new();
            // Damage is an error or wrong bytes (the checksum's to catch), never more bytes
            // than the size said. The first 400 KB are enough to meet the damage in most.
            let _ = reader.take(400_000).read_to_end(&mut out);
            assert!(
                out.len() <= size.min(400_000),
                "{name}: {} bytes",
                out.len()
            );
        }
    }
    // Properties out of range are refused.
    let empty = || Box::new(std::io::empty()) as Box<dyn Read>;
    assert!(lzma::LzmaReader::new(empty(), &[225, 0, 0, 1, 0], 10).is_err());
    assert!(lzma::LzmaReader::new(empty(), &[0x5D, 0, 0], 10).is_err());
    assert!(lzma::Lzma2Reader::new(empty(), 41, 10).is_err());
}

#[test]
fn a_stream_that_ends_before_its_stated_size_is_an_error() {
    // text.lzma holds 50,000 bytes and then its end marker; a header saying one more is wrong.
    let mut r = lzma::LzmaReader::new(
        Box::new(std::io::Cursor::new(fixture("text.lzma"))),
        &[0x5D, 0, 0, 1, 0],
        50_001,
    )
    .unwrap();
    let mut out = Vec::new();
    assert!(r.read_to_end(&mut out).is_err());
    assert!(out.len() <= 50_000);
    // The same for LZMA2: its end byte comes before the bytes the header promised.
    let mut r = lzma::Lzma2Reader::new(
        Box::new(std::io::Cursor::new(fixture("mixed.lzma2"))),
        8,
        mixed().len() as u64 + 1,
    )
    .unwrap();
    assert!(r.read_to_end(&mut Vec::new()).is_err());
}
