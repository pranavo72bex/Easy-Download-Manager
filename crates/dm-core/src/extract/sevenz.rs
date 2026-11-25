// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A 7z archive (`.7z`), read by dm's own code from the format's public description, under the
//! same rules as a ZIP or a tar (gaps row F10): every entry judged before anything is written,
//! nothing outside the folder, no links or special files, no name twice, the limits held.
//!
//! dm unpacks the methods nearly every 7z archive uses: stored (Copy), LZMA and LZMA2, with
//! its own decoders (`lzma.rs`), solid archives (many files in one packed stream) included.
//! Anything else (a BCJ filter, PPMd, BZip2, an archive in several parts) is refused with a
//! plain message before anything is written. Every file's checksum (CRC32) is checked as it
//! is written; a mismatch fails the archive and removes its folder.
//!
//! With a password, a folder 7-Zip encrypted (AES-256 under one of those methods, its list
//! of files too) is decrypted (`crypt.rs`). A 7z carries no check of the password itself, so
//! it is proved before anything is written: by the list of files' checksum where that is
//! encrypted, or else by unpacking one encrypted folder's first file and checking its CRC;
//! once for each key the archive uses.

mod header;
mod keys;
mod lzma;
mod volumes;

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use self::header::{Archive, Folder, Method, Refusal};
use self::keys::{aes_of, Keys};
use super::crypt::CbcReader;
use super::{plain_mode, safe_path, Error, Extracted, Job};

/// Whether `name` is a 7z archive, or the first part of one in parts (`.7z.001`, gaps row
/// C3b): the rest are read from beside it.
pub fn is_7z(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".7z") || name.ends_with(".7z.001")
}

/// A Windows attribute: the entry is a reparse point (a link or junction).
const REPARSE_POINT: u32 = 0x400;
/// A 7z attribute: the high 16 bits hold a Unix mode.
const UNIX_EXTENSION: u32 = 0x8000;

/// Whether an entry's attributes make it a link or special file.
fn special(attributes: Option<u32>) -> bool {
    let Some(a) = attributes else {
        return false;
    };
    a & REPARSE_POINT != 0 || (a & UNIX_EXTENSION != 0 && !plain_mode(Some(a >> 16)))
}

/// Whether a folder can be unpacked with what was given; `shown` names it in an error.
fn usable(folder: &Folder, keys: &Keys, shown: &str) -> Result<(), Error> {
    if folder.encrypted && !keys.given() {
        return Err(Error::Encrypted(shown.into()));
    }
    match &folder.method {
        Ok(_) => aes_of(folder).map(|_| ()),
        Err(Refusal::Method(m)) => Err(Error::Method(m.clone())),
    }
}

/// A reader of a folder's unpacked bytes, from the archive at `path`.
fn open_folder(path: &Path, folder: &Folder, keys: &Keys) -> Result<Box<dyn Read>, Error> {
    usable(folder, keys, "the archive")?;
    let mut file = volumes::open(path)?;
    file.seek(SeekFrom::Start(folder.pack_offset))?;
    let mut packed: Box<dyn Read> = Box::new(file.take(folder.pack_size));
    let size = folder.unpack_size;
    let unreadable = |e: std::io::Error| Error::Unreadable(e.to_string());
    if let (Some(aes), Some(coder)) = (aes_of(folder)?, folder.aes.as_ref()) {
        let key = keys
            .key(&aes)?
            .ok_or_else(|| Error::Encrypted("the archive".into()))?;
        packed = Box::new(CbcReader::new(packed, &key, aes.iv, coder.size).map_err(unreadable)?);
    }
    Ok(match &folder.method {
        Ok(Method::Copy) => packed,
        Ok(Method::Lzma(props)) => {
            Box::new(lzma::LzmaReader::new(packed, props, size).map_err(unreadable)?)
        }
        Ok(Method::Lzma2(prop)) => {
            Box::new(lzma::Lzma2Reader::new(packed, *prop, size).map_err(unreadable)?)
        }
        Err(Refusal::Method(m)) => return Err(Error::Method(m.clone())),
    })
}

