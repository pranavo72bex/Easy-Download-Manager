// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A fragmented MP4 whose samples are scrambled by Common Encryption's `cbcs` scheme, which is
//! HLS's SAMPLE-AES for fragmented MP4 (gaps row Y16), put in the clear where it lies.
//!
//! Each track's protection is read from its sample entry (`encv` or `enca`, whose `sinf` names
//! the original format, the scheme and, in `tenc`, the pattern and the vector); each fragment's
//! samples from its `trun`s, and how each is scrambled from its `senc` (its vector, and which
//! bytes of it are in the clear). The protected bytes of each subsample are one chain from the
//! vector, a block scrambled then `skip` left (1 and 9 for a picture; every block for sound),
//! anything past the last whole block in the clear. A sample keeps its length, so the file is
//! put right in place: the samples, and each sample entry named for its original format, its
//! `sinf` made a `free` box that every reader steps over.

use super::*;
use crate::sealed::{Chain, Locks};

/// How a track's samples are scrambled.
#[derive(Debug, Clone, Copy)]
struct Protection {
    crypt: u8,
    skip: u8,
    /// Bytes of each sample's own vector in `senc`; 0 where every sample takes `constant`.
    iv_size: u8,
    constant: Option<[u8; 16]>,
}

/// An index's protected tracks, by id, and where in the file each sample entry's type and its
/// `sinf`'s type are, with the original format the entry is to be named for.
struct Index {
    moov: Vec<u8>,
    tracks: Vec<(u32, Protection)>,
    renames: Vec<(u64, [u8; 4])>,
}

/// Puts the `cbcs`-scrambled fragmented MP4 at `path` in the clear, in place, each sample with
/// the key of the segment it came in (`locks`, by where each segment starts in the file).
/// A sample in a segment with no key is left as it is. An error where the file is scrambled
/// some other way, or says too little to put it right.
pub fn clear_cbcs(path: &Path, locks: &Locks) -> Result<(), Error> {
    let failed = |e: std::io::Error| format!("{} could not be read: {e}", path.display());
    let mut file = File::options()
        .read(true)
        .write(true)
        .open(path)
        .map_err(failed)?;
    let length = file.metadata().map_err(failed)?.len();
    let mut index: Option<Index> = None;
    let mut renames = Vec::new();
    let mut at = 0u64;
    while at + 8 <= length {
        let mut header = [0u8; 16];
        file.seek(SeekFrom::Start(at)).map_err(failed)?;
        file.read_exact(&mut header[..8]).map_err(failed)?;
        let (size, header_len) =
            match u32::from_be_bytes([header[0], header[1], header[2], header[3]]) {
                0 => (length - at, 8u64),
                1 => {
                    file.read_exact(&mut header[8..16]).map_err(failed)?;
                    (u64_at(&header, 8).unwrap_or(0), 16)
                }
                n => (u64::from(n), 8),
            };
        let Some(end) = at
            .checked_add(size)
            .filter(|e| *e <= length && size >= header_len)
        else {
            break;
        };
        let kind = [header[4], header[5], header[6], header[7]];
        if &kind == b"moov" || &kind == b"moof" {
            if size > 512 << 20 {
                return Err("its index is too large".into());
            }
            let mut body = vec![0u8; (size - header_len) as usize];
            file.read_exact(&mut body).map_err(failed)?;
            if &kind == b"moov" {
                let read = protection_of(body, at + header_len)?;
                renames.extend_from_slice(&read.renames);
                index = Some(read);
            } else if let Some(index) = &index {
                clear_fragment(&mut file, at, &body, index, locks)?;
            }
        }
        at = end;
    }
    // Each entry named for what it holds, its `sinf` stepped over.
    for (offset, name) in renames {
        file.seek(SeekFrom::Start(offset)).map_err(failed)?;
        file.write_all(&name).map_err(failed)?;
    }
    file.sync_all().map_err(|e| Error::from(failed(e)))
}

/// Where `part` starts within `whole`, which holds it.
fn offset_in(whole: &[u8], part: &[u8]) -> u64 {
    (part.as_ptr() as usize).saturating_sub(whole.as_ptr() as usize) as u64
}

