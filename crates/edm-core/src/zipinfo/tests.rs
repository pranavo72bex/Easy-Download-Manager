// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A file said to unpack to more than the cap is refused before anything is fetched, and
/// unpacking stops at the cap whatever the list says (security audit).
#[tokio::test]
async fn a_file_said_to_unpack_past_the_cap_is_refused_and_never_unpacked_past_it() {
    let huge = ZipEntry {
        name: "zeros.bin".into(),
        size: 10 * MOST_TAKEN,
        packed: 1024,
        at: 0,
        method: 8,
        crc: 0,
    };
    let dir = tempfile::tempdir().unwrap();
    let why = zip_take_one(
        &crate::client(),
        "http://127.0.0.1:9/a.zip",
        None,
        None,
        &huge,
        dir.path(),
    )
    .await
    .unwrap_err();
    assert!(why.contains("too large"), "{why}");
    // Deflated zeros: a few kilobytes that would unpack to far more.
    use std::io::Write;
    let mut packer = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    packer.write_all(&vec![0u8; 4 * 1024 * 1024]).unwrap();
    let packed = packer.finish().unwrap();
    let lying = ZipEntry {
        size: u64::MAX,
        packed: packed.len() as u64,
        ..huge
    };
    // Unpacked no further than the cap plus one, then refused as not what the list says.
    assert!(unpack(&packed, &lying).is_err());
}

/// A ZIP with a folder and two files, stored, built by the writer.
fn archive() -> Vec<u8> {
    use std::io::Write;
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.add_directory("docs/", stored).unwrap();
        zip.start_file("docs/readme.txt", stored).unwrap();
        zip.write_all(&vec![b'a'; 1000]).unwrap();
        zip.start_file("setup.exe", stored).unwrap();
        zip.write_all(&vec![b'b'; 5000]).unwrap();
        zip.set_comment("a comment after the directory").unwrap();
        zip.finish().unwrap();
    }
    buffer.into_inner()
}

#[test]
fn the_directory_is_found_through_a_comment_and_its_files_are_listed() {
    let bytes = archive();
    let (at, directory) = end_of_directory(&bytes).expect("an end record");
    assert!(at > 0);
    assert_eq!(directory.entries, 3);
    let start = usize::try_from(directory.offset).unwrap();
    let end = start + usize::try_from(directory.size).unwrap();
    let entries = entries_of(&bytes[start..end], directory.entries).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, "docs/");
    assert!(entries[0].is_folder());
    assert_eq!(
        entries[1],
        ZipEntry {
            name: "docs/readme.txt".into(),
            size: 1000,
            packed: 1000,
            ..entries[1].clone()
        }
    );
    assert_eq!(entries[2].name, "setup.exe");
    assert_eq!(entries[2].size, 5000);
}

#[test]
fn bytes_that_are_not_a_zip_or_are_cut_off_list_nothing() {
    assert!(end_of_directory(b"just some text, not an archive at all").is_none());
    // A record whose comment does not reach the end is not the record.
    let mut bytes = archive();
    bytes.push(0);
    assert!(end_of_directory(&bytes).is_none());
    // A directory cut short is not read.
    let good = archive();
    let (_, directory) = end_of_directory(&good).unwrap();
    let start = usize::try_from(directory.offset).unwrap();
    assert!(entries_of(&good[start..start + 30], 3).is_none());
}

#[test]
fn a_zip_on_disk_is_listed_from_its_end_and_a_file_that_is_not_one_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("pack.zip");
    std::fs::write(&zip, archive()).unwrap();
    let entries = zip_contents_of_file(&zip).expect("the entries");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[1].name, "docs/readme.txt");
    assert_eq!(entries[2].size, 5000);

    let text = dir.path().join("notes.zip");
    std::fs::write(
        &text,
        b"just some text that is long enough to be read, but no archive",
    )
    .unwrap();
    assert!(zip_contents_of_file(&text).is_none());
    assert!(zip_contents_of_file(&dir.path().join("missing.zip")).is_none());
}

/// The nightly fuzzer's find (issue #72, `crash-7eb7dded`): a ZIP64 locator naming a record at
/// the end of what a u64 holds. Reading 56 bytes from there overflowed and panicked; it is no
/// listing now, as any other archive that does not add up.
#[test]
fn a_zip64_record_named_past_the_end_of_numbers_is_no_listing_not_a_panic() {
    let crash: &[u8] = &[
        0x84, 0xf, 0xf, 0xf, 0xf, 0xf, 0xa, 0xf, 0xf, 0xf, 0xf, 0x0, 0x0, 0x0, 0x50, 0x4b, 0x6,
        0x7, 0x20, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x0,
        0xa3, 0xff, 0x50, 0x4b, 0x5, 0x6, 0xff, 0xff, 0xff, 0x0, 0xef, 0xff, 0xff, 0xff, 0xca,
        0xca, 0xca, 0xd0, 0xca, 0xca, 0x24, 0x0, 0x0, 0x0,
    ];
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.zip");
    std::fs::write(&file, crash).unwrap();
    assert!(zip_contents_of_file(&file).is_none());
}
