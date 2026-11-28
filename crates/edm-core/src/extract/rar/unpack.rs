// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM's own decompressor for RAR 5's compression (gaps row F10): LZ77 over a window of up to
//! the file's dictionary, coded with the Huffman tables of `huffman.rs`.
//!
//! A file's packed bytes are blocks, each a small header (its flags, a check byte and its
//! size), then optionally new tables, then codes: a literal byte; a match (a length and a
//! distance, the distance's lowest four bits under a table of their own when it is long);
//! one of the last four distances again with a new length; the last match again; or a filter
//! over a block of the output, undone (`filters.rs`) before that block is written. In a solid
//! archive the window, the tables and the distances carry on from one file to the next.
//!
//! Positions are counted from the stream's start in 64 bits, so the window's wrap and a
//! filter's place need no other bookkeeping. Every count is outside data: a match before the
//! first byte, a filter larger than the window can hold, a block cut short are errors, never
//! a panic or a read out of bounds.

use std::collections::VecDeque;
use std::io::Read;

use super::super::Error;
use super::filters;
use super::header::damaged;
use super::huffman::{read_tables, Bits, Tables};

/// The longest match: a length slot's most (4097) and three more for a far distance.
const MAX_MATCH: u64 = 4100;
/// The largest block a filter covers; a filter said to be larger is none.
const MAX_FILTER: u32 = 0x40_0000;
/// The most filters waiting at once.
const MAX_FILTERS: usize = 8192;
/// The window is never smaller than this, so a filter's whole block fits in it beside what
/// is still to be written.
const MIN_WINDOW: u64 = 8 << 20;
/// The largest window: EDM's largest dictionary, rounded up to a power of two (the format's
/// own is 4 GiB; a larger dictionary is refused, `allowed`).
const MAX_WINDOW: u64 = crate::extract::MOST_DICTIONARY.next_power_of_two();

/// `dict` refused where it is over the largest dictionary EDM unpacks with, before any window
/// is made (gaps row H35e); it was clamped to 4 GiB.
fn allowed(dict: u64) -> Result<(), Error> {
    if dict > crate::extract::MOST_DICTIONARY {
        return Err(Error::TooBig(crate::extract::DICTIONARY_TOO_LARGE));
    }
    Ok(())
}
/// Bytes unpacked between writes.
const FLUSH: u64 = 1 << 20;

/// The bytes unpacked last, which a match copies from. It grows as bytes are unpacked, up to
/// its size (a power of two), and then wraps, so a small file never costs a large window.
pub(super) struct Window {
    buf: Vec<u8>,
    pub(super) size: u64,
}

impl Window {
    pub(super) fn new(dict: u64) -> Window {
        Window {
            buf: Vec::new(),
            size: dict.clamp(MIN_WINDOW, MAX_WINDOW).next_power_of_two(),
        }
    }

    fn index(&self, pos: u64) -> usize {
        (pos & (self.size - 1)) as usize
    }

    /// Puts the byte at `pos`, the next one after the last put.
    pub(super) fn put(&mut self, pos: u64, b: u8) {
        let i = self.index(pos);
        match self.buf.get_mut(i) {
            Some(slot) => *slot = b,
            // Still growing: `i` is the end.
            None => self.buf.push(b),
        }
    }

    pub(super) fn get(&self, pos: u64) -> u8 {
        self.buf.get(self.index(pos)).copied().unwrap_or(0)
    }

    /// Appends the bytes from `from` up to `to` to `out`.
    pub(super) fn copy_out(&self, from: u64, to: u64, out: &mut Vec<u8>) -> Result<(), Error> {
        let mut at = from;
        while at < to {
            let i = self.index(at);
            let run = (self.size - i as u64).min(to - at) as usize;
            let bytes = self
                .buf
                .get(i..i + run)
                .ok_or_else(|| damaged("a block is written before it is unpacked"))?;
            out.extend_from_slice(bytes);
            at += run as u64;
        }
        Ok(())
    }

    /// Makes room for a solid file's dictionary `dict`, where `pos` bytes are unpacked: a
    /// larger one only while the window has not wrapped yet.
    pub(super) fn fit(&mut self, dict: u64, pos: u64) -> Result<(), Error> {
        allowed(dict)?;
        let want = dict.clamp(MIN_WINDOW, MAX_WINDOW).next_power_of_two();
        if want > self.size {
            if pos > self.buf.len() as u64 {
                return Err(damaged(
                    "a solid file needs a larger dictionary than the one before",
                ));
            }
            self.size = want;
        }
        Ok(())
    }
}

/// A filter waiting for its block to be unpacked.
#[derive(Debug, Clone, Copy)]
struct Filter {
    start: u64,
    len: u64,
    kind: u8,
    channels: usize,
}

/// One block of a file's packed bytes.
struct Block {
    bits: Bits,
    /// Where its last bit ends.
    end: u64,
    last: bool,
    tables: bool,
}

/// What one code of the main table asks for.
enum Op {
    Literal(u8),
    /// A length and a distance.
    Match(u64, u64),
    Filter,
    /// The last match again.
    Last,
    /// One of the last four distances, by its place, with a new length.
    Repeat(usize, u64),
}

