// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A 7z archive's list of what it holds, read from the format's public description
//! (`7zFormat.txt` in the public-domain LZMA SDK): the 32-byte signature header, the header it
//! points to (plain, or itself packed), the packed streams, the folders that unpack them, the
//! files those folders hold in order, and each file's name, kind and checksum.
//!
//! Nothing here is trusted: every count is held to what the bytes around it can hold before
//! anything is made that size, every offset is checked against the file, and every checksum
//! the format carries is checked.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::super::Error;

mod streams;

use streams::streams_info;

/// The first six bytes of every 7z archive.
pub(super) const SIGNATURE: [u8; 6] = *b"7z\xBC\xAF\x27\x1C";

/// The most a header may be, packed or not: a list of names, not a file.
pub(super) const HEADER_MOST: u64 = 64 << 20;

/// The most coders, or streams between them, one folder may have.
const CODERS_MOST: u64 = 64;

/// The most packed streams one folder may read (a BCJ2 folder reads four), so the packed
/// streams listed may be at most this many times the entries allowed.
const PACKED_PER_FOLDER: usize = 4;

// The header's property numbers, as the format names them.
const END: u8 = 0x00;
const HEADER: u8 = 0x01;
const ARCHIVE_PROPERTIES: u8 = 0x02;
const ADDITIONAL_STREAMS: u8 = 0x03;
const MAIN_STREAMS: u8 = 0x04;
const FILES_INFO: u8 = 0x05;
const PACK_INFO: u8 = 0x06;
const UNPACK_INFO: u8 = 0x07;
const SUBSTREAMS_INFO: u8 = 0x08;
const SIZE: u8 = 0x09;
const CRC: u8 = 0x0A;
const FOLDER: u8 = 0x0B;
const CODERS_UNPACK_SIZE: u8 = 0x0C;
const NUM_UNPACK_STREAM: u8 = 0x0D;
const EMPTY_STREAM: u64 = 0x0E;
const EMPTY_FILE: u64 = 0x0F;
const ANTI: u64 = 0x10;
const NAME: u64 = 0x11;
const ATTRIBUTES: u64 = 0x15;
const ENCODED_HEADER: u8 = 0x17;

/// A damaged header, said plainly.
pub(super) fn damaged(what: &str) -> Error {
    Error::Unreadable(format!(
        "the 7z archive's list of files is damaged ({what})"
    ))
}

/// How a folder's bytes are unpacked: the methods dm reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Method {
    /// Stored as they are.
    Copy,
    /// LZMA, with its five property bytes.
    Lzma(Vec<u8>),
    /// LZMA2, with its dictionary-size byte.
    Lzma2(u8),
}

/// Why a folder cannot be unpacked by dm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refusal {
    /// It uses a method dm does not read; the text names it.
    Method(String),
}

/// A folder's AES coder (gaps row F10): its properties, read once a password is given, and
/// the bytes it decrypts to, the input of the coder after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AesCoder {
    pub(super) props: Vec<u8>,
    pub(super) size: u64,
}

/// A stream's size and, where the archive gives one, its checksum.
pub(super) type StreamSize = (u64, Option<u32>);

/// One folder: a packed stream and how it unpacks, and the files' streams inside it.
#[derive(Debug, Clone)]
pub(super) struct Folder {
    /// How its bytes unpack once any AES is undone.
    pub(super) method: Result<Method, Refusal>,
    /// Whether any coder of it is AES: a password is needed, whatever else it holds.
    pub(super) encrypted: bool,
    /// The AES coder under `method`, where the folder is one dm decrypts: AES alone, or
    /// AES read by one coder dm reads, as 7-Zip writes an archive with a password.
    pub(super) aes: Option<AesCoder>,
    /// Where its packed bytes start in the file, and how many there are.
    pub(super) pack_offset: u64,
    pub(super) pack_size: u64,
    /// Bytes once unpacked.
    pub(super) unpack_size: u64,
    pub(super) crc: Option<u32>,
    /// The streams it holds, one file each, in order: size and checksum.
    pub(super) streams: Vec<StreamSize>,
}

