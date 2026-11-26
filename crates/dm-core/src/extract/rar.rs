// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A RAR archive (`.rar`), read by dm's own code from the format's public description, under
//! the same rules as a ZIP, a tar or a 7z (gaps row F10): every entry judged before anything
//! is written, nothing outside the folder, no links or special files, no name twice, the
//! limits held.
//!
//! RAR 5 archives are read whole: files stored as they are, and RAR 5's compression (methods
//! 1 to 5) by dm's own decompressor (`unpack.rs`: LZ77 under Huffman codes, with the DELTA,
//! E8, E8E9 and ARM filters), solid archives (one stream across many files) included. An
//! archive of RAR 1.5 to 4.x has its stored files extracted, and those packed by RAR 2.9 to
//! 4.x unpacked by dm's own decompressor for that format (`v3.rs`: LZ under its Huffman
//! tables, PPMd, and the standard filters); the compression of RAR 2.x and older is refused
//! in words. Every file's CRC32 is checked as it is written; a mismatch fails the archive and
//! removes its folder.
//!
//! A RAR 5 archive in parts is read from its first part (`film.part1.rar`), the next ones found
//! beside it by their names and read in place, never joined on disk (gaps row C3c).
//!
//! Refused before anything is written, in plain words: an archive with a password (its list
//! of files or any file encrypted), an old-format one in parts or one whose parts are named
//! otherwise, a part of a set that is missing or is not the first, a later version's
//! compression, and a file checked only by a BLAKE2 hash, which dm does not compute.

mod filters;
mod header;
mod huffman;
mod old;
mod unpack;
mod v3;

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use self::header::{damaged, Entry, Pack, Span, OLD_SIGNATURE, SIGNATURE};
use self::unpack::Unpacker;
use super::{safe_path, Error, Extracted, Job};

/// Whether `name` is a RAR archive.
pub fn is_rar(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".rar")
}

/// Reads the list of the archive at `path`, RAR 5 or older, with the paths of its parts.
fn read_archive(path: &Path, max_entries: usize) -> Result<(Vec<PathBuf>, Vec<Entry>), Error> {
    let mut start = Vec::new();
    File::open(path)?.take(8).read_to_end(&mut start)?;
    if start.starts_with(&SIGNATURE) {
        header::read(path, max_entries)
    } else if start.starts_with(&OLD_SIGNATURE) {
        Ok((vec![path.to_path_buf()], old::read(path, max_entries)?))
    } else {
        Err(Error::Unreadable("this is not a RAR archive".into()))
    }
}

/// Why `entry`, a file, cannot be unpacked, if it cannot; `shown` names it.
fn refusal(entry: &Entry, shown: &str) -> Option<Error> {
    if entry.encrypted {
        return Some(Error::Encrypted(shown.into()));
    }
    if entry.dir {
        return None;
    }
    if let Pack::Refused(what) = &entry.pack {
        return Some(Error::Method(what.clone()));
    }
    if entry.crc.is_none() && entry.blake2 {
        return Some(Error::Method("BLAKE2 checksums in place of CRC32".into()));
    }
    if entry.crc.is_none() && entry.size > 0 {
        return Some(damaged("a file carries no checksum"));
    }
    if entry.pack == Pack::Stored && entry.packed != entry.size {
        return Some(damaged("a stored file's sizes differ"));
    }
    None
}

/// One entry as it will be written.
struct Planned {
    path: PathBuf,
    index: usize,
}

/// Extracts the RAR at `archive` into `folder`, which must not exist yet, as
/// `super::extract_into` does a ZIP.
pub(super) fn extract_into(archive: &Path, folder: &Path, job: &Job) -> Result<Extracted, Error> {
    let limits = job.limits;
    let (volumes, entries) = read_archive(archive, limits.max_entries)?;

    // First pass: every entry judged before anything is written.
    let mut plan: Vec<Planned> = Vec::with_capacity(entries.len());
    let (mut total, mut seen) = (0u64, HashMap::new());
    let mut streamed = false;
    for (index, entry) in entries.iter().enumerate() {
        let shown: String = entry.name.chars().take(120).collect();
        let path = safe_path(&entry.name).ok_or_else(|| Error::Escapes(shown.clone()))?;
        if entry.special {
            return Err(Error::Special(shown));
        }
        if let Some(e) = refusal(entry, &shown) {
            return Err(e);
        }
        // A solid file carries on the stream of the compressed file before it.
        if let Pack::Lz { solid, .. } | Pack::Rar4 { solid, .. } = entry.pack {
            if solid && !streamed {
                return Err(damaged("a solid file has no file before it"));
            }
            streamed = true;
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
        if !entry.dir {
            total = total.saturating_add(entry.size);
        }
        if total > limits.max_bytes {
            return Err(Error::TooBig("too many bytes"));
        }
        plan.push(Planned { path, index });
    }
    if let Some(shown) = super::inside_a_file(plan.iter().map(|p| p.path.as_path()), &seen) {
        return Err(Error::Duplicate(shown));
    }
    let mut stored = 0u64;
    for volume in &volumes {
        stored = stored.saturating_add(fs::metadata(volume)?.len());
    }
    if total > limits.ratio_floor && total / stored.max(1) > limits.max_ratio {
        return Err(Error::TooBig("it compresses far beyond anything real"));
    }

    fs::create_dir(folder)?;
    let made = write_all(&volumes, &entries, folder, &plan, total, job);
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

/// Where the bytes of the files written go: the file, its checksum, the counts and the
/// progress.
struct Out<'a> {
    file: File,
    crc: crc32fast::Hasher,
    /// Bytes of this file so far, and the most it may have.
    wrote: u64,
    size: u64,
    /// Bytes of the files before it, and of all of them.
    done: u64,
    expected: u64,
    job: &'a Job<'a>,
}

impl Out<'_> {
    fn take(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.job.cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        self.wrote += bytes.len() as u64;
        // The header's size is a claim; the bytes are the fact.
        if self.wrote > self.size || self.done + self.wrote > self.job.limits.max_bytes {
            return Err(Error::TooBig("an entry is larger than it said"));
        }
        self.crc.update(bytes);
        self.file.write_all(bytes)?;
        (self.job.progress)(self.done + self.wrote, self.expected);
        Ok(())
    }
}