fn ends_early() -> Error {
    Error::Unreadable("the archive ends in the middle of a file".into())
}

/// Reads the next block of `input`: its header checked, its bytes held.
fn read_block(input: &mut dyn Read) -> Result<Block, Error> {
    let mut head = [0u8; 2];
    input.read_exact(&mut head).map_err(|_| ends_early())?;
    let (flags, check) = (head[0], head[1]);
    // Its size takes one to three bytes.
    let count = usize::from((flags >> 3) & 3) + 1;
    if count == 4 {
        return Err(damaged("a block's header"));
    }
    let mut size = [0u8; 3];
    input
        .read_exact(size.get_mut(..count).unwrap_or_default())
        .map_err(|_| ends_early())?;
    if 0x5a ^ flags ^ size[0] ^ size[1] ^ size[2] != check {
        return Err(damaged("a block's header does not add up"));
    }
    let size = u64::from(u32::from_le_bytes([size[0], size[1], size[2], 0]));
    let mut data = Vec::new();
    (&mut *input)
        .take(size)
        .read_to_end(&mut data)
        .map_err(|e| Error::Unreadable(e.to_string()))?;
    if data.len() as u64 != size {
        return Err(ends_early());
    }
    // Its last byte holds one to eight bits of it.
    let end = match size {
        0 => 0,
        n => (n - 1) * 8 + u64::from(flags & 7) + 1,
    };
    Ok(Block {
        bits: Bits::new(data),
        end,
        last: flags & 0x40 != 0,
        tables: flags & 0x80 != 0,
    })
}

/// A match's length from its slot: 2 to 9, then ranges that double, the slot's extra bits
/// choosing within one.
fn length(bits: &mut Bits, slot: u32) -> u64 {
    if slot < 8 {
        return 2 + u64::from(slot);
    }
    let n = slot / 4 - 1;
    2 + (u64::from(4 | (slot & 3)) << n) + u64::from(bits.take(n))
}

/// A match's distance from the distance table: 1 to 4, then ranges that double; past 16 its
/// lowest four bits come from the low-bits table.
fn distance(t: &Tables, bits: &mut Bits) -> u64 {
    let slot = u32::from(t.dist.decode(bits));
    if slot < 4 {
        return 1 + u64::from(slot);
    }
    let n = slot / 2 - 1;
    let mut dist = 1 + (u64::from(2 | (slot & 1)) << n);
    if n >= 4 {
        if n > 4 {
            dist += u64::from(bits.peek32() >> (36 - n)) << 4;
            bits.skip(n - 4);
        }
        dist += u64::from(t.align.decode(bits));
    } else {
        dist += u64::from(bits.peek32() >> (32 - n));
        bits.skip(n);
    }
    dist
}

/// The next code of the main table and what follows it.
fn decode(t: &Tables, bits: &mut Bits) -> Op {
    let slot = u32::from(t.main.decode(bits));
    match slot {
        0..=255 => Op::Literal(slot as u8),
        256 => Op::Filter,
        257 => Op::Last,
        258..=261 => {
            let code = u32::from(t.len.decode(bits));
            let len = length(bits, code);
            Op::Repeat((slot - 258) as usize, len)
        }
        _ => {
            let mut len = length(bits, slot - 262);
            let dist = distance(t, bits);
            // A far match is never short, so its length is counted from further up.
            for far in [0x100, 0x2000, 0x40000] {
                if dist > far {
                    len += 1;
                }
            }
            Op::Match(len, dist)
        }
    }
}

/// A filter's number: two bits for how many bytes, then the bytes, the lowest first.
fn filter_number(bits: &mut Bits) -> u32 {
    let count = bits.take(2) + 1;
    (0..count).fold(0u32, |v, i| v | (bits.take(8) << (i * 8)))
}

/// The decompressor's state, kept from one file to the next of a solid stream.
pub(super) struct Unpacker {
    win: Window,
    /// Bytes unpacked since the stream began.
    pos: u64,
    /// Bytes of those handed on to be written.
    written: u64,
    /// Where the file being unpacked began.
    file_start: u64,
    /// The last four distances, the newest first, and the last match's length.
    old: [u64; 4],
    last_len: u64,
    tables: Option<Tables>,
    filters: VecDeque<Filter>,
    out: Vec<u8>,
    spare: Vec<u8>,
}

impl Unpacker {
    /// A stream whose first file needs the dictionary `dict`.
    pub(super) fn new(dict: u64) -> Result<Unpacker, Error> {
        allowed(dict)?;
        Ok(Unpacker {
            win: Window::new(dict),
            pos: 0,
            written: 0,
            file_start: 0,
            old: [0; 4],
            last_len: 0,
            tables: None,
            filters: VecDeque::new(),
            out: Vec::new(),
            spare: Vec::new(),
        })
    }

    /// Readies the stream for a solid file whose dictionary is `dict`.
    pub(super) fn solid(&mut self, dict: u64) -> Result<(), Error> {
        self.win.fit(dict, self.pos)
    }

