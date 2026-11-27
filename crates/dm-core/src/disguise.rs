// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finished file that is a program while its name says a picture, a film, a document or an
//! archive (gaps row H2): read from its first bytes, which a program's system reads to run it,
//! whatever the name says. Nothing is sent anywhere; nothing is changed in the file.

use std::io::Read;
use std::path::Path;

/// How many of a file's first bytes are read.
const HEAD: usize = 64;

/// What `head` (a file's first bytes) says the file runs as, in words for the row: a Windows,
/// Linux or macOS program, an Android app’s code, or a script. `None` for anything else.
pub fn program_in(head: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| head.starts_with(magic);
    if starts(b"MZ") {
        Some("a Windows program")
    } else if starts(b"\x7fELF") {
        Some("a Linux program")
    } else if starts(&[0xfe, 0xed, 0xfa, 0xce])
        || starts(&[0xfe, 0xed, 0xfa, 0xcf])
        || starts(&[0xce, 0xfa, 0xed, 0xfe])
        || starts(&[0xcf, 0xfa, 0xed, 0xfe])
        || starts(&[0xca, 0xfe, 0xba, 0xbe])
    {
        // 0xcafebabe is a universal macOS program and also Java's class file: both run.
        Some("a macOS program")
    } else if starts(b"dex\n") {
        Some("an Android app’s code")
    } else if starts(b"#!") {
        Some("a script")
    } else {
        None
    }
}

/// The endings that name something a person opens to look at or listen to, never to run:
/// a file so named that is a program is one in disguise.
const NOT_PROGRAMS: &[&str] = &[
    // Pictures.
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "avif", "svg", "ico",
    // Sound and film.
    "mp3", "m4a", "aac", "flac", "ogg", "opus", "wav", "wma", "mp4", "m4v", "mkv", "webm", "mov",
    "avi", "wmv", "flv", "3gp", "ts", // Documents.
    "pdf", "txt", "rtf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "csv",
    "epub", "md", // Archives (a program meant to unpack itself is named `.exe`).
    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "iso",
];

/// What the file named `name` whose first bytes are `head` really is, where its name says
/// otherwise: `Some("a Windows program")` for `photo.jpg` that begins `MZ`. `None` where the
/// name claims nothing (no ending, or one that may be a program) or the contents agree.
pub fn disguised(name: &str, head: &[u8]) -> Option<&'static str> {
    let ending = Path::new(name)
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase();
    if !NOT_PROGRAMS.contains(&ending.as_str()) {
        return None;
    }
    program_in(head)
}

/// [`disguised`] for the finished file at `path`, reading its first bytes; `None` for a folder
/// or a file that cannot be read.
pub fn of_file(path: &Path) -> Option<&'static str> {
    if !path.is_file() {
        return None;
    }
    let mut head = Vec::with_capacity(HEAD);
    std::fs::File::open(path)
        .ok()?
        .take(HEAD as u64)
        .read_to_end(&mut head)
        .ok()?;
    disguised(&path.file_name()?.to_string_lossy(), &head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_named_as_a_picture_or_document_is_found() {
        assert_eq!(
            disguised("photo.JPG", b"MZ\x90\x00"),
            Some("a Windows program")
        );
        assert_eq!(
            disguised("invoice.pdf", b"\x7fELF\x02"),
            Some("a Linux program")
        );
        assert_eq!(
            disguised("film.mp4", &[0xcf, 0xfa, 0xed, 0xfe, 7]),
            Some("a macOS program")
        );
        assert_eq!(
            disguised("notes.txt", b"#!/bin/sh\nrm -rf"),
            Some("a script")
        );
        assert_eq!(disguised("pack.zip", b"MZ"), Some("a Windows program"));
    }

    #[test]
    fn what_its_name_says_is_left_alone() {
        // A program named as one, a file with no ending, and a picture that is one.
        assert_eq!(disguised("setup.exe", b"MZ"), None);
        assert_eq!(disguised("installer", b"\x7fELF"), None);
        assert_eq!(disguised("run.sh", b"#!/bin/sh"), None);
        assert_eq!(disguised("photo.jpg", b"\xff\xd8\xff\xe0"), None);
        assert_eq!(disguised("doc.pdf", b"%PDF-1.7"), None);
        assert_eq!(disguised("short.png", b""), None);
    }

    #[test]
    fn the_finished_file_is_read_from_its_first_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("cat.png");
        std::fs::write(&fake, [b"MZ".as_slice(), &[0u8; 4096]].concat()).unwrap();
        assert_eq!(of_file(&fake), Some("a Windows program"));
        let real = dir.path().join("dog.png");
        std::fs::write(&real, b"\x89PNG\r\n\x1a\n").unwrap();
        assert_eq!(of_file(&real), None);
        assert_eq!(of_file(dir.path()), None);
    }
}