/// A whole folder's bytes, for a header packed into one; its checksum checked. An encrypted
/// one that does not unpack or add up was opened with the wrong password; one that does
/// proves its key.
fn unpack_whole(path: &Path, folder: &Folder, keys: &Keys) -> Result<Vec<u8>, Error> {
    const WHAT: &str = "the archive's list of files";
    // Without a password, the words it always had.
    usable(folder, keys, "its list of files")?;
    let unpacked = || -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        open_folder(path, folder, keys)?
            .take(folder.unpack_size)
            .read_to_end(&mut out)
            .map_err(|e| Error::Unreadable(e.to_string()))?;
        if out.len() as u64 != folder.unpack_size {
            return Err(header::damaged("its packed list of files ends early"));
        }
        if folder.crc.is_some_and(|crc| crc32fast::hash(&out) != crc) {
            return Err(header::damaged("its packed list of files does not add up"));
        }
        Ok(out)
    };
    match (unpacked(), aes_of(folder)?) {
        (Err(Error::Unreadable(_)), Some(_)) => Err(Error::WrongPassword(WHAT.into())),
        (Ok(out), Some(aes)) => {
            if folder.crc.is_some() {
                keys.prove(&aes);
            }
            Ok(out)
        }
        (got, _) => got,
    }
}

/// Reads what the archive at `path` holds.
fn read_archive(path: &Path, max_entries: usize, keys: &Keys) -> Result<Archive, Error> {
    // An encrypted list of files with no checksum of its own unpacks, under the wrong
    // password, into bytes that are no list: that too is the wrong password.
    let decrypted = std::cell::Cell::new(false);
    let read = header::read(path, max_entries, &|f| {
        decrypted.set(f.aes.is_some());
        unpack_whole(path, f, keys)
    });
    match read {
        Err(Error::Unreadable(_)) if decrypted.get() => {
            Err(Error::WrongPassword("the archive's list of files".into()))
        }
        other => other,
    }
}

/// Proves the password on each key the archive's files are encrypted with that its list of
/// files has not already proved: the first file of the first folder of that key is unpacked
/// and its checksum compared, nothing written. A 7z has no other check of a password.
fn prove(path: &Path, archive: &Archive, keys: &Keys, job: &Job) -> Result<(), Error> {
    const WHAT: &str = "the archive";
    for folder in &archive.folders {
        let Some(aes) = aes_of(folder)? else {
            continue;
        };
        let Some(&(size, crc)) = folder.streams.first() else {
            continue;
        };
        if keys.proved(&aes) {
            continue;
        }
        let mut reader = open_folder(path, folder, keys)?;
        let mut sum = crc32fast::Hasher::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut left = size;
        while left > 0 {
            if job.cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let want = (buf.len() as u64).min(left) as usize;
            let chunk = buf.get_mut(..want).unwrap_or_default();
            let n = match reader.read(chunk) {
                Ok(0) | Err(_) => return Err(Error::WrongPassword(WHAT.into())),
                Ok(n) => n,
            };
            sum.update(chunk.get(..n).unwrap_or_default());
            left -= n as u64;
        }
        if crc.is_some_and(|want| want != sum.finalize()) {
            return Err(Error::WrongPassword(WHAT.into()));
        }
        keys.prove(&aes);
    }
    Ok(())
}

/// One entry as it will be written: where, whether a folder, and its stream if it has one.
struct Planned {
    path: PathBuf,
    dir: bool,
    stream: Option<header::Stream>,
}

/// Extracts the 7z at `archive` into `folder`, which must not exist yet, as
/// `super::extract_into` does a ZIP.
pub(super) fn extract_into(archive: &Path, folder: &Path, job: &Job) -> Result<Extracted, Error> {
    let limits = job.limits;
    let keys = Keys::new(job.password);
    let parsed = read_archive(archive, limits.max_entries, &keys)?;
    if parsed.entries.len() > limits.max_entries {
        return Err(Error::TooBig("too many entries"));
    }

    // First pass: every entry judged before anything is written.
    let mut plan: Vec<Planned> = Vec::with_capacity(parsed.entries.len());
    let (mut total, mut seen) = (0u64, HashMap::new());
    for entry in &parsed.entries {
        let shown: String = entry.name.chars().take(120).collect();
        let path = safe_path(&entry.name).ok_or_else(|| Error::Escapes(shown.clone()))?;
        if entry.anti || special(entry.attributes) {
            return Err(Error::Special(shown));
        }
        if let Some(stream) = entry.stream {
            // A file in a folder dm cannot unpack fails the archive, named by that file.
            match parsed.folders.get(stream.folder) {
                Some(f) => usable(f, &keys, &shown)?,
                None => return Err(header::damaged("a stream with no folder")),
            }
        }
        // A folder named twice is the same folder; any other repeat, in any case, is refused
        // here, a file and a folder of one name included, before anything is written.
        let key = path.to_string_lossy().to_lowercase();
        if seen
            .insert(key, entry.dir)
            .is_some_and(|was_dir| !(was_dir && entry.dir))
        {
            return Err(Error::Duplicate(shown));
        }
        let size = entry.stream.map_or(0, |s| s.size);
        total = total.saturating_add(size);
        if total > limits.max_bytes {
            return Err(Error::TooBig("too many bytes"));
        }
        plan.push(Planned {
            path,
            dir: entry.dir,
            stream: entry.stream,
        });
    }
    if let Some(shown) = super::inside_a_file(plan.iter().map(|p| p.path.as_path()), &seen) {
        return Err(Error::Duplicate(shown));
    }
    let stored = volumes::open(archive)?.len();
    if total > limits.ratio_floor && total / stored.max(1) > limits.max_ratio {
        return Err(Error::TooBig("it compresses far beyond anything real"));
    }

    // The password proved before the folder is made.
    prove(archive, &parsed, &keys, job)?;

    fs::create_dir(folder)?;
    let made = write_all(archive, &parsed, folder, &plan, total, &keys, job);
    match made {
        Ok(files) => Ok(Extracted {
            folder: folder.to_path_buf(),
            files,
            bytes: total,
        }),
        Err(e) => {
            // The folder is one this call made, so it is this call's to remove.
            let _ = fs::remove_dir_all(folder);
            Err(e)
        }
    }
}