/// The second pass: the entries judged, written in their order, a solid stream unpacked
/// once from start to end as its files come.
fn write_all(
    volumes: &[PathBuf],
    entries: &[Entry],
    folder: &Path,
    plan: &[Planned],
    expected: u64,
    job: &Job,
) -> Result<usize, Error> {
    let (mut done, mut files) = (0u64, 0usize);
    let mut stream: Option<Unpacker> = None;
    let mut stream4: Option<v3::Unpacker> = None;
    let mut buf = vec![0u8; 256 * 1024];
    (job.progress)(0, expected);
    for p in plan {
        if job.cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let Some(entry) = entries.get(p.index) else {
            return Err(damaged("an entry with no header"));
        };
        let target = folder.join(&p.path);
        if entry.dir {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // `create_new`: never replaces, never follows a link that is there.
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let mut out = Out {
            file,
            crc: crc32fast::Hasher::new(),
            wrote: 0,
            size: entry.size,
            done,
            expected,
            job,
        };
        let mut packed = Packed {
            volumes,
            spans: &entry.spans,
            next: 0,
            reading: None,
        };
        match entry.pack {
            Pack::Stored => copy(&mut packed, &mut buf, &mut out)?,
            Pack::Lz { solid, dict } => {
                // A file that is not solid starts a stream of its own.
                match (&mut stream, solid) {
                    (Some(u), true) => u.solid(dict)?,
                    (s, _) => *s = Some(Unpacker::new(dict)),
                }
                let Some(unpacker) = stream.as_mut() else {
                    return Err(damaged("a solid file has no file before it"));
                };
                if entry.packed > 0 || entry.size > 0 {
                    unpacker.file(&mut packed, entry.size, &mut |b: &[u8]| out.take(b))?;
                }
            }
            Pack::Rar4 { solid, dict } => {
                match (&mut stream4, solid) {
                    (Some(u), true) => u.solid(dict)?,
                    (s, _) => *s = Some(v3::Unpacker::new(dict)),
                }
                let Some(unpacker) = stream4.as_mut() else {
                    return Err(damaged("a solid file has no file before it"));
                };
                if entry.packed > 0 || entry.size > 0 {
                    unpacker.file(&mut packed, entry.size, &mut |b: &[u8]| out.take(b))?;
                }
            }
            Pack::Refused(ref what) => return Err(Error::Method(what.clone())),
        }
        if out.wrote != entry.size {
            return Err(Error::Unreadable(
                "the archive ends in the middle of a file".into(),
            ));
        }
        if entry
            .crc
            .is_some_and(|want| want != out.crc.clone().finalize())
        {
            return Err(Error::Unreadable(format!(
                "the archive is damaged: {} does not add up",
                p.path.display()
            )));
        }
        out.file.flush()?;
        drop(out);
        done += entry.size;
        files += 1;
        if let Some(url) = job.origin {
            crate::motw::mark(&target, url, "");
        }
    }
    Ok(files)
}

/// A file's packed bytes, read run by run from the parts they are in.
struct Packed<'a> {
    volumes: &'a [PathBuf],
    spans: &'a [Span],
    /// The next run to open, and the one being read.
    next: usize,
    reading: Option<std::io::Take<File>>,
}

impl Read for Packed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if let Some(run) = self.reading.as_mut() {
                let n = run.read(buf)?;
                if n > 0 || buf.is_empty() {
                    return Ok(n);
                }
                self.reading = None;
            }
            let Some(span) = self.spans.get(self.next) else {
                return Ok(0);
            };
            self.next += 1;
            let path = self
                .volumes
                .get(span.volume)
                .ok_or_else(|| std::io::Error::other("a part of the archive is not there"))?;
            let mut file = File::open(path)?;
            file.seek(SeekFrom::Start(span.offset))?;
            self.reading = Some(file.take(span.len));
        }
    }
}

/// A stored file's bytes, copied as they are.
fn copy(packed: &mut dyn Read, buf: &mut [u8], out: &mut Out) -> Result<(), Error> {
    loop {
        let n = packed
            .read(buf)
            .map_err(|e| Error::Unreadable(e.to_string()))?;
        if n == 0 {
            return Ok(());
        }
        out.take(buf.get(..n).unwrap_or_default())?;
    }
}

#[cfg(test)]
mod writer;

#[cfg(test)]
mod tests;
