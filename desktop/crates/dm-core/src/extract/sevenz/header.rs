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

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::super::Error;

/// The first six bytes of every 7z archive.
pub(super) const SIGNATURE: [u8; 6] = *b"7z\xBC\xAF\x27\x1C";

/// The most a header may be, packed or not: a list of names, not a file.
pub(super) const HEADER_MOST: u64 = 64 << 20;

/// The most coders, or streams between them, one folder may have.
const CODERS_MOST: u64 = 64;

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
    /// It is encrypted (AES): a password would be needed.
    Encrypted,
    /// It uses a method dm does not read; the text names it.
    Method(String),
}

/// A stream's size and, where the archive gives one, its checksum.
pub(super) type StreamSize = (u64, Option<u32>);

/// One folder: a packed stream and how it unpacks, and the files' streams inside it.
#[derive(Debug, Clone)]
pub(super) struct Folder {
    pub(super) method: Result<Method, Refusal>,
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

/// A coder's method id, named as plainly as it can be for a refusal.
fn method_of(id: &[u8], props: &[u8]) -> Result<Method, Refusal> {
    match id {
        [0x00] => Ok(Method::Copy),
        [0x03, 0x01, 0x01] if props.len() == 5 => Ok(Method::Lzma(props.to_vec())),
        [0x21] if props.len() == 1 => Ok(Method::Lzma2(props.first().copied().unwrap_or(0))),
        [0x06, 0xF1, 0x07, 0x01] => Err(Refusal::Encrypted),
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
        methods.push((method_of(id, props), i, o));
    }
    if outs == 0 {
        return Err(damaged("a folder with no output"));
    }
    let binds = outs - 1;
    let mut bound_outs = Vec::new();
    for _ in 0..binds {
        let _in_index = c.number()?;
        bound_outs.push(c.number()?);
    }
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
    // One plain coder dm reads, or the first thing in the chain it does not.
    let method = match methods.as_slice() {
        [(m, 1, 1)] => m.clone(),
        _ => {
            let refused = methods.iter().find_map(|(m, _, _)| m.clone().err());
            if methods
                .iter()
                .any(|(m, _, _)| *m == Err(Refusal::Encrypted))
            {
                Err(Refusal::Encrypted)
            } else {
                Err(refused.unwrap_or_else(|| Refusal::Method("a chain of several methods".into())))
            }
        }
    };
    Ok(RawFolder {
        method,
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

fn pack_info(c: &mut Cursor, s: &mut Streams) -> Result<(), Error> {
    s.pack_pos = c.number()?;
    let n = c.count()?;
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

fn unpack_info(c: &mut Cursor, s: &mut Streams) -> Result<(), Error> {
    if c.byte()? != FOLDER {
        return Err(damaged("its folders"));
    }
    let n = c.count()?;
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

fn streams_info(c: &mut Cursor, max_entries: usize) -> Result<Streams, Error> {
    let mut s = Streams::default();
    loop {
        match c.byte()? {
            END => return Ok(s),
            PACK_INFO => pack_info(c, &mut s)?,
            UNPACK_INFO => unpack_info(c, &mut s)?,
            SUBSTREAMS_INFO => substreams_info(c, &mut s, max_entries)?,
            _ => return Err(damaged("its streams")),
        }
    }
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
        out.push(Folder {
            method: f.method,
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
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
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
            match &f.method {
                Ok(_) => {}
                Err(Refusal::Encrypted) => {
                    return Err(Error::Encrypted("its list of files".into()))
                }
                Err(Refusal::Method(m)) => return Err(Error::Method(m.clone())),
            }
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