/// An index's protection, its body read from `body_at` in the file.
fn protection_of(moov: Vec<u8>, body_at: u64) -> Result<Index, Error> {
    let mut tracks = Vec::new();
    let mut renames = Vec::new();
    for (kind, trak, _) in boxes(&moov) {
        if &kind != b"trak" {
            continue;
        }
        let tkhd = child(trak, b"tkhd").ok_or("a track has no header")?;
        let id = match tkhd.first() {
            Some(1) => u32_at(tkhd, 20),
            _ => u32_at(tkhd, 12),
        }
        .ok_or("a track header is short")?;
        let Some(stsd) = child(trak, b"mdia")
            .and_then(|m| child(m, b"minf"))
            .and_then(|m| child(m, b"stbl"))
            .and_then(|s| child_whole(s, b"stsd"))
        else {
            continue;
        };
        for entry in runs::entries_of(stsd) {
            let Some(kind) = entry.get(4..8) else {
                continue;
            };
            let fixed = match kind {
                b"encv" => 8 + 78,
                b"enca" => match u16_at(entry, 16) {
                    Some(1) => 8 + 44,
                    Some(2) => 8 + 64,
                    _ => 8 + 28,
                },
                _ => continue,
            };
            let children = entry.get(fixed..).unwrap_or_default();
            let (_, sinf, sinf_whole) = boxes(children)
                .find(|(k, _, _)| k == b"sinf")
                .ok_or("a scrambled track says nothing of how")?;
            let original: [u8; 4] = child(sinf, b"frma")
                .and_then(|f| f.get(..4))
                .and_then(|f| f.try_into().ok())
                .ok_or("a scrambled track does not say what it holds")?;
            let scheme = child(sinf, b"schm").and_then(|s| s.get(4..8));
            if scheme != Some(b"cbcs") {
                let named = scheme.map(String::from_utf8_lossy).unwrap_or_default();
                return Err(format!(
                    "its samples are scrambled by the \"{named}\" scheme, which dm does not clear"
                )
                .into());
            }
            let tenc = child(sinf, b"schi")
                .and_then(|s| child(s, b"tenc"))
                .ok_or("a scrambled track has no key information")?;
            tracks.push((id, protection(tenc)?));
            renames.push((body_at + offset_in(&moov, entry) + 4, original));
            renames.push((body_at + offset_in(&moov, sinf_whole) + 4, *b"free"));
        }
    }
    Ok(Index {
        moov,
        tracks,
        renames,
    })
}

/// A `tenc`'s pattern and vectors.
fn protection(tenc: &[u8]) -> Result<Protection, Error> {
    let short = || Error::from("a track's key information is short");
    let version = *tenc.first().ok_or_else(short)?;
    let pattern = if version == 0 {
        0
    } else {
        *tenc.get(5).ok_or_else(short)?
    };
    let protected = *tenc.get(6).ok_or_else(short)?;
    let iv_size = *tenc.get(7).ok_or_else(short)?;
    let constant = if protected == 1 && iv_size == 0 {
        let size = usize::from(*tenc.get(24).ok_or_else(short)?);
        let iv = tenc.get(25..25 + size).ok_or_else(short)?;
        Some(padded(iv))
    } else {
        None
    };
    if !matches!(iv_size, 0 | 8 | 16) {
        return Err(short());
    }
    Ok(Protection {
        crypt: pattern >> 4,
        skip: pattern & 0x0f,
        iv_size,
        constant,
    })
}

/// A vector of 8 or 16 bytes as 16: the rest nought.
fn padded(iv: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (slot, byte) in out.iter_mut().zip(iv) {
        *slot = *byte;
    }
    out
}

/// How one sample is scrambled: its vector, if its own, and its subsamples, bytes in the clear
/// and bytes protected, if it says.
type Scrambled = (Option<[u8; 16]>, Vec<(u32, u32)>);

/// A `senc`'s entries, each sample's vector of `iv_size` bytes and its subsamples.
fn sample_encryption(senc: &[u8], iv_size: u8) -> Result<Vec<Scrambled>, Error> {
    let short = || Error::from("a fragment's encryption map is short");
    let flags = u32_at(senc, 0).ok_or_else(short)? & 0x00ff_ffff;
    let count = u32_at(senc, 4).ok_or_else(short)? as usize;
    let mut at = 8usize;
    let mut out = Vec::with_capacity(count.min(1 << 16));
    for _ in 0..count {
        let iv = if iv_size > 0 {
            let iv = senc.get(at..at + usize::from(iv_size)).ok_or_else(short)?;
            at += usize::from(iv_size);
            Some(padded(iv))
        } else {
            None
        };
        let mut subsamples = Vec::new();
        if flags & 2 != 0 {
            let n = u16_at(senc, at).ok_or_else(short)?;
            at += 2;
            for _ in 0..n {
                let clear = u16_at(senc, at).ok_or_else(short)?;
                let protected = u32_at(senc, at + 2).ok_or_else(short)?;
                subsamples.push((u32::from(clear), protected));
                at += 6;
            }
        }
        out.push((iv, subsamples));
    }
    Ok(out)
}