/// The folder being unpacked as files are written from it, with its checksum so far.
struct Open {
    index: usize,
    reader: Box<dyn Read>,
    crc: crc32fast::Hasher,
}

/// Checks a folder finished: every stream read, so its own checksum can be compared.
fn finish(open: Open, archive: &Archive) -> Result<(), Error> {
    let want = archive.folders.get(open.index).and_then(|f| f.crc);
    if want.is_some_and(|w| w != open.crc.finalize()) {
        return Err(Error::Unreadable(
            "the archive is damaged: a packed stream does not add up".into(),
        ));
    }
    Ok(())
}

/// The second pass: the entries judged, written in their order, each folder unpacked once
/// from start to end as its files come.
fn write_all(
    path: &Path,
    archive: &Archive,
    folder: &Path,
    plan: &[Planned],
    expected: u64,
    keys: &Keys,
    job: &Job,
) -> Result<usize, Error> {
    let (limits, cancel, origin, progress) = (job.limits, job.cancel, job.origin, job.progress);
    let (mut done, mut files) = (0u64, 0usize);
    let mut buf = vec![0u8; 256 * 1024];
    let mut open: Option<Open> = None;
    progress(0, expected);
    for entry in plan {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let target = folder.join(&entry.path);
        if entry.dir {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // `create_new`: never replaces, never follows a link that is there.
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        if let Some(stream) = entry.stream {
            // Streams come in folder order, so a new folder means the last one is done.
            if open.as_ref().is_none_or(|o| o.index != stream.folder) {
                if let Some(last) = open.take() {
                    finish(last, archive)?;
                }
                let f = archive
                    .folders
                    .get(stream.folder)
                    .ok_or_else(|| header::damaged("a stream with no folder"))?;
                open = Some(Open {
                    index: stream.folder,
                    reader: open_folder(path, f, keys)?,
                    crc: crc32fast::Hasher::new(),
                });
            }
            let Some(o) = open.as_mut() else {
                return Err(header::damaged("a stream with no folder"));
            };
            let mut crc = crc32fast::Hasher::new();
            let mut left = stream.size;
            while left > 0 {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                let want = (buf.len() as u64).min(left) as usize;
                let chunk = buf.get_mut(..want).unwrap_or_default();
                let n = o
                    .reader
                    .read(chunk)
                    .map_err(|e| Error::Unreadable(e.to_string()))?;
                if n == 0 {
                    return Err(Error::Unreadable(
                        "the archive ends in the middle of a file".into(),
                    ));
                }
                let bytes = chunk.get(..n).unwrap_or_default();
                crc.update(bytes);
                o.crc.update(bytes);
                // The sizes were judged; the bytes written are held to them all the same.
                if done + (stream.size - left) + n as u64 > limits.max_bytes {
                    return Err(Error::TooBig("too many bytes"));
                }
                out.write_all(bytes)?;
                left -= n as u64;
                progress(done + (stream.size - left), expected);
            }
            if stream.crc.is_some_and(|want| want != crc.finalize()) {
                return Err(Error::Unreadable(format!(
                    "the archive is damaged: {} does not add up",
                    entry.path.display()
                )));
            }
            done += stream.size;
            let last = archive
                .folders
                .get(stream.folder)
                .is_some_and(|f| stream.index + 1 >= f.streams.len());
            if last {
                if let Some(o) = open.take() {
                    finish(o, archive)?;
                }
            }
        }
        out.flush()?;
        drop(out);
        files += 1;
        if let Some(url) = origin {
            crate::motw::mark(&target, url, "");
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests;