/// One entry of the archive.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub(super) name: String,
    /// A folder (an entry with no stream that is not an empty file).
    pub(super) dir: bool,
    /// An entry marked as deleted, in an archive made by updating another.
    pub(super) anti: bool,
    pub(super) attributes: Option<u32>,
    /// Its bytes: the folder, the stream's place in it, its size and checksum; `None` for an
    /// empty file or a folder.
    pub(super) stream: Option<Stream>,
}

/// Where an entry's bytes are.
#[derive(Debug, Clone, Copy)]
pub(super) struct Stream {
    pub(super) folder: usize,
    pub(super) index: usize,
    pub(super) size: u64,
    pub(super) crc: Option<u32>,
}

/// What a 7z archive holds.
#[derive(Debug, Clone, Default)]
pub(super) struct Archive {
    pub(super) folders: Vec<Folder>,
    pub(super) entries: Vec<Entry>,
}

/// A reader over the header's bytes; every read is checked against its end.
pub(super) struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(data: &'a [u8]) -> Cursor<'a> {
        Cursor { data, at: 0 }
    }

    fn left(&self) -> usize {
        self.data.len().saturating_sub(self.at)
    }

    fn byte(&mut self) -> Result<u8, Error> {
        let b = self
            .data
            .get(self.at)
            .copied()
            .ok_or_else(|| damaged("it ends early"))?;
        self.at += 1;
        Ok(b)
    }

    fn bytes(&mut self, n: u64) -> Result<&'a [u8], Error> {
        let n = usize::try_from(n).map_err(|_| damaged("a length too large"))?;
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| damaged("it ends early"))?;
        let out = self.data.get(self.at..end).unwrap_or_default();
        self.at = end;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// The format's variable-length number: the first byte's leading one bits say how many
    /// bytes follow, little-endian, its remaining bits are the highest.
    pub(super) fn number(&mut self) -> Result<u64, Error> {
        let first = self.byte()?;
        let mut mask = 0x80u8;
        let mut value = 0u64;
        for i in 0..8 {
            if first & mask == 0 {
                let high = u64::from(first & mask.wrapping_sub(1));
                return Ok(value | (high << (8 * i)));
            }
            value |= u64::from(self.byte()?) << (8 * i);
            mask >>= 1;
        }
        Ok(value)
    }

    /// A count of things that each take at least one byte of what is left.
    fn count(&mut self) -> Result<usize, Error> {
        let n = self.number()?;
        if n > self.left() as u64 {
            return Err(damaged("a count larger than the header"));
        }
        Ok(n as usize)
    }

    /// `n` flags, eight to a byte, the first in the highest bit.
    fn bits(&mut self, n: usize) -> Result<Vec<bool>, Error> {
        let bytes = self.bytes(n.div_ceil(8) as u64)?;
        Ok((0..n)
            .map(|i| bytes.get(i / 8).is_some_and(|b| b & (0x80 >> (i % 8)) != 0))
            .collect())
    }

    /// `n` flags, or all set where the first byte says so.
    fn defined(&mut self, n: usize) -> Result<Vec<bool>, Error> {
        if self.byte()? != 0 {
            Ok(vec![true; n])
        } else {
            self.bits(n)
        }
    }

    /// `n` checksums, some perhaps missing.
    fn digests(&mut self, n: usize) -> Result<Vec<Option<u32>>, Error> {
        let defined = self.defined(n)?;
        defined
            .into_iter()
            .map(|d| if d { self.u32().map(Some) } else { Ok(None) })
            .collect()
    }
}

/// A folder as the header lists it, before its streams are placed.
#[derive(Debug, Clone)]
struct RawFolder {
    method: Result<Method, Refusal>,
    encrypted: bool,
    /// The AES coder's output and its properties.
    aes: Option<(usize, Vec<u8>)>,
    /// The number of packed streams it reads.
    packed: usize,
    /// The number of coder outputs it has.
    outs: usize,
    /// The size of each coder output, in order.
    out_sizes: Vec<u64>,
    /// Which output is the folder's own (the one no other coder reads).
    main_out: usize,
    crc: Option<u32>,
}