/// One fragment's samples put in the clear in the file.
fn clear_fragment(
    file: &mut File,
    moof_at: u64,
    moof: &[u8],
    index: &Index,
    locks: &Locks,
) -> Result<(), Error> {
    let mut tracks = tracks_of(&index.moov, 0)?;
    for track in &mut tracks {
        track.samples.clear();
    }
    read_fragment(moof_at, moof, &trex_defaults(&index.moov), &mut tracks)?;
    // Each track's samples' encryption, from its `traf`s in order.
    let mut maps: Vec<(u32, Vec<Scrambled>, bool)> = Vec::new();
    for (kind, traf, _) in boxes(moof) {
        if &kind != b"traf" {
            continue;
        }
        let id = child(traf, b"tfhd")
            .and_then(|t| u32_at(t, 4))
            .ok_or("a fragment is short")?;
        let Some((_, protection)) = index.tracks.iter().find(|(t, _)| *t == id) else {
            continue;
        };
        // A group of samples with keys of their own: not what HLS sends; refused, not guessed.
        if boxes(traf).any(|(k, b, _)| &k == b"sbgp" && b.get(4..8) == Some(b"seig")) {
            return Err(
                "its samples change keys within a fragment, which dm does not clear".into(),
            );
        }
        let map = match child(traf, b"senc") {
            Some(senc) => sample_encryption(senc, protection.iv_size)?,
            None if protection.iv_size == 0 => Vec::new(),
            None => return Err("a scrambled fragment has no encryption map".into()),
        };
        match maps.iter_mut().find(|(t, _, _)| *t == id) {
            Some((_, all, _)) => all.extend(map),
            None => maps.push((id, map, child(traf, b"senc").is_some())),
        }
    }
    for (id, map, mapped) in maps {
        let (Some(track), Some((_, protection))) = (
            tracks.iter().find(|t| t.id == id),
            index.tracks.iter().find(|(t, _)| *t == id),
        ) else {
            continue;
        };
        if mapped && map.len() != track.samples.len() {
            return Err("a fragment's encryption map does not match its samples".into());
        }
        clear_samples(file, &track.samples, &map, protection, locks)?;
    }
    Ok(())
}

/// A fragment's samples of one track, read together, put in the clear and written back.
fn clear_samples(
    file: &mut File,
    samples: &[Sample],
    map: &[Scrambled],
    protection: &Protection,
    locks: &Locks,
) -> Result<(), Error> {
    let (Some(first), Some(end)) = (
        samples.iter().map(|s| s.offset).min(),
        samples.iter().map(|s| s.offset + u64::from(s.size)).max(),
    ) else {
        return Ok(());
    };
    if end - first > 512 << 20 {
        return Err("a fragment is too large".into());
    }
    let mut data = vec![0u8; (end - first) as usize];
    let io = |e: std::io::Error| format!("a scrambled sample could not be put right: {e}");
    file.seek(SeekFrom::Start(first)).map_err(io)?;
    file.read_exact(&mut data).map_err(io)?;
    let mut changed = false;
    for (i, sample) in samples.iter().enumerate() {
        let Some((key, playlist_iv)) = locks.at(sample.offset) else {
            continue;
        };
        let (own, subsamples) = map.get(i).cloned().unwrap_or_default();
        let iv = own.or(protection.constant).unwrap_or(playlist_iv);
        let from = (sample.offset - first) as usize;
        let Some(bytes) = data.get_mut(from..from + sample.size as usize) else {
            continue;
        };
        let whole = [(0, sample.size)];
        let ranges: &[(u32, u32)] = if subsamples.is_empty() {
            &whole
        } else {
            &subsamples
        };
        let mut at = 0usize;
        for (clear, protected) in ranges {
            at += *clear as usize;
            let Some(run) = bytes.get_mut(at..at + *protected as usize) else {
                return Err("a sample's encryption map runs past it".into());
            };
            clear_run(run, &key, &iv, protection.crypt, protection.skip)?;
            at += *protected as usize;
        }
        changed = true;
    }
    if changed {
        file.seek(SeekFrom::Start(first)).map_err(io)?;
        file.write_all(&data).map_err(io)?;
    }
    Ok(())
}

/// One protected run, a chain from `iv`: `crypt` blocks scrambled then `skip` in the clear, over
/// and over (every block where both are nought); a partial block at the end in the clear.
fn clear_run(
    run: &mut [u8],
    key: &[u8; 16],
    iv: &[u8; 16],
    crypt: u8,
    skip: u8,
) -> Result<(), Error> {
    let mut chain = Chain::new(key, iv).ok_or("a stream key is not usable")?;
    if crypt == 0 && skip == 0 {
        chain.blocks(run);
        return Ok(());
    }
    let (crypt, skip) = (usize::from(crypt) * 16, usize::from(skip) * 16);
    let mut at = 0;
    while at < run.len() {
        let take = crypt.min(run.len() - at);
        if let Some(blocks) = run.get_mut(at..at + take) {
            chain.blocks(blocks);
        }
        at += take + skip;
    }
    Ok(())
}
