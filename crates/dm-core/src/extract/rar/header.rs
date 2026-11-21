// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A RAR 5 archive's list of what it holds, read from the format's public description (the
//! technical note on the RAR 5.0 archive format its maker publishes): the signature, then
//! blocks, each a CRC32, a size and a type, its fields, an optional extra area of records and
//! an optional data area. Only the headers are read here; a file's packed bytes are left where
//! they are and found again by their offset. Every count is outside data and is checked.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::super::{plain_mode, Error};

/// The first eight bytes of a RAR 5 archive.
pub(super) const SIGNATURE: [u8; 8] = *b"Rar!\x1a\x07\x01\x00";
/// The first seven bytes of a RAR archive of versions 1.5 to 4.x.
pub(super) const OLD_SIGNATURE: [u8; 7] = *b"Rar!\x1a\x07\x00";

/// The format's own limit on one header.
const HEADER_MOST: u64 = 2 << 20;

/// Block types.
const MAIN: u64 = 1;
const FILE: u64 = 2;
const CRYPT: u64 = 4;
const END: u64 = 5;

/// A Windows attribute: the entry is a reparse point (a link or junction).
const REPARSE_POINT: u64 = 0x400;

/// A damaged header, said plainly.
pub(super) fn damaged(what: &str) -> Error {
    Error::Unreadable(format!("the RAR archive is damaged ({what})"))
}

/// An archive in several parts, which dm does not join.
pub(super) fn in_parts() -> Error {
    Error::Unreadable(
        "the RAR archive is one part of several (.part1.rar, .part2.rar…), \
         which dm does not join"
            .into(),
    )
}

/// An archive that ends before its headers do.
pub(super) fn cut_short() -> Error {
    Error::Unreadable("the RAR archive is cut short".into())
}

/// How a file's bytes are packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pack {
    /// Stored as they are.
    Stored,
    /// RAR 5's compression (methods 1 to 5 differ only in effort); `solid` continues the
    /// stream of the compressed file before it; `dict` is the dictionary it needs, in bytes.
    Lz { solid: bool, dict: u64 },
    /// RAR 4's compression (unpack version 29), LZ or PPMd; `solid` and `dict` as for RAR 5.
    Rar4 { solid: bool, dict: u64 },
    /// Something dm does not unpack, named for [`Error::Method`].
    Refused(String),
}

/// One file or folder the archive holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    /// Its name, with either slash between folders.
    pub name: String,
    pub dir: bool,
    /// Bytes once unpacked, as the header says.
    pub size: u64,
    /// Where its packed bytes start in the archive, and how many there are.
    pub offset: u64,
    pub packed: u64,
    /// The CRC32 of its unpacked bytes, where it has one.
    pub crc: Option<u32>,
    pub pack: Pack,
    /// Locked with a password.
    pub encrypted: bool,
    /// A link, a junction, a device or another special file.
    pub special: bool,
    /// Checked by a BLAKE2 hash, which dm does not compute, instead of a CRC32.
    pub blake2: bool,
}

/// Bytes read from the front, every read checked.
pub(super) struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Cursor<'a> {
        Cursor { bytes, at: 0 }
    }

    pub(super) fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    pub(super) fn at(&self) -> usize {
        self.at
    }

    pub(super) fn take(&mut self, n: u64) -> Result<&'a [u8], Error> {
        let n = usize::try_from(n).map_err(|_| damaged("a field runs past its header"))?;
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| damaged("a field runs past its header"))?;
        let got = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| damaged("a field runs past its header"))?;
        self.at = end;
        Ok(got)
    }

    pub(super) fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?.first().copied().unwrap_or(0))
    }

    pub(super) fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(super) fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A variable-length number: seven bits a byte, the lowest first, the top bit set on
    /// every byte but the last; at most ten bytes.
    pub(super) fn vint(&mut self) -> Result<u64, Error> {
        let mut value = 0u64;
        for i in 0..10 {
            let b = self.byte()?;
            value |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(damaged("a number is too long"))
    }
}

