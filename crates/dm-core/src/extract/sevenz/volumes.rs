// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A 7z in parts (`film.7z.001`, `film.7z.002`…, gaps row C3b): the parts read in order as one
//! file, through a reader that reads and seeks across them, so they are never joined on disk.
//! A 7z cut into parts is the archive's bytes cut where the parts end; nothing is added.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The most parts read as one archive.
const MOST_PARTS: usize = 10_000;

/// The archive at `path` as one file: its parts in order where `path` is a first part
/// (`.7z.001`), else the file alone.
pub(super) struct Joined {
    /// Each part, where it begins in the whole and how long it is.
    parts: Vec<(File, u64, u64)>,
    /// Where the next read begins, in the whole.
    at: u64,
    /// How long the whole is.
    len: u64,
}

impl Joined {
    /// How long the archive is, all its parts together.
    pub(super) fn len(&self) -> u64 {
        self.len
    }
}

/// Opens the archive at `path`: every part, from `.001` on until the next is not there, where
/// it is a first part; the file alone otherwise.
pub(super) fn open(path: &Path) -> std::io::Result<Joined> {
    let mut parts = Vec::new();
    let mut len = 0u64;
    for part in part_paths(path) {
        let file = File::open(&part)?;
        let size = file.metadata()?.len();
        parts.push((file, len, size));
        len = len.saturating_add(size);
    }
    Ok(Joined { parts, at: 0, len })
}

/// The paths of `path`'s parts in order: `path` alone where it is no first part.
fn part_paths(path: &Path) -> Vec<PathBuf> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(stem) = name
        .len()
        .checked_sub(4)
        .filter(|&at| name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".001"))
        .map(|at| name[..at].to_string())
        .filter(|stem| stem.to_ascii_lowercase().ends_with(".7z"))
    else {
        return vec![path.to_path_buf()];
    };
    let mut paths = vec![path.to_path_buf()];
    for number in 2..=MOST_PARTS {
        let next = path.with_file_name(format!("{stem}.{number:03}"));
        if !next.is_file() {
            break;
        }
        paths.push(next);
    }
    paths
}

impl Read for Joined {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let at = self.at;
        let Some((file, start, size)) = self
            .parts
            .iter_mut()
            .find(|(_, start, size)| at >= *start && at < start.saturating_add(*size))
        else {
            return Ok(0);
        };
        let within = at - *start;
        file.seek(SeekFrom::Start(within))?;
        let left = usize::try_from(*size - within).unwrap_or(usize::MAX);
        let want = buf.len().min(left);
        let n = file.read(&mut buf[..want])?;
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Joined {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let next = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.at.checked_add_signed(d),
        };
        let next = next.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "a seek before the start")
        })?;
        self.at = next;
        Ok(next)
    }
}