impl RawFolder {
    fn unpack_size(&self) -> u64 {
        self.out_sizes.get(self.main_out).copied().unwrap_or(0)
    }
}

/// One coder: a method dm reads, AES with its properties, or a refusal.
#[derive(Debug, Clone)]
enum Coder {
    Plain(Method),
    Aes(Vec<u8>),
    Refused(Refusal),
}

/// The 7zAES coder's id.
const AES_ID: [u8; 4] = [0x06, 0xF1, 0x07, 0x01];

fn coder_of(id: &[u8], props: &[u8]) -> Coder {
    if id == AES_ID {
        return Coder::Aes(props.to_vec());
    }
    match method_of(id, props) {
        Ok(m) => Coder::Plain(m),
        Err(r) => Coder::Refused(r),
    }
}

/// A coder's method id, named as plainly as it can be for a refusal.
fn method_of(id: &[u8], props: &[u8]) -> Result<Method, Refusal> {
    match id {
        [0x00] => Ok(Method::Copy),
        [0x03, 0x01, 0x01] if props.len() == 5 => Ok(Method::Lzma(props.to_vec())),
        [0x21] if props.len() == 1 => Ok(Method::Lzma2(props.first().copied().unwrap_or(0))),
        [0x03, 0x03, ..] | [0x04..=0x0B] => Err(Refusal::Method("a BCJ filter".into())),
        [0x03] => Err(Refusal::Method("a Delta filter".into())),
        [0x03, 0x04, 0x01] => Err(Refusal::Method("PPMd".into())),
        [0x04, 0x02, 0x02] => Err(Refusal::Method("BZip2".into())),
        [0x04, 0x01, 0x08] => Err(Refusal::Method("Deflate".into())),
        [0x04, 0x01, 0x09] => Err(Refusal::Method("Deflate64".into())),
        _ => {
            let hex: String = id.iter().map(|b| format!("{b:02X}")).collect();
            Err(Refusal::Method(format!(
                "a method dm does not know ({hex})"
            )))
        }
    }
}

/// One folder's coders, bind pairs and packed streams.
fn folder(c: &mut Cursor) -> Result<RawFolder, Error> {
    let coders = c.number()?;
    if coders == 0 || coders > CODERS_MOST {
        return Err(damaged("a folder's coders"));
    }
    let (mut ins, mut outs) = (0u64, 0u64);
    let mut methods = Vec::new();
    for _ in 0..coders {
        let flags = c.byte()?;
        if flags & 0x80 != 0 {
            return Err(damaged("a coder with alternatives"));
        }
        let id = c.bytes(u64::from(flags & 0x0F))?;
        let (i, o) = if flags & 0x10 != 0 {
            (c.number()?, c.number()?)
        } else {
            (1, 1)
        };
        ins = ins.saturating_add(i);
        outs = outs.saturating_add(o);
        if ins > CODERS_MOST || outs > CODERS_MOST {
            return Err(damaged("a folder's streams"));
        }
        let props = if flags & 0x20 != 0 {
            let n = c.number()?;
            c.bytes(n)?
        } else {
            &[]
        };
        methods.push((coder_of(id, props), i, o));
    }
    if outs == 0 {
        return Err(damaged("a folder with no output"));
    }
    let binds = outs - 1;
    let mut bound = Vec::new();
    for _ in 0..binds {
        let in_index = c.number()?;
        bound.push((in_index, c.number()?));
    }
    let bound_outs: Vec<u64> = bound.iter().map(|&(_, o)| o).collect();
    let packed = ins
        .checked_sub(binds)
        .filter(|&p| p > 0)
        .ok_or_else(|| damaged("a folder's packed streams"))?;
    if packed > 1 {
        for _ in 0..packed {
            c.number()?;
        }
    }
    let main_out = (0..outs)
        .find(|o| !bound_outs.contains(o))
        .ok_or_else(|| damaged("a folder with no output of its own"))? as usize;
    let encrypted = methods.iter().any(|(m, _, _)| matches!(m, Coder::Aes(_)));
    // One plain coder dm reads; AES alone (over stored bytes); or one coder dm reads whose
    // input is AES's output, as an archiver writes a password's archive: the bind pair names
    // the reading coder's input and AES's output, in either order of the two (7-Zip lists
    // the reading coder first, py7zr AES). Anything else is refused by the first thing dm
    // does not read.
    let pair = |reader: usize, aes: usize| -> Option<(Method, Vec<u8>)> {
        match (methods.get(reader), methods.get(aes)) {
            (Some((Coder::Plain(m), 1, 1)), Some((Coder::Aes(p), 1, 1)))
                if bound == [(reader as u64, aes as u64)] =>
            {
                Some((m.clone(), p.clone()))
            }
            _ => None,
        }
    };
    let chained = match methods.len() {
        2 => pair(0, 1)
            .map(|c| (c, 1))
            .or_else(|| pair(1, 0).map(|c| (c, 0))),
        _ => None,
    };
    let (method, aes) = match (methods.as_slice(), chained) {
        (_, Some(((m, p), at))) => (Ok(m), Some((at, p))),
        ([(Coder::Plain(m), 1, 1)], _) => (Ok(m.clone()), None),
        ([(Coder::Refused(r), 1, 1)], _) => (Err(r.clone()), None),
        ([(Coder::Aes(p), 1, 1)], _) => (Ok(Method::Copy), Some((0, p.clone()))),
        _ => {
            let refused = methods.iter().find_map(|(m, _, _)| match m {
                Coder::Refused(r) => Some(r.clone()),
                _ => None,
            });
            let chain = || Refusal::Method("a chain of several methods".into());
            (Err(refused.unwrap_or_else(chain)), None)
        }
    };
    Ok(RawFolder {
        method,
        encrypted,
        aes,
        packed: packed as usize,
        outs: outs as usize,
        out_sizes: Vec::new(),
        main_out,
        crc: None,
    })
}

