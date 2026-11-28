// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list of a RAR archive of versions 1.5 to 4.x, read from the format's public
//! description: blocks of a 16-bit CRC, a type, flags and a size, a file block followed by
//! its packed bytes. EDM unpacks the files such an archive stores as they are, and those
//! packed by RAR 2.9 to 4.x (unpack version 29, `v3.rs`); the compression of RAR 2.x and
//! older is refused in words, by the file it names, before anything is written.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::super::{plain_mode, Error};
use super::header::{cut_short, damaged, in_parts, Cursor, Entry, Pack, OLD_SIGNATURE};

/// Block types.
const MAIN: u8 = 0x73;
const FILE: u8 = 0x74;
const END: u8 = 0x7b;

/// Its method byte for a file stored as it is.
const STORED: u8 = 0x30;
/// The unpack version of RAR 2.9 to 4.x's compression.
const VERSION_29: u8 = 29;

/// Reads the list of the old-format RAR archive at `path` (its signature already checked).
pub(super) fn read(path: &Path, max_entries: usize) -> Result<Vec<Entry>, Error> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut entries = Vec::new();
    let mut at = OLD_SIGNATURE.len() as u64;
    while at < file_len {
        file.seek(SeekFrom::Start(at))?;
        let mut head = [0u8; 7];
        file.read_exact(&mut head).map_err(|_| cut_short())?;
        let crc = u16::from_le_bytes([head[0], head[1]]);
        let kind = head[2];
        let flags = u16::from_le_bytes([head[3], head[4]]);
        let size = u16::from_le_bytes([head[5], head[6]]);
        if size < 7 {
            return Err(damaged("a header's size"));
        }
        let mut whole = head.to_vec();
        let mut rest = vec![0u8; usize::from(size) - 7];
        file.read_exact(&mut rest).map_err(|_| cut_short())?;
        whole.extend_from_slice(&rest);
        // A block with data after it says how much in the four bytes after its size.
        let mut data = if flags & 0x8000 != 0 {
            u64::from(Cursor::new(whole.get(7..).unwrap_or_default()).u32()?)
        } else {
            0
        };
        match kind {
            MAIN => {
                if flags & 0x0001 != 0 {
                    return Err(in_parts());
                }
                // Every header after this one is encrypted.
                if flags & 0x0080 != 0 {
                    return Err(Error::Encrypted("its list of files".into()));
                }
            }
            FILE => {
                // The CRC covers the whole header after itself.
                if (crc32fast::hash(whole.get(2..).unwrap_or_default()) & 0xffff) as u16 != crc {
                    return Err(damaged("a header does not add up"));
                }
                if entries.len() >= max_entries {
                    return Err(Error::TooBig("too many entries"));
                }
                let entry = file_entry(&whole, flags, at + u64::from(size))?;
                data = entry.packed;
                entries.push(entry);
            }
            END => {
                // Another volume follows.
                if flags & 0x0001 != 0 {
                    return Err(in_parts());
                }
                break;
            }
            // A comment, a recovery record, a service block: passed over with its data.
            _ => {}
        }
        at = at
            .checked_add(u64::from(size))
            .and_then(|a| a.checked_add(data))
            .ok_or_else(|| damaged("a block's size"))?;
        if at > file_len {
            return Err(cut_short());
        }
    }
    Ok(entries)
}

