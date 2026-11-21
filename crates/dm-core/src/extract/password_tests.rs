// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Archives with a password (gaps row F10): real ZIPs by libarchive (AES-256, AES-128, the
//! older PKWARE cipher; `tests/fixtures/archives/zip/make.py`), real 7z archives by py7zr
//! (`7z/make_aes.py`), and ZIPs written here with AES-192 and AE-1, each with the right
//! password, the wrong one and none; nothing written but by the right one.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::crypt::{zip_aes_seal, ZipAes};
use super::*;

fn fixture(kind: &str, name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/archives")
        .join(kind)
        .join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

// The generator the fixtures' `make.py` scripts use.
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

/// Writes `bytes` as `name` in `dir` and extracts it with `password`.
fn run(dir: &Path, name: &str, bytes: &[u8], password: Option<&str>) -> Result<Extracted, Error> {
    let archive = dir.join(name);
    fs::write(&archive, bytes).unwrap();
    extract_in_with(
        &archive,
        None,
        password,
        &Limits::default(),
        &AtomicBool::new(false),
        None,
        &|_, _| {},
    )
}

/// Only the archive is in `dir`: nothing was made.
fn nothing_made(dir: &Path) {
    assert_eq!(fs::read_dir(dir).unwrap().count(), 1, "only the archive");
}

fn check_tree(out: &Path, hello: &[u8]) {
    assert_eq!(fs::read(out.join("docs/a.txt")).unwrap(), hello);
    assert_eq!(fs::read(out.join("docs/b.txt")).unwrap(), text(20000, 1));
    assert_eq!(fs::read(out.join("c.bin")).unwrap(), noise(3000, 2));
    assert_eq!(fs::read(out.join("zero.txt")).unwrap(), b"");
    assert!(out.join("docs/empty").is_dir());
}

#[test]
fn libarchive_zips_with_aes_and_the_older_cipher_open_with_their_password() {
    for name in ["aes256.zip", "aes128.zip", "pkware.zip"] {
        let dir = tempfile::tempdir().unwrap();
        let got = run(dir.path(), name, &fixture("zip", name), Some("dm secret")).unwrap();
        assert_eq!((got.files, got.bytes), (4, 11 + 20000 + 3000), "{name}");
        check_tree(&got.folder, b"hello, zip\n");
    }
}

#[test]
fn a_zip_without_its_password_says_so_as_it_always_did() {
    for name in ["aes256.zip", "aes128.zip", "pkware.zip"] {
        let dir = tempfile::tempdir().unwrap();
        let e = run(dir.path(), name, &fixture("zip", name), None).unwrap_err();
        assert!(matches!(e, Error::Encrypted(_)), "{name}: {e:?}");
        assert!(
            e.to_string()
                .starts_with("the archive is password-protected"),
            "{e}"
        );
        nothing_made(dir.path());
        // `extract` itself is unchanged.
        let archive = dir.path().join(name);
        let no = AtomicBool::new(false);
        let e = extract(&archive, &Limits::default(), &no, None, &|_, _| {}).unwrap_err();
        assert!(matches!(e, Error::Encrypted(_)), "{name}: {e:?}");
    }
}

#[test]
fn a_zip_with_the_wrong_password_is_said_and_nothing_is_written() {
    for name in ["aes256.zip", "aes128.zip", "pkware.zip"] {
        // Many wrong passwords, so the older cipher's one-byte check is passed by some and
        // the proof on a whole entry is what tells.
        for wrong in 0..40 {
            let dir = tempfile::tempdir().unwrap();
            let pw = format!("wrong {wrong}");
            match run(dir.path(), name, &fixture("zip", name), Some(&pw)) {
                Err(Error::WrongPassword(_)) => {}
                other => panic!("{name} with {pw}: {other:?}"),
            }
            nothing_made(dir.path());
        }
    }
}

/// A ZIP of one stored entry `name` sealed with AES by `zip_aes_seal`, written byte by
/// byte from the format's description: local header, data, central directory, end.
fn aes_zip(name: &str, plain: &[u8], aes: ZipAes, password: &str, crc_kept: bool) -> Vec<u8> {
    let salt: Vec<u8> = (0..aes.salt_len() as u8)
        .map(|b| b.wrapping_mul(37))
        .collect();
    let data = zip_aes_seal(aes, password.as_bytes(), &salt, plain);
    let crc = if crc_kept { crc32fast::hash(plain) } else { 0 };
    let mut extra = vec![0x01, 0x99, 7, 0];
    extra.extend_from_slice(&aes.vendor.to_le_bytes());
    extra.extend_from_slice(&[b'A', b'E', aes.strength]);
    extra.extend_from_slice(&aes.method.to_le_bytes());
    let fields = |out: &mut Vec<u8>| {
        out.extend_from_slice(&51u16.to_le_bytes()); // version needed
        out.extend_from_slice(&1u16.to_le_bytes()); // flags: encrypted
        out.extend_from_slice(&99u16.to_le_bytes()); // method: AES
        out.extend_from_slice(&[0, 0, 0x21, 0]); // time and date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(plain.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
    };
    let mut out = b"PK\x03\x04".to_vec();
    fields(&mut out);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&extra);
    out.extend_from_slice(&data);
    let central_at = out.len();
    out.extend_from_slice(b"PK\x01\x02");
    out.extend_from_slice(&51u16.to_le_bytes()); // made by
    fields(&mut out);
    out.extend_from_slice(&[0; 6]); // comment, disk, internal attributes
    out.extend_from_slice(&0u32.to_le_bytes()); // external attributes
    out.extend_from_slice(&0u32.to_le_bytes()); // local header at 0
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&extra);
    let central_len = out.len() - central_at;
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
    out.extend_from_slice(&(central_len as u32).to_le_bytes());
    out.extend_from_slice(&(central_at as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

#[test]
fn aes_192_and_ae_1_with_its_checksum_open_and_a_changed_byte_does_not() {
    let plain = text(7000, 9);
    for (strength, vendor) in [(2u8, 2u16), (2, 1), (3, 1), (1, 2)] {
        let aes = ZipAes {
            vendor,
            strength,
            method: 0,
        };
        let bytes = aes_zip("f.txt", &plain, aes, "pw", vendor == 1);
        let dir = tempfile::tempdir().unwrap();
        let got = run(dir.path(), "a.zip", &bytes, Some("pw")).unwrap();
        assert_eq!(fs::read(got.folder.join("f.txt")).unwrap(), plain);
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            run(dir.path(), "a.zip", &bytes, Some("pW")),
            Err(Error::WrongPassword(n)) if n == "f.txt"
        ));
        nothing_made(dir.path());
        // A byte changed in the middle: the authentication code fails, nothing is left.
        let mut bad = bytes.clone();
        let at = 30 + 5 + 11 + aes.salt_len() + 2 + 3000;
        bad[at] ^= 0x40;
        let dir = tempfile::tempdir().unwrap();
        assert!(run(dir.path(), "a.zip", &bad, Some("pw")).is_err());
        nothing_made(dir.path());
    }
    // AE-1 whose kept checksum is wrong: refused, though the code adds up.
    let aes = ZipAes {
        vendor: 1,
        strength: 3,
        method: 0,
    };
    let mut bytes = aes_zip("f.txt", &plain, aes, "pw", true);
    for at in [14, 30 + 5 + 11 + 28 + 7000 + 16] {
        bytes[at] ^= 1;
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(run(dir.path(), "a.zip", &bytes, Some("pw")).is_err());
    nothing_made(dir.path());
}

#[test]
fn aes_over_a_method_dm_does_not_unpack_is_refused_by_name() {
    let aes = ZipAes {
        vendor: 2,
        strength: 3,
        method: 12,
    };
    let bytes = aes_zip("f.txt", b"x", aes, "pw", false);
    let dir = tempfile::tempdir().unwrap();
    match run(dir.path(), "a.zip", &bytes, Some("pw")) {
        Err(Error::Method(m)) => assert!(m.contains("12"), "{m}"),
        other => panic!("{other:?}"),
    }
    nothing_made(dir.path());
}

#[test]
fn a_password_given_for_a_zip_without_one_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plain.zip");
    let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
    w.start_file("a.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    w.write_all(b"plain").unwrap();
    w.finish().unwrap();
    let bytes = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    let got = run(dir.path(), "plain.zip", &bytes, Some("anything")).unwrap();
    assert_eq!(fs::read(got.folder.join("a.txt")).unwrap(), b"plain");
}

#[test]
fn py7zr_archives_with_aes_open_with_their_password_list_of_files_and_all() {
    for name in ["aes.7z", "aes-header.7z"] {
        let dir = tempfile::tempdir().unwrap();
        let got = run(
            dir.path(),
            name,
            &fixture("7z", name),
            Some("p\u{e4}sswort 7z"),
        )
        .unwrap();
        assert_eq!((got.files, got.bytes), (5, 10 + 20000 + 3000 + 8), "{name}");
        check_tree(&got.folder, b"hello, 7z\n");
        assert_eq!(
            fs::read(got.folder.join("docs/na\u{ef}ve \u{65e5}\u{672c}.txt")).unwrap(),
            b"unicode\n"
        );
    }
}

#[test]
fn a_7z_without_its_password_or_with_the_wrong_one_writes_nothing() {
    for name in ["aes.7z", "aes-header.7z"] {
        let dir = tempfile::tempdir().unwrap();
        let e = run(dir.path(), name, &fixture("7z", name), None).unwrap_err();
        assert!(matches!(e, Error::Encrypted(_)), "{name}: {e:?}");
        nothing_made(dir.path());
        // The password's letters as ASCII alone: the key is made of its UTF-16.
        for wrong in ["passwort 7z", "p\u{e4}sswort 7Z", ""] {
            let dir = tempfile::tempdir().unwrap();
            match run(dir.path(), name, &fixture("7z", name), Some(wrong)) {
                Err(Error::WrongPassword(_)) => {}
                other => panic!("{name} with {wrong:?}: {other:?}"),
            }
            nothing_made(dir.path());
        }
    }
    // A list of files kept in the clear is read without the password; its files are not.
    let dir = tempfile::tempdir().unwrap();
    match run(dir.path(), "aes.7z", &fixture("7z", "aes.7z"), None) {
        Err(Error::Encrypted(n)) => assert_eq!(n, "docs/a.txt"),
        other => panic!("{other:?}"),
    }
    let dir = tempfile::tempdir().unwrap();
    match run(dir.path(), "h.7z", &fixture("7z", "aes-header.7z"), None) {
        Err(Error::Encrypted(n)) => assert_eq!(n, "its list of files"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_tar_takes_no_password_and_unpacks_as_ever() {
    // A password given for an archive kind that has none is not an error.
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("x.tar");
    let mut header = [0u8; 512];
    header[..5].copy_from_slice(b"a.txt");
    header[100..107].copy_from_slice(b"0000644");
    header[124..135].copy_from_slice(b"00000000002");
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
    let mut bytes = header.to_vec();
    let mut body = [0u8; 512];
    body[..2].copy_from_slice(b"hi");
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&[0u8; 1024]);
    fs::write(&archive, &bytes).unwrap();
    let got = extract_in_with(
        &archive,
        None,
        Some("pw"),
        &Limits::default(),
        &AtomicBool::new(false),
        None,
        &|_, _| {},
    )
    .unwrap();
    assert_eq!(fs::read(got.folder.join("a.txt")).unwrap(), b"hi");
}