    /// Unpacks one file of `size` bytes from its packed bytes `input`, handing what it
    /// unpacks to `sink` in order.
    pub(super) fn file(
        &mut self,
        input: &mut dyn Read,
        size: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        // A filter never reaches from one file into the next.
        self.filters.clear();
        self.file_start = self.pos;
        self.written = self.pos;
        let mut block = self.block(input)?;
        let mut due = self.pos + FLUSH;
        loop {
            if block.bits.at() >= block.end {
                if block.last {
                    break;
                }
                block = self.block(input)?;
                continue;
            }
            if self.pos >= due || self.filters.len() >= MAX_FILTERS {
                self.flush(sink)?;
                due = self.pos + FLUSH;
            }
            // Room for the longest match without overwriting what is not written yet.
            if self.pos + MAX_MATCH - self.written > self.win.size {
                return Err(damaged("a filter's block is larger than its window"));
            }
            let op = match self.tables.as_ref() {
                Some(t) => decode(t, &mut block.bits),
                None => return Err(damaged("a block has no tables")),
            };
            self.apply(op, &mut block.bits)?;
        }
        self.flush(sink)?;
        if self.filters.iter().any(|f| f.start < self.pos) {
            return Err(damaged("a filter runs past the end of its file"));
        }
        if self.pos - self.file_start != size {
            return Err(damaged("a file unpacks to another size than it says"));
        }
        Ok(())
    }

    /// The next block, with the tables it gives.
    fn block(&mut self, input: &mut dyn Read) -> Result<Block, Error> {
        let mut block = read_block(input)?;
        if block.tables {
            self.tables = Some(read_tables(&mut block.bits)?);
        }
        if self.tables.is_none() {
            return Err(damaged("a block has no tables"));
        }
        Ok(block)
    }

    fn apply(&mut self, op: Op, bits: &mut Bits) -> Result<(), Error> {
        match op {
            Op::Literal(b) => {
                self.win.put(self.pos, b);
                self.pos += 1;
                Ok(())
            }
            Op::Match(len, dist) => {
                self.old = [dist, self.old[0], self.old[1], self.old[2]];
                self.last_len = len;
                self.copy(len, dist)
            }
            Op::Last if self.last_len == 0 => Ok(()),
            Op::Last => self.copy(self.last_len, self.old[0]),
            Op::Repeat(n, len) => {
                let n = n.min(3);
                let dist = self.old[n];
                // It becomes the newest.
                self.old.copy_within(0..n, 1);
                self.old[0] = dist;
                self.last_len = len;
                self.copy(len, dist)
            }
            Op::Filter => {
                self.filter(bits);
                Ok(())
            }
        }
    }

    /// Copies `len` bytes from `dist` back, one at a time, so a match may overlap itself.
    fn copy(&mut self, len: u64, dist: u64) -> Result<(), Error> {
        if dist == 0 || dist > self.pos || dist > self.win.size {
            return Err(damaged("a match reaches before the first byte"));
        }
        for _ in 0..len {
            let b = self.win.get(self.pos - dist);
            self.win.put(self.pos, b);
            self.pos += 1;
        }
        Ok(())
    }

    /// A filter: where its block starts from here, how long it is, its kind, and DELTA's
    /// channels. One of no length, or longer than any, is none; one of a kind EDM does not
    /// know leaves its block as it is unpacked, which the file's CRC then refuses.
    fn filter(&mut self, bits: &mut Bits) {
        let start = filter_number(bits);
        let len = filter_number(bits);
        let kind = bits.take(3) as u8;
        let channels = if kind == filters::DELTA {
            bits.take(5) as usize + 1
        } else {
            0
        };
        if len == 0 || len > MAX_FILTER || kind > filters::ARM {
            return;
        }
        // Still too many after a write: dropped, as the format's own reader does.
        if self.filters.len() >= MAX_FILTERS {
            self.filters.clear();
        }
        self.filters.push_back(Filter {
            start: self.pos + u64::from(start),
            len: u64::from(len),
            kind,
            channels,
        });
    }

    /// Hands on what is unpacked, each filter's block undone once it is whole, up to the
    /// first filter whose block is not.
    fn flush(&mut self, sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        while self.written < self.pos {
            let Some(f) = self.filters.front().copied() else {
                return self.write(self.pos, sink);
            };
            if f.start < self.written {
                // It overlaps what is written already: no filter.
                self.filters.pop_front();
                continue;
            }
            if f.start > self.written {
                self.write(f.start.min(self.pos), sink)?;
                continue;
            }
            let end = f.start + f.len;
            if end > self.pos {
                break;
            }
            self.out.clear();
            self.win.copy_out(f.start, end, &mut self.out)?;
            let offset = f.start - self.file_start;
            filters::undo(f.kind, f.channels, offset, &mut self.out, &mut self.spare);
            sink(&self.out)?;
            self.written = end;
            self.filters.pop_front();
        }
        Ok(())
    }

    /// Hands on the bytes unpacked up to `to`, as they are.
    fn write(
        &mut self,
        to: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.out.clear();
        self.win.copy_out(self.written, to, &mut self.out)?;
        sink(&self.out)?;
        self.written = to;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