/// The packed streams, folders and their files' streams, as one `StreamsInfo` lists them.
#[derive(Debug, Default)]
struct Streams {
    pack_pos: u64,
    pack_sizes: Vec<u64>,
    folders: Vec<RawFolder>,
    /// Per folder: its streams' sizes and checksums.
    substreams: Option<Vec<Vec<StreamSize>>>,
}

/// The folders with their packed bytes placed in the file: pack positions count from just
/// after the 32-byte signature header, and nothing may lie past `file_len`.
fn place(s: Streams, file_len: u64) -> Result<Vec<Folder>, Error> {
    let mut offset = 32u64
        .checked_add(s.pack_pos)
        .ok_or_else(|| damaged("a packed stream's place"))?;
    let mut sizes = s.pack_sizes.iter();
    let mut subs = s.substreams.map(Vec::into_iter);
    let mut out = Vec::with_capacity(s.folders.len());
    for f in s.folders {
        // A folder of several packed streams is refused anyway; its place is the first.
        let mut pack_size = 0u64;
        let start = offset;
        for _ in 0..f.packed {
            let size = *sizes
                .next()
                .ok_or_else(|| damaged("a folder with no packed stream"))?;
            pack_size = pack_size.saturating_add(size);
            offset = offset
                .checked_add(size)
                .ok_or_else(|| damaged("a packed stream's size"))?;
        }
        if offset > file_len {
            return Err(Error::Unreadable(
                "the 7z archive is cut short, or is one part of several (.7z.001), \
                 which dm does not join"
                    .into(),
            ));
        }
        let unpack_size = f.unpack_size();
        let streams = match subs.as_mut() {
            Some(it) => it.next().unwrap_or_default(),
            None => vec![(unpack_size, f.crc)],
        };
        let aes = f.aes.map(|(at, props)| AesCoder {
            props,
            size: f.out_sizes.get(at).copied().unwrap_or(0),
        });
        out.push(Folder {
            method: f.method,
            encrypted: f.encrypted,
            aes,
            pack_offset: start,
            pack_size,
            unpack_size,
            crc: f.crc,
            streams,
        });
    }
    Ok(out)
}

