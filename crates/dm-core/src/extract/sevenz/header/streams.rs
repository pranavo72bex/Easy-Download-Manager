// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The header's streams: the packed streams' sizes, the folders that unpack them, and the files
//! each folder holds, with their sizes and checksums.

use super::*;

fn pack_info(c: &mut Cursor, s: &mut Streams, max_entries: usize) -> Result<(), Error> {
    s.pack_pos = c.number()?;
    let n = c.count()?;
    // Each folder holds at least one file, so packed streams past the entries allowed are a
    // header made to fill memory (a list of 64 MiB packs into a few KB), not an archive.
    if n > max_entries.saturating_mul(PACKED_PER_FOLDER) {
        return Err(Error::TooBig("too many entries"));
    }
    loop {
        match c.byte()? {
            END => return Ok(()),
            SIZE => {
                s.pack_sizes = (0..n).map(|_| c.number()).collect::<Result<_, _>>()?;
            }
            CRC => {
                // The packed bytes' own checksums; the unpacked ones are checked instead.
                c.digests(n)?;
            }
            _ => return Err(damaged("its packed streams")),
        }
    }
}

fn unpack_info(c: &mut Cursor, s: &mut Streams, max_entries: usize) -> Result<(), Error> {
    if c.byte()? != FOLDER {
        return Err(damaged("its folders"));
    }
    let n = c.count()?;
    // As for packed streams: more folders than entries allowed only fill memory.
    if n > max_entries {
        return Err(Error::TooBig("too many entries"));
    }
    if c.byte()? != 0 {
        return Err(damaged("folders kept elsewhere"));
    }
    s.folders = (0..n).map(|_| folder(c)).collect::<Result<_, _>>()?;
    if c.byte()? != CODERS_UNPACK_SIZE {
        return Err(damaged("its folders' sizes"));
    }
    for f in &mut s.folders {
        f.out_sizes = (0..f.outs).map(|_| c.number()).collect::<Result<_, _>>()?;
    }
    loop {
        match c.byte()? {
            END => return Ok(()),
            CRC => {
                let crcs = c.digests(n)?;
                for (f, crc) in s.folders.iter_mut().zip(crcs) {
                    f.crc = crc;
                }
            }
            _ => return Err(damaged("its folders")),
        }
    }
}

fn substreams_info(c: &mut Cursor, s: &mut Streams, max_entries: usize) -> Result<(), Error> {
    let mut counts = vec![1usize; s.folders.len()];
    let mut ty = c.byte()?;
    if ty == NUM_UNPACK_STREAM {
        let mut total = 0usize;
        for n in &mut counts {
            let v = c.number()?;
            total = total.saturating_add(usize::try_from(v).unwrap_or(usize::MAX));
            if total > max_entries {
                return Err(Error::TooBig("too many entries"));
            }
            *n = v as usize;
        }
        ty = c.byte()?;
    }
    let mut sizes: Vec<Vec<u64>> = Vec::with_capacity(counts.len());
    for (f, &n) in s.folders.iter().zip(&counts) {
        let whole = f.unpack_size();
        let mut these = Vec::new();
        if n == 0 {
            sizes.push(these);
            continue;
        }
        let mut sum = 0u64;
        if ty == SIZE {
            for _ in 1..n {
                let size = c.number()?;
                sum = sum
                    .checked_add(size)
                    .ok_or_else(|| damaged("a stream's size"))?;
                these.push(size);
            }
        } else if n > 1 {
            return Err(damaged("a folder's streams have no sizes"));
        }
        let last = whole
            .checked_sub(sum)
            .ok_or_else(|| damaged("a folder's streams are larger than it"))?;
        these.push(last);
        sizes.push(these);
    }
    if ty == SIZE {
        ty = c.byte()?;
    }
    // Checksums are listed for the streams whose folder does not already give theirs.
    let known = |f: &RawFolder, n: usize| n == 1 && f.crc.is_some();
    let unknown: usize = s
        .folders
        .iter()
        .zip(&counts)
        .filter(|(f, &n)| !known(f, n))
        .map(|(_, &n)| n)
        .sum();
    let mut listed: Vec<Option<u32>> = Vec::new();
    loop {
        match ty {
            END => break,
            CRC => listed = c.digests(unknown)?,
            _ => return Err(damaged("its streams")),
        }
        ty = c.byte()?;
    }
    let mut listed = listed.into_iter();
    let mut out = Vec::with_capacity(s.folders.len());
    for ((f, &n), these) in s.folders.iter().zip(&counts).zip(sizes) {
        let crcs: Vec<Option<u32>> = if known(f, n) {
            vec![f.crc]
        } else {
            (0..n).map(|_| listed.next().flatten()).collect()
        };
        out.push(these.into_iter().zip(crcs).collect());
    }
    s.substreams = Some(out);
    Ok(())
}

pub(super) fn streams_info(c: &mut Cursor, max_entries: usize) -> Result<Streams, Error> {
    let mut s = Streams::default();
    loop {
        match c.byte()? {
            END => return Ok(s),
            PACK_INFO => pack_info(c, &mut s, max_entries)?,
            UNPACK_INFO => unpack_info(c, &mut s, max_entries)?,
            SUBSTREAMS_INFO => substreams_info(c, &mut s, max_entries)?,
            _ => return Err(damaged("its streams")),
        }
    }
}
