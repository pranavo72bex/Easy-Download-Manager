// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A tar archive, plain or gzipped (`.tar`, `.tar.gz`, `.tgz`), read by dm's own code from the
//! format as POSIX describes it (ustar, with pax and GNU long names), under the same rules as a
//! ZIP (gaps row F10): every entry judged before anything is written, nothing outside the
//! folder, no links or special files, no name twice, the limits held to the bytes written.
//!
//! A tar has no index, so it is read twice: once to judge, once to write.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{safe_path, Error, Extracted, Limits};

/// A tar block.
const BLOCK: usize = 512;

/// The most a pax header or a GNU long name may be: a path, not a file.
const META_MOST: u64 = 1 << 20;

/// Whether `name` is a tar this reads, plain or gzipped.
pub fn is_tar(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".tar") || name.ends_with(".tar.gz") || name.ends_with(".tgz")
}

/// The archive's bytes, ungzipped where its name says so.
fn open(archive: &Path) -> Result<Box<dyn Read>, Error> {
    let file = File::open(archive)?;
    let name = archive.to_string_lossy().to_ascii_lowercase();
    Ok(if name.ends_with(".gz") || name.ends_with(".tgz") {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    })
}

/// One entry as its header says it.
struct Header {
    name: String,
    size: u64,
    kind: u8,
}

/// A number in a header: octal text, or base-256 where its first byte's high bit is set.
fn number(field: &[u8]) -> Result<u64, Error> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        let mut n: u64 = 0;
        for (i, &b) in field.iter().enumerate() {
            let b = if i == 0 { b & 0x7f } else { b };
            n = n
                .checked_mul(256)
                .and_then(|n| n.checked_add(u64::from(b)))
                .ok_or_else(|| Error::Unreadable("a size too large to read".into()))?;
        }
        return Ok(n);
    }
    let text: String = field
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8)
        .map_err(|_| Error::Unreadable("a header's number is not one".into()))
}

/// A header's text field, up to its first NUL.
fn text(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(field.get(..end).unwrap_or_default()).into_owned()
}

/// The next header, or `None` at the archive's end (a block of zeros, or no more bytes).
fn next_header(reader: &mut dyn Read) -> Result<Option<Header>, Error> {
    let mut block = [0u8; BLOCK];
    let mut got = 0;
    while got < BLOCK {
        let n = reader
            .read(block.get_mut(got..).unwrap_or_default())
            .map_err(|e| Error::Unreadable(e.to_string()))?;
        if n == 0 {
            break;
        }
        got += n;
    }
    if got == 0 || block.iter().all(|&b| b == 0) {
        return Ok(None);
    }
    if got < BLOCK {
        return Err(Error::Unreadable(
            "the archive ends in the middle of a header".into(),
        ));
    }
    // The checksum: every byte of the header summed, its own field taken as spaces.
    let stated = number(block.get(148..156).unwrap_or_default())?;
    let sum: u64 = block
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            if (148..156).contains(&i) {
                32
            } else {
                u64::from(b)
            }
        })
        .sum();
    if stated != sum {
        return Err(Error::Unreadable(
            "a header does not add up; this is not a tar".into(),
        ));
    }
    let mut name = text(block.get(0..100).unwrap_or_default());
    // ustar's prefix, before the name.
    if block.get(257..262) == Some(b"ustar".as_slice()) {
        let prefix = text(block.get(345..500).unwrap_or_default());
        if !prefix.is_empty() {
            name = format!("{prefix}/{name}");
        }
    }
    Ok(Some(Header {
        name,
        size: number(block.get(124..136).unwrap_or_default())?,
        kind: block.get(156).copied().unwrap_or(0),
    }))
}

/// Reads `size` bytes (and the padding to the next block) into `sink`, or past them.
fn take(reader: &mut dyn Read, size: u64, sink: &mut dyn Write) -> Result<(), Error> {
    let padded = size.div_ceil(BLOCK as u64) * BLOCK as u64;
    let copied = std::io::copy(&mut reader.take(size), sink)
        .map_err(|e| Error::Unreadable(e.to_string()))?;
    if copied != size {
        return Err(Error::Unreadable(
            "the archive ends in the middle of a file".into(),
        ));
    }
    std::io::copy(&mut reader.take(padded - size), &mut std::io::sink())
        .map_err(|e| Error::Unreadable(e.to_string()))?;
    Ok(())
}

/// The path a pax header gives the next entry, if any.
fn pax_path(text: &str) -> Option<String> {
    let mut rest = text;
    while !rest.is_empty() {
        let (len, after) = rest.split_once(' ')?;
        let len: usize = len.parse().ok()?;
        let record = rest.get(..len)?;
        let (key, value) = record
            .get(record.len() - after.len()..)?
            .trim_end_matches('\n')
            .split_once('=')?;
        if key == "path" {
            return Some(value.to_string());
        }
        rest = rest.get(len..)?;
    }
    None
}