/// Names, UTF-16LE, each ended by a zero.
fn names(data: &[u8], n: usize) -> Result<Vec<String>, Error> {
    let mut c = Cursor::new(data);
    if c.byte()? != 0 {
        return Err(damaged("names kept elsewhere"));
    }
    let units: Vec<u16> = c
        .bytes(c.left() as u64)?
        .chunks(2)
        .map(|p| {
            u16::from_le_bytes([
                p.first().copied().unwrap_or(0),
                p.get(1).copied().unwrap_or(0),
            ])
        })
        .collect();
    let mut out = Vec::with_capacity(n);
    let mut parts = units.split(|&u| u == 0);
    for _ in 0..n {
        let part = parts
            .next()
            .ok_or_else(|| damaged("fewer names than entries"))?;
        // A name that is not text cannot be placed, so the archive is refused rather than
        // guessed at, as a ZIP's would be.
        let name = String::from_utf16(part)
            .map_err(|_| Error::Unreadable("an entry has a name that cannot be read".into()))?;
        out.push(name);
    }
    Ok(out)
}

/// The attributes, each a Windows attribute word with, where bit 15 is set, a Unix mode in
/// its high half.
fn attributes(data: &[u8], n: usize) -> Result<Vec<Option<u32>>, Error> {
    let mut c = Cursor::new(data);
    let defined = c.defined(n)?;
    if c.byte()? != 0 {
        return Err(damaged("attributes kept elsewhere"));
    }
    defined
        .into_iter()
        .map(|d| if d { c.u32().map(Some) } else { Ok(None) })
        .collect()
}

/// The entries, given each its stream from `folders` in order.
fn files_info(c: &mut Cursor, folders: &[Folder], max_entries: usize) -> Result<Vec<Entry>, Error> {
    let n = c.number()?;
    if n > max_entries as u64 {
        return Err(Error::TooBig("too many entries"));
    }
    let n = n as usize;
    let (mut empty_stream, mut empty_file, mut anti) = (vec![false; n], Vec::new(), Vec::new());
    let mut names_found: Option<Vec<String>> = None;
    let mut attrs: Vec<Option<u32>> = vec![None; n];
    loop {
        let ty = c.number()?;
        if ty == 0 {
            break;
        }
        let size = c.number()?;
        let data = c.bytes(size)?;
        let empties = empty_stream.iter().filter(|&&e| e).count();
        match ty {
            EMPTY_STREAM => empty_stream = Cursor::new(data).bits(n)?,
            EMPTY_FILE => empty_file = Cursor::new(data).bits(empties)?,
            ANTI => anti = Cursor::new(data).bits(empties)?,
            NAME => names_found = Some(names(data, n)?),
            ATTRIBUTES => attrs = attributes(data, n)?,
            // Times, a start position, padding: nothing dm needs.
            _ => {}
        }
    }
    let names_found = match names_found {
        Some(v) => v,
        None if n == 0 => Vec::new(),
        None => return Err(damaged("entries without names")),
    };
    // The streams, in folder order, are handed to the entries that have one, in order.
    let mut streams = folders.iter().enumerate().flat_map(|(fi, f)| {
        f.streams
            .iter()
            .enumerate()
            .map(move |(si, &(size, crc))| Stream {
                folder: fi,
                index: si,
                size,
                crc,
            })
    });
    let mut out = Vec::with_capacity(n);
    let mut empty_at = 0usize;
    for (i, name) in names_found.into_iter().enumerate() {
        let attributes = attrs.get(i).copied().flatten();
        if empty_stream.get(i).copied().unwrap_or(false) {
            let file = empty_file.get(empty_at).copied().unwrap_or(false);
            let gone = anti.get(empty_at).copied().unwrap_or(false);
            empty_at += 1;
            out.push(Entry {
                name,
                dir: !file,
                anti: gone,
                attributes,
                stream: None,
            });
        } else {
            let stream = streams
                .next()
                .ok_or_else(|| damaged("more files than streams"))?;
            out.push(Entry {
                name,
                dir: false,
                anti: false,
                attributes,
                stream: Some(stream),
            });
        }
    }
    if streams.next().is_some() {
        return Err(damaged("more streams than files"));
    }
    Ok(out)
}

