// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A tar of `entries` (name, kind, bytes), as GNU tar writes one: ustar headers, the data
/// padded to blocks, two zero blocks at the end.
fn tar(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, kind, data) in entries {
        let mut h = [0u8; BLOCK];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        h[136..147].copy_from_slice(b"00000000000");
        h[156] = *kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(BLOCK) * BLOCK, 0);
    }
    out.extend_from_slice(&[0u8; BLOCK * 2]);
    out
}

fn unpack(dir: &Path, name: &str, bytes: &[u8]) -> Result<Extracted, Error> {
    let archive = dir.join(name);
    std::fs::write(&archive, bytes).unwrap();
    extract_into(
        &archive,
        &dir.join("out"),
        &Limits::default(),
        &AtomicBool::new(false),
        None,
        &|_, _| {},
    )
}

#[test]
fn a_tar_and_a_tar_gz_are_unpacked_into_their_folder() {
    let bytes = tar(&[
        ("docs/", b'5', b""),
        ("docs/a.txt", b'0', b"hello"),
        ("b.bin", b'0', &[7u8; 1000]),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let made = unpack(dir.path(), "x.tar", &bytes).unwrap();
    assert_eq!(made.files, 2);
    assert_eq!(
        std::fs::read(dir.path().join("out/docs/a.txt")).unwrap(),
        b"hello"
    );
    assert_eq!(
        std::fs::read(dir.path().join("out/b.bin")).unwrap(),
        vec![7u8; 1000]
    );
    // Gzipped, the same.
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&bytes).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let made = unpack(dir.path(), "x.tgz", &gz.finish().unwrap()).unwrap();
    assert_eq!(made.files, 2);
}

#[test]
fn a_tar_that_would_leave_its_folder_or_holds_a_link_is_refused_whole() {
    let dir = tempfile::tempdir().unwrap();
    let escaping = tar(&[("ok.txt", b'0', b"x"), ("../evil", b'0', b"x")]);
    assert!(matches!(
        unpack(dir.path(), "e.tar", &escaping),
        Err(Error::Escapes(_))
    ));
    assert!(!dir.path().join("out").exists(), "nothing written");
    let linked = tar(&[("link", b'2', b"")]);
    assert!(matches!(
        unpack(dir.path(), "l.tar", &linked),
        Err(Error::Special(_))
    ));
    let twice = tar(&[("a", b'0', b"x"), ("A", b'0', b"y")]);
    assert!(matches!(
        unpack(dir.path(), "d.tar", &twice),
        Err(Error::Duplicate(_))
    ));
    assert!(matches!(
        unpack(
            dir.path(),
            "n.tar",
            b"not a tar at all, just words, ".repeat(40).as_slice()
        ),
        Err(Error::Unreadable(_))
    ));
}

#[test]
fn a_file_and_a_folder_of_one_name_in_any_case_are_refused_before_any_write() {
    // A file `a` and a folder `A`, or a file `a` and a file under it, would fail only once
    // the first was written; they are judged with the rest instead.
    for entries in [
        vec![("a", b'0', b"x".as_slice()), ("A/", b'5', b"".as_slice())],
        vec![("A/", b'5', b"".as_slice()), ("a", b'0', b"x".as_slice())],
        vec![("a", b'0', b"x".as_slice()), ("A/b", b'0', b"y".as_slice())],
    ] {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            matches!(
                unpack(dir.path(), "c.tar", &tar(&entries)),
                Err(Error::Duplicate(_))
            ),
            "{entries:?}"
        );
        assert!(!dir.path().join("out").exists(), "nothing written");
    }
    // A folder named twice, in two cases, is still one folder.
    let dir = tempfile::tempdir().unwrap();
    let fine = tar(&[("d/", b'5', b""), ("D/", b'5', b""), ("d/f", b'0', b"z")]);
    assert_eq!(unpack(dir.path(), "f.tar", &fine).unwrap().files, 1);
}

#[test]
fn a_pax_or_gnu_long_name_names_the_next_entry() {
    let long = format!("{}/file.txt", "d".repeat(150));
    let record = format!("path={long}\n");
    let len = record.len() + 4; // "NNN " with a three-digit length
    let pax = format!("{len} {record}");
    let bytes = tar(&[
        ("PaxHeader", b'x', pax.as_bytes()),
        ("short", b'0', b"long name"),
    ]);
    let dir = tempfile::tempdir().unwrap();
    unpack(dir.path(), "p.tar", &bytes).unwrap();
    assert!(dir.path().join("out").join(&long).exists());
    assert!(is_tar("a.TAR.GZ") && is_tar("b.tgz") && !is_tar("c.zip"));
}
