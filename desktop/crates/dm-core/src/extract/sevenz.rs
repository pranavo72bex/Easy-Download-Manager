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
//! Anything else (a BCJ filter, PPMd, BZip2, an AES password, an archive in several parts) is
//! refused with a plain message before anything is written. Every file's checksum (CRC32) is
//! checked as it is written; a mismatch fails the archive and removes its folder.

mod header;
mod lzma;

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use self::header::{Archive, Folder, Method, Refusal};
use super::{plain_mode, safe_path, Error, Extracted, Limits};

/// Whether `name` is a 7z archive.
pub fn is_7z(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".7z")
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

/// A reader of a folder's unpacked bytes, from the archive at `path`.
fn open_folder(path: &Path, folder: &Folder) -> Result<Box<dyn Read>, Error> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(folder.pack_offset))?;
    let packed: Box<dyn Read> = Box::new(file.take(folder.pack_size));
    let size = folder.unpack_size;
    let unreadable = |e: std::io::Error| Error::Unreadable(e.to_string());
    Ok(match &folder.method {
        Ok(Method::Copy) => packed,
        Ok(Method::Lzma(props)) => {
            Box::new(lzma::LzmaReader::new(packed, props, size).map_err(unreadable)?)
        }
        Ok(Method::Lzma2(prop)) => {
            Box::new(lzma::Lzma2Reader::new(packed, *prop, size).map_err(unreadable)?)
        }
        Err(Refusal::Encrypted) => return Err(Error::Encrypted("the archive".into())),
        Err(Refusal::Method(m)) => return Err(Error::Method(m.clone())),
    })
}

/// A whole folder's bytes, for a header packed into one; its checksum checked.
fn unpack_whole(path: &Path, folder: &Folder) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    open_folder(path, folder)?
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
}

/// Reads what the archive at `path` holds.
fn read_archive(path: &Path, max_entries: usize) -> Result<Archive, Error> {
    header::read(path, max_entries, &|f| unpack_whole(path, f))
}

/// One entry as it will be written: where, whether a folder, and its stream if it has one.
struct Planned {
    path: PathBuf,
    dir: bool,
    stream: Option<header::Stream>,
}

/// Extracts the 7z at `archive` into `folder`, which must not exist yet, as
/// `super::extract_into` does a ZIP.
pub(super) fn extract_into(
    archive: &Path,
    folder: &Path,
    limits: &Limits,
    cancel: &AtomicBool,
    origin: Option<&str>,
    progress: &dyn Fn(u64, u64),
) -> Result<Extracted, Error> {
    let parsed = read_archive(archive, limits.max_entries)?;
    if parsed.entries.len() > limits.max_entries {
        return Err(Error::TooBig("too many entries"));
    }

    // First pass: every entry judged before anything is written.
    let mut plan: Vec<Planned> = Vec::with_capacity(parsed.entries.len());
    let (mut total, mut seen) = (0u64, HashSet::new());
    for entry in &parsed.entries {
        let shown: String = entry.name.chars().take(120).collect();
        let path = safe_path(&entry.name).ok_or_else(|| Error::Escapes(shown.clone()))?;
        if entry.anti || special(entry.attributes) {
            return Err(Error::Special(shown));
        }
        if let Some(stream) = entry.stream {
            // A file in a folder dm cannot unpack fails the archive, named by that file.
            match parsed.folders.get(stream.folder).map(|f| &f.method) {
                Some(Ok(_)) => {}
                Some(Err(Refusal::Encrypted)) => return Err(Error::Encrypted(shown)),
                Some(Err(Refusal::Method(m))) => return Err(Error::Method(m.clone())),
                None => return Err(header::damaged("a stream with no folder")),
            }
        }
        if !seen.insert(path.to_string_lossy().to_lowercase()) && !entry.dir {
            // A folder named twice is the same folder; a file named twice is refused.
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
    let stored = fs::metadata(archive)?.len();
    if total > limits.ratio_floor && total / stored.max(1) > limits.max_ratio {
        return Err(Error::TooBig("it compresses far beyond anything real"));
    }
    let dirs: HashSet<&Path> = plan
        .iter()
        .filter(|p| p.dir)
        .map(|p| p.path.as_path())
        .collect();
    if plan
        .iter()
        .any(|p| !p.dir && dirs.contains(p.path.as_path()))
    {
        return Err(Error::Duplicate("a file is also a folder".into()));
    }

    fs::create_dir(folder)?;
    let made = write_all(
        archive, &parsed, folder, &plan, total, limits, cancel, origin, progress,
    );
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
#[allow(clippy::too_many_arguments)]
fn write_all(
    path: &Path,
    archive: &Archive,
    folder: &Path,
    plan: &[Planned],
    expected: u64,
    limits: &Limits,
    cancel: &AtomicBool,
    origin: Option<&str>,
    progress: &dyn Fn(u64, u64),
) -> Result<usize, Error> {
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
                    reader: open_folder(path, f)?,
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