/// One block's header: its type, its flags, the bytes of its own fields (after the common
/// ones), its extra area, and where its data area is.
struct Block {
    kind: u64,
    flags: u64,
    fields: Vec<u8>,
    extra: Vec<u8>,
    data_at: u64,
    data_size: u64,
}

/// Reads the block at `at`, its CRC checked.
fn block(file: &mut File, at: u64) -> Result<Block, Error> {
    file.seek(SeekFrom::Start(at))?;
    // A CRC32, then the header's size, at most three bytes for at most 2 MiB; the smallest
    // header (a type and its flags) makes seven bytes in all.
    let mut head = [0u8; 7];
    file.read_exact(&mut head).map_err(|_| cut_short())?;
    let crc = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let mut c = Cursor::new(&head[4..]);
    let size = c.vint().map_err(|_| damaged("a header's size"))?;
    let size_len = c.at();
    if !(2..=HEADER_MOST).contains(&size) {
        return Err(damaged("a header's size"));
    }
    // What the CRC covers: the size and the header after it.
    let hashed_len = size_len as u64 + size;
    let mut hashed = head[4..].to_vec();
    if hashed_len > 3 {
        let mut rest = vec![0u8; (hashed_len - 3) as usize];
        file.read_exact(&mut rest).map_err(|_| cut_short())?;
        hashed.extend_from_slice(&rest);
    }
    if crc32fast::hash(&hashed) != crc {
        return Err(damaged("a header does not add up"));
    }
    let body = hashed.get(size_len..).unwrap_or_default();
    let mut c = Cursor::new(body);
    let kind = c.vint()?;
    let flags = c.vint()?;
    let extra_size = if flags & 0x01 != 0 { c.vint()? } else { 0 };
    let data_size = if flags & 0x02 != 0 { c.vint()? } else { 0 };
    let fields_end = usize::try_from(extra_size)
        .ok()
        .and_then(|e| body.len().checked_sub(e))
        .filter(|&end| end >= c.at())
        .ok_or_else(|| damaged("a header's extra area"))?;
    Ok(Block {
        kind,
        flags,
        fields: body.get(c.at()..fields_end).unwrap_or_default().to_vec(),
        extra: body.get(fields_end..).unwrap_or_default().to_vec(),
        data_at: at + 4 + hashed_len,
        data_size,
    })
}

/// Reads the list of a RAR 5 archive at `path` (its signature already checked), at most
/// `max_entries` files and folders.
pub(super) fn read(path: &Path, max_entries: usize) -> Result<Vec<Entry>, Error> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut entries = Vec::new();
    let mut at = SIGNATURE.len() as u64;
    // An archive whose end block is missing ends where its last block does.
    while at < file_len {
        let b = block(&mut file, at)?;
        // Continued from, or into, another part.
        if b.flags & (0x08 | 0x10) != 0 {
            return Err(in_parts());
        }
        let next = b
            .data_at
            .checked_add(b.data_size)
            .ok_or_else(|| damaged("a block's size"))?;
        if next > file_len {
            return Err(cut_short());
        }
        match b.kind {
            MAIN => {
                // Its first flag: the archive is one volume of several.
                if Cursor::new(&b.fields).vint()? & 0x01 != 0 {
                    return Err(in_parts());
                }
            }
            // Every header after this one is encrypted.
            CRYPT => return Err(Error::Encrypted("its list of files".into())),
            FILE => {
                if entries.len() >= max_entries {
                    return Err(Error::TooBig("too many entries"));
                }
                entries.push(file_entry(&b)?);
            }
            END => {
                // Its first flag: another volume follows.
                if Cursor::new(&b.fields).vint()? & 0x01 != 0 {
                    return Err(in_parts());
                }
                break;
            }
            // A service header (a comment, a recovery record, a quick-open copy of the
            // list) or a type of a later version: passed over with its data.
            _ => {}
        }
        at = next;
    }
    Ok(entries)
}