/// A file block's fields; its packed bytes start at `data_at`.
fn file_entry(whole: &[u8], flags: u16, data_at: u64) -> Result<Entry, Error> {
    // Continued from, or into, another volume.
    if flags & (0x0001 | 0x0002) != 0 {
        return Err(in_parts());
    }
    let mut c = Cursor::new(whole.get(7..).unwrap_or_default());
    let mut packed = u64::from(c.u32()?);
    let mut size = u64::from(c.u32()?);
    let host = c.byte()?;
    let crc = c.u32()?;
    // The time it was changed, as DOS writes it, and the version needed to unpack it.
    c.u32()?;
    let version = c.byte()?;
    let method = c.byte()?;
    let name_len = c.u16()?;
    let attributes = c.u32()?;
    if flags & 0x0100 != 0 {
        packed |= u64::from(c.u32()?) << 32;
        size |= u64::from(c.u32()?) << 32;
    }
    let raw = c.take(u64::from(name_len))?;
    let name = if flags & 0x0200 != 0 {
        unicode_name(raw)
    } else {
        std::str::from_utf8(raw).ok().map(str::to_string)
    }
    .ok_or_else(|| Error::Unreadable("an entry has a name that cannot be read".into()))?;
    let dir = flags & 0x00e0 == 0x00e0;
    let pack = if dir || method == STORED {
        Pack::Stored
    } else if version == VERSION_29 {
        Pack::Rar4 {
            solid: flags & 0x0010 != 0,
            // 64 KiB times a power of two, in bits 5 to 7.
            dict: (64 << 10) << ((flags >> 5) & 7),
        }
    } else {
        Pack::Refused("the compression of RAR 2.x and older".into())
    };
    let special = match host {
        // MS-DOS, Windows: a reparse point.
        0 | 2 => attributes & 0x400 != 0,
        // Unix: a mode that is not a file or a folder.
        3 => !plain_mode(Some(attributes)),
        _ => false,
    };
    Ok(Entry {
        name,
        dir,
        size,
        spans: vec![super::header::Span {
            volume: 0,
            offset: data_at,
            len: packed,
        }],
        packed,
        crc: (!dir).then_some(crc),
        pack,
        encrypted: flags & 0x0004 != 0,
        special,
        blake2: false,
    })
}

/// A name in the old format's Unicode form: the name in single bytes, a zero byte, then the
/// UTF-16 name coded against it; with no zero byte the whole name is UTF-8.
fn unicode_name(raw: &[u8]) -> Option<String> {
    let Some(zero) = raw.iter().position(|&b| b == 0) else {
        return std::str::from_utf8(raw).ok().map(str::to_string);
    };
    let (narrow, coded) = (raw.get(..zero)?, raw.get(zero + 1..)?);
    let mut out: Vec<u16> = Vec::new();
    let mut c = coded.iter().copied();
    let high = u16::from(c.next().unwrap_or(0));
    let (mut flags, mut flag_bits) = (0u8, 0u32);
    // Four codes to a flag byte, two bits each, the highest first. Codes cut short end the
    // name where they stop.
    loop {
        if flag_bits == 0 {
            let Some(f) = c.next() else {
                break;
            };
            flags = f;
            flag_bits = 8;
        }
        let Some(byte) = c.next() else {
            break;
        };
        match flags >> 6 {
            // A byte as it is, or under the name's high byte.
            0 => out.push(u16::from(byte)),
            1 => out.push(u16::from(byte) | (high << 8)),
            // Two bytes, the lower first.
            2 => {
                let Some(upper) = c.next() else {
                    break;
                };
                out.push(u16::from(byte) | (u16::from(upper) << 8));
            }
            // A run of the narrow name's bytes at the same places, corrected or as they are.
            _ => {
                if byte & 0x80 != 0 {
                    let Some(correction) = c.next() else {
                        break;
                    };
                    for _ in 0..(byte & 0x7f) + 2 {
                        let b = narrow.get(out.len()).copied().unwrap_or(0);
                        out.push(u16::from(b.wrapping_add(correction)) | (high << 8));
                    }
                } else {
                    for _ in 0..u16::from(byte) + 2 {
                        let Some(&b) = narrow.get(out.len()) else {
                            break;
                        };
                        out.push(u16::from(b));
                    }
                }
            }
        }
        flags <<= 2;
        flag_bits -= 2;
    }
    String::from_utf16(&out).ok()
}

#[cfg(test)]
mod tests {
    use super::unicode_name;

    #[test]
    fn an_old_unicode_name_is_decoded_against_its_narrow_form() {
        // "ab?" narrow, then: high byte 0x04, flags 0b11_01_00_00 (the narrow name's first
        // two bytes copied, then one byte under the high byte).
        let mut raw = b"ab?\0".to_vec();
        raw.extend_from_slice(&[0x04, 0b1101_0000, 0x00, 0x10]);
        let got = unicode_name(&raw).unwrap_or_default();
        assert_eq!(got, "ab\u{0410}");
        // No zero byte: the whole name is UTF-8.
        assert_eq!(unicode_name("só.txt".as_bytes()).as_deref(), Some("só.txt"));
    }
}