/// The plain header: `kHeader` already read.
fn header(c: &mut Cursor, file_len: u64, max_entries: usize) -> Result<Archive, Error> {
    let mut archive = Archive::default();
    loop {
        match c.byte()? {
            END => return Ok(archive),
            ARCHIVE_PROPERTIES => loop {
                let ty = c.byte()?;
                if ty == 0 {
                    break;
                }
                let n = c.number()?;
                c.bytes(n)?;
            },
            ADDITIONAL_STREAMS => {
                // Streams for data kept outside the header, which dm never reads.
                streams_info(c, max_entries)?;
            }
            MAIN_STREAMS => {
                archive.folders = place(streams_info(c, max_entries)?, file_len)?;
            }
            FILES_INFO => {
                archive.entries = files_info(c, &archive.folders, max_entries)?;
            }
            _ => return Err(damaged("a part of no known kind")),
        }
    }
}

/// Reads the list of what the archive at `path` holds. `unpack` turns a folder into its
/// bytes, for a header that is itself packed.
pub(super) fn read(
    path: &Path,
    max_entries: usize,
    unpack: &dyn Fn(&Folder) -> Result<Vec<u8>, Error>,
) -> Result<Archive, Error> {
    let mut file = super::volumes::open(path)?;
    let file_len = file.len();
    let mut start = [0u8; 32];
    file.read_exact(&mut start)
        .map_err(|_| Error::Unreadable("this is not a 7z archive".into()))?;
    if start.get(..6) != Some(SIGNATURE.as_slice()) {
        return Err(Error::Unreadable("this is not a 7z archive".into()));
    }
    if start[6] != 0 {
        return Err(Error::Unreadable(
            "this 7z archive is of a later version than dm reads".into(),
        ));
    }
    let mut c = Cursor::new(&start[8..]);
    let start_crc = u32::from_le_bytes([start[8], start[9], start[10], start[11]]);
    if crc32fast::hash(&start[12..32]) != start_crc {
        return Err(damaged("its first header does not add up"));
    }
    c.bytes(4)?;
    let next_offset = c
        .bytes(8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))?;
    let next_size = c
        .bytes(8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))?;
    let next_crc = c.u32()?;
    if next_size == 0 {
        // An archive of nothing.
        return Ok(Archive::default());
    }
    if next_size > HEADER_MOST {
        return Err(Error::TooBig("its list of files is too large"));
    }
    let at = 32u64
        .checked_add(next_offset)
        .ok_or_else(|| damaged("where its list of files is"))?;
    if at.checked_add(next_size).is_none_or(|end| end > file_len) {
        return Err(Error::Unreadable(
            "the 7z archive is cut short, or is one part of several (.7z.001), \
             which dm does not join"
                .into(),
        ));
    }
    file.seek(SeekFrom::Start(at))?;
    let mut bytes = vec![0u8; next_size as usize];
    file.read_exact(&mut bytes)?;
    if crc32fast::hash(&bytes) != next_crc {
        return Err(damaged("its list of files does not add up"));
    }
    let mut c = Cursor::new(&bytes);
    match c.byte()? {
        HEADER => header(&mut c, file_len, max_entries),
        ENCODED_HEADER => {
            // The header is itself packed, in the folder these streams describe.
            let s = streams_info(&mut c, max_entries)?;
            let folders = place(s, file_len)?;
            let f = match folders.as_slice() {
                [f] => f,
                _ => return Err(damaged("its packed list of files")),
            };
            // A method dm does not read, or a password not given, is `unpack`'s to say.
            if f.unpack_size > HEADER_MOST {
                return Err(Error::TooBig("its list of files is too large"));
            }
            let plain = unpack(f)?;
            let mut c = Cursor::new(&plain);
            if c.byte()? != HEADER {
                return Err(damaged("its packed list of files"));
            }
            header(&mut c, file_len, max_entries)
        }
        _ => Err(damaged("its list of files")),
    }
}