/// A file header's fields and extra records.
fn file_entry(b: &Block) -> Result<Entry, Error> {
    let mut c = Cursor::new(&b.fields);
    let file_flags = c.vint()?;
    let size = c.vint()?;
    let attributes = c.vint()?;
    if file_flags & 0x02 != 0 {
        // The time it was changed: dm does not set it on what it extracts (nor a ZIP's,
        // a 7z's or a tar's).
        c.u32()?;
    }
    let crc = if file_flags & 0x04 != 0 {
        Some(c.u32()?)
    } else {
        None
    };
    let info = c.vint()?;
    let host = c.vint()?;
    let name_len = c.vint()?;
    let name = std::str::from_utf8(c.take(name_len)?)
        .map_err(|_| Error::Unreadable("an entry has a name that cannot be read".into()))?
        .to_string();
    let dir = file_flags & 0x01 != 0;
    let pack = if file_flags & 0x08 != 0 && !dir {
        // Its size was not known when it was packed (it came from a pipe).
        Pack::Refused("a file of unknown size".into())
    } else {
        pack_of(info)
    };
    let special = match host {
        // Windows: a reparse point.
        0 => attributes & REPARSE_POINT != 0,
        // Unix: a mode that is not a file or a folder.
        1 => !plain_mode(Some(u32::try_from(attributes).unwrap_or(u32::MAX))),
        _ => false,
    };
    let mut entry = Entry {
        name,
        dir,
        size,
        offset: b.data_at,
        packed: b.data_size,
        crc,
        pack,
        encrypted: false,
        special,
        blake2: false,
    };
    extras(&b.extra, &mut entry)?;
    Ok(entry)
}

/// What a file's compression information says: its algorithm's version (bits 0 to 5), solid
/// (bit 6), its method (bits 7 to 9, 0 stored) and its dictionary (bits 10 to 13, 128 KiB
/// times a power of two).
fn pack_of(info: u64) -> Pack {
    let version = info & 0x3f;
    let method = (info >> 7) & 0x07;
    if method == 0 {
        return Pack::Stored;
    }
    if version != 0 {
        return Pack::Refused(format!("a later RAR compression (version {version})"));
    }
    if method > 5 {
        return Pack::Refused(format!("an unknown RAR method ({method})"));
    }
    Pack::Lz {
        solid: info & 0x40 != 0,
        dict: (128 << 10) << ((info >> 10) & 0x0f),
    }
}

/// A file header's extra records: each a size, a type and its data.
fn extras(extra: &[u8], entry: &mut Entry) -> Result<(), Error> {
    let mut c = Cursor::new(extra);
    while !c.done() {
        let size = c.vint()?;
        let mut r = Cursor::new(c.take(size)?);
        match r.vint()? {
            // File encryption.
            0x01 => entry.encrypted = true,
            // A hash; its only kind is BLAKE2sp.
            0x02 => entry.blake2 = true,
            // A link, a junction, a hard link or a copy of another file.
            0x05 => entry.special = true,
            // Times, a version, Unix owners, service data: nothing dm uses.
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_vint_is_seven_bits_a_byte_lowest_first() {
        let mut c = Cursor::new(&[0x05, 0x80, 0x01, 0xff, 0x7f, 0x80]);
        assert_eq!(c.vint().unwrap(), 5);
        assert_eq!(c.vint().unwrap(), 128);
        assert_eq!(c.vint().unwrap(), 0x3fff);
        // Cut short in the middle of a number.
        assert!(c.vint().is_err());
        let too_long = [0x80u8; 11];
        assert!(Cursor::new(&too_long).vint().is_err());
    }

    #[test]
    fn compression_information_is_read_by_its_bits() {
        assert_eq!(pack_of(0), Pack::Stored);
        // Method 3, solid, dictionary 128 KiB << 5 = 4 MiB.
        let info = (3 << 7) | 0x40 | (5 << 10);
        assert_eq!(
            pack_of(info),
            Pack::Lz {
                solid: true,
                dict: 4 << 20
            }
        );
        assert!(matches!(pack_of((1 << 7) | 1), Pack::Refused(_)));
        assert!(matches!(pack_of(6 << 7), Pack::Refused(_)));
    }
}