/// Each entry in order: its name (after any pax or GNU long name), size and kind, the
/// metadata entries read and gone. `each` is handed the reader positioned at its data, which
/// it must read or skip whole.
fn walk(
    reader: &mut dyn Read,
    mut each: impl FnMut(&str, u64, u8, &mut dyn Read) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut long_name: Option<String> = None;
    while let Some(header) = next_header(reader)? {
        match header.kind {
            // A pax header (`x`) or a GNU long name (`L`): the next entry's name.
            b'x' | b'L' => {
                if header.size > META_MOST {
                    return Err(Error::Unreadable("a header too large to be one".into()));
                }
                let mut meta = Vec::new();
                take(reader, header.size, &mut meta)?;
                let text = String::from_utf8_lossy(&meta).into_owned();
                long_name = if header.kind == b'x' {
                    pax_path(&text)
                } else {
                    Some(text.trim_end_matches('\0').to_string())
                };
            }
            // A pax header for the whole archive: nothing dm needs.
            b'g' => take(reader, header.size, &mut std::io::sink())?,
            kind => {
                let name = long_name.take().unwrap_or(header.name);
                each(&name, header.size, kind, reader)?;
            }
        }
    }
    Ok(())
}

/// Extracts the tar at `archive` into `folder`, which must not exist yet, as
/// `super::extract_into` does a ZIP.
pub(super) fn extract_into(
    archive: &Path,
    folder: &Path,
    limits: &Limits,
    cancel: &AtomicBool,
    origin: Option<&str>,
    progress: &dyn Fn(u64, u64),
) -> Result<Extracted, Error> {
    // First pass: every entry judged before anything is written.
    let mut plan: Vec<(PathBuf, bool, u64)> = Vec::new();
    let (mut total, mut seen) = (0u64, HashMap::new());
    walk(&mut *open(archive)?, |name, size, kind, reader| {
        let shown: String = name.chars().take(120).collect();
        let dir = match kind {
            b'0' | 0 | b'7' => false,
            b'5' => true,
            _ => return Err(Error::Special(shown)),
        };
        let path = safe_path(name).ok_or_else(|| Error::Escapes(shown.clone()))?;
        // A folder named twice is the same folder; any other repeat, in any case, is refused
        // here, a file and a folder of one name included, before anything is written.
        let key = path.to_string_lossy().to_lowercase();
        if seen
            .insert(key, dir)
            .is_some_and(|was_dir| !(was_dir && dir))
        {
            return Err(Error::Duplicate(shown));
        }
        if plan.len() >= limits.max_entries {
            return Err(Error::TooBig("too many entries"));
        }
        total = total.saturating_add(size);
        if total > limits.max_bytes {
            return Err(Error::TooBig("too many bytes"));
        }
        plan.push((path, dir, size));
        take(reader, size, &mut std::io::sink())
    })?;
    if let Some(shown) = super::inside_a_file(plan.iter().map(|(p, _, _)| p.as_path()), &seen) {
        return Err(Error::Duplicate(shown));
    }
    let stored = fs::metadata(archive)?.len();
    if total > limits.ratio_floor && total / stored.max(1) > limits.max_ratio {
        return Err(Error::TooBig("it compresses far beyond anything real"));
    }

    fs::create_dir(folder)?;
    let made = write_all(
        archive, folder, &plan, total, limits, cancel, origin, progress,
    );
    match made {
        Ok(files) => Ok(Extracted {
            folder: folder.to_path_buf(),
            files,
            bytes: total,
        }),
        Err(e) => {
            let _ = fs::remove_dir_all(folder);
            Err(e)
        }
    }
}

/// The second pass: the entries judged, written in their order.
#[allow(clippy::too_many_arguments)]
fn write_all(
    archive: &Path,
    folder: &Path,
    plan: &[(PathBuf, bool, u64)],
    expected: u64,
    limits: &Limits,
    cancel: &AtomicBool,
    origin: Option<&str>,
    progress: &dyn Fn(u64, u64),
) -> Result<usize, Error> {
    let (mut done, mut files, mut at) = (0u64, 0usize, 0usize);
    progress(0, expected);
    walk(&mut *open(archive)?, |_, size, _, reader| {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        // The archive read the first time is the one read now: the same entries, in order.
        let Some((path, dir, declared)) = plan.get(at) else {
            return Err(Error::Unreadable(
                "the archive changed while it was read".into(),
            ));
        };
        at += 1;
        if size != *declared {
            return Err(Error::Unreadable(
                "the archive changed while it was read".into(),
            ));
        }
        let target = folder.join(path);
        if *dir {
            fs::create_dir_all(&target)?;
            return take(reader, size, &mut std::io::sink());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // `create_new`: never replaces, never follows a link that is there.
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        if done + size > limits.max_bytes {
            return Err(Error::TooBig("too many bytes"));
        }
        take(reader, size, &mut out)?;
        out.flush()?;
        drop(out);
        done += size;
        files += 1;
        progress(done, expected);
        if let Some(url) = origin {
            crate::motw::mark(&target, url, "");
        }
        Ok(())
    })?;
    Ok(files)
}

#[cfg(test)]
mod tests;
