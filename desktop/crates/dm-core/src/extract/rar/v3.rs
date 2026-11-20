// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's own decompressor for RAR 4's compression (versions 2.9 to 4.x, "unpack version 29";
//! gaps row F10), from the format's public description.
//!
//! A file's packed bytes are one stream of bits, the highest of each byte first. It starts
//! with tables (`tables.rs`), then codes under them: a literal; a match (a length, then a
//! distance whose lowest four bits, when it is long, come from a table of their own or
//! repeat the last ones); a short match of two bytes; one of the last four distances again;
//! the last match again; a filter (`vm.rs`); or the end of a block, after which come new
//! tables, or the file's end. Tables may also switch the stream to PPMd, a context model
//! under a range coder, until it switches back.
//!
//! In a solid archive the window, the distances, the tables and the filters carry on from
//! one file to the next. Positions count from the stream's start in 64 bits, as RAR 5's do
//! (`unpack.rs`, whose window this shares). Every count is outside data: a match before the
//! first byte, a filter out of bounds, a stream cut short are errors, never a panic.

mod tables;
mod vm;

use std::io::Read;

use self::tables::{Tables, ALL, DIST_BASE, DIST_BITS, LENGTH_BASE, LENGTH_BITS, REP};
use self::tables::{SHORT_BASE, SHORT_BITS};
use self::vm::Vm;
use super::super::Error;
use super::header::damaged;
use super::huffman::Bits;
use super::unpack::Window;

/// The longest match: a length slot's most (260) or PPMd's (287), with room to spare.
const MAX_MATCH: u64 = 300;
/// Bytes unpacked between writes.
const FLUSH: u64 = 1 << 20;
/// The packed bits kept ahead of the place being read (128 KiB): more than a filter's
/// record holds.
const KEEP: u64 = (128 << 10) * 8;
/// Packed bytes read at a time.
const CHUNK: u64 = 1 << 20;
/// How many long distances in a row take the last lowest four bits after a repeat code.
const LOW_REPEATS: u32 = 16;

fn ends_early() -> Error {
    Error::Unreadable("the archive ends in the middle of a file".into())
}

/// A file's packed bytes, read in pieces as they are needed.
struct Input<'a> {
    bits: Bits,
    src: &'a mut dyn Read,
    /// The source has given all it has.
    done: bool,
}

impl Input<'_> {
    /// Reads more of the source when what is left ahead runs low.
    fn fill(&mut self) -> Result<(), Error> {
        if self.done || self.bits.left() >= KEEP {
            return Ok(());
        }
        let mut more = Vec::new();
        (&mut *self.src)
            .take(CHUNK)
            .read_to_end(&mut more)
            .map_err(|e| Error::Unreadable(e.to_string()))?;
        if (more.len() as u64) < CHUNK {
            self.done = true;
        }
        self.bits.refill(&more);
        Ok(())
    }

    /// Every bit is read.
    fn exhausted(&self) -> bool {
        self.done && self.bits.left() == 0
    }

    /// More bits were read than there are.
    fn over(&self) -> bool {
        self.done && self.bits.over()
    }
}

/// A long distance's lowest four bits: the last ones, and how many more distances take
/// them again.
#[derive(Default)]
struct LowBits {
    last: u32,
    repeats: u32,
}

/// What one code of the main table asks for.
enum Op {
    Literal(u8),
    /// A length and a new distance.
    Match(u64, u64),
    /// One of the last four distances, by its place, with a new length.
    Repeat(usize, u64),
    /// The last match again.
    Last,
    EndOfBlock,
    Filter,
}

/// `n` extra bits (at most 16) as a number.
fn extra(bits: &mut Bits, n: u8) -> u64 {
    if n == 0 {
        0
    } else {
        u64::from(bits.take(u32::from(n)))
    }
}

/// The next code of the main table and the numbers that follow it.
fn decode(t: &Tables, low: &mut LowBits, bits: &mut Bits) -> Op {
    let n = usize::from(t.main.decode(bits));
    match n {
        0..=255 => Op::Literal(n as u8),
        256 => Op::EndOfBlock,
        257 => Op::Filter,
        258 => Op::Last,
        259..=262 => {
            let slot = usize::from(t.rep.decode(bits)).min(REP - 1);
            let len = u64::from(LENGTH_BASE[slot]) + 2 + extra(bits, LENGTH_BITS[slot]);
            Op::Repeat(n - 259, len)
        }
        263..=270 => {
            let i = n - 263;
            let dist = u64::from(SHORT_BASE[i]) + 1 + extra(bits, SHORT_BITS[i]);
            Op::Match(2, dist)
        }
        _ => {
            let slot = (n - 271).min(REP - 1);
            let mut len = u64::from(LENGTH_BASE[slot]) + 3 + extra(bits, LENGTH_BITS[slot]);
            let d = usize::from(t.dist.decode(bits)).min(DIST_BASE.len() - 1);
            let n = u32::from(DIST_BITS[d]);
            let mut dist = u64::from(DIST_BASE[d]) + 1;
            if d > 9 {
                // The high bits as they are, the lowest four by their table.
                if n > 4 {
                    dist += u64::from(bits.peek16() >> (20 - n)) << 4;
                    bits.skip(n - 4);
                }
                if low.repeats > 0 {
                    low.repeats -= 1;
                    dist += u64::from(low.last);
                } else {
                    let l = u32::from(t.low.decode(bits));
                    if l == 16 {
                        low.repeats = LOW_REPEATS - 1;
                        dist += u64::from(low.last);
                    } else {
                        dist += u64::from(l);
                        low.last = l;
                    }
                }
            } else {
                dist += extra(bits, n as u8);
            }
            // A far match is never short, so its length is counted from further up.
            if dist >= 0x2000 {
                len += 1;
                if dist >= 0x4_0000 {
                    len += 1;
                }
            }
            Op::Match(len, dist)
        }
    }
}

/// Hands on the part of `bytes`, which start at `from` in the stream, that is before `end`.
fn give(
    bytes: &[u8],
    from: u64,
    end: u64,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let n = end.saturating_sub(from).min(bytes.len() as u64) as usize;
    match bytes.get(..n) {
        Some(b) if !b.is_empty() => sink(b),
        _ => Ok(()),
    }
}

/// The decompressor's state, kept from one file to the next of a solid stream.
pub(super) struct Unpacker {
    win: Window,
    /// Bytes unpacked since the stream began.
    pos: u64,
    /// Bytes of those handed on to be written (or passed over, past a file's end).
    written: u64,
    /// Where the file being unpacked begins and ends.
    file_start: u64,
    file_end: u64,
    /// The last four distances, the newest first, and the last match's length.
    old: [u64; 4],
    last_len: u64,
    low: LowBits,
    tables: Option<Tables>,
    /// The code lengths of the last tables, which new ones may be given as changes from.
    lengths: [u8; ALL],
    /// The tables carry on into the next solid file (else it starts with its own).
    tables_read: bool,
    /// The stream is in PPMd, not LZ.
    ppm_mode: bool,
    vm: Vm,
    out: Vec<u8>,
    spare: Vec<u8>,
}

impl Unpacker {
    /// A stream whose first file needs the dictionary `dict`.
    pub(super) fn new(dict: u64) -> Unpacker {
        Unpacker {
            win: Window::new(dict),
            pos: 0,
            written: 0,
            file_start: 0,
            file_end: 0,
            old: [0; 4],
            last_len: 0,
            low: LowBits::default(),
            tables: None,
            lengths: [0; ALL],
            tables_read: false,
            ppm_mode: false,
            vm: Vm::default(),
            out: Vec::new(),
            spare: Vec::new(),
        }
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
        let mut input = Input {
            bits: Bits::new(Vec::new()),
            src: input,
            done: false,
        };
        input.fill()?;
        // A block waiting for a filter never reaches from one file into the next.
        self.vm.waiting.clear();
        self.file_start = self.pos;
        self.written = self.pos;
        self.file_end = self.pos.saturating_add(size);
        if !self.tables_read {
            self.read_tables(&mut input)?;
        }
        let mut due = self.pos + FLUSH;
        loop {
            input.fill()?;
            // Past its size (the padding of its last byte, read as codes) or at the end of
            // its bytes, the file is done.
            if self.pos > self.file_end || input.exhausted() {
                break;
            }
            if self.pos >= due {
                self.flush(sink)?;
                due = self.pos + FLUSH;
            }
            // Room for the longest match without overwriting what is not written yet.
            if self.pos + MAX_MATCH - self.written > self.win.size {
                return Err(damaged("a filter's block is larger than its window"));
            }
            let done = if self.ppm_mode {
                self.ppm_step(&mut input)?
            } else {
                self.lz_step(&mut input)?
            };
            if done {
                break;
            }
        }
        self.flush(sink)?;
        if self.pos < self.file_end || self.written < self.file_end {
            return Err(ends_early());
        }
        Ok(())
    }

    /// One code of an LZ block; whether the file ends with it.
    fn lz_step(&mut self, input: &mut Input) -> Result<bool, Error> {
        let Some(t) = self.tables.as_ref() else {
            return Err(damaged("a block has no tables"));
        };
        if self.pos == self.file_end {
            // At its size only an end of block may follow; anything else is the padding of
            // its last byte.
            let at = input.bits.at();
            let next = t.main.decode(&mut input.bits);
            input.bits.seek(at);
            if next != 256 {
                return Ok(true);
            }
        }
        match decode(t, &mut self.low, &mut input.bits) {
            Op::Literal(b) => {
                self.win.put(self.pos, b);
                self.pos += 1;
            }
            Op::Match(len, dist) => {
                self.old = [dist, self.old[0], self.old[1], self.old[2]];
                self.last_len = len;
                self.copy(len, dist)?;
            }
            Op::Repeat(n, len) => {
                let n = n.min(3);
                let dist = self.old[n];
                // It becomes the newest.
                self.old.copy_within(0..n, 1);
                self.old[0] = dist;
                self.last_len = len;
                self.copy(len, dist)?;
            }
            Op::Last if self.last_len > 0 => self.copy(self.last_len, self.old[0])?,
            Op::Last => {}
            Op::EndOfBlock => return self.end_of_block(input),
            Op::Filter => self.lz_filter(input)?,
        }
        Ok(false)
    }

    /// After the end of a block: "1", new tables follow; "00", the file ends; "01", the file
    /// ends and the next one starts with new tables. Whether the file ends.
    fn end_of_block(&mut self, input: &mut Input) -> Result<bool, Error> {
        let field = input.bits.peek16();
        let (new_tables, new_file) = if field & 0x8000 != 0 {
            input.bits.skip(1);
            (true, false)
        } else {
            input.bits.skip(2);
            (field & 0x4000 != 0, true)
        };
        self.tables_read = !new_tables;
        if new_file {
            return Ok(true);
        }
        match self.read_tables(input) {
            Ok(()) => Ok(false),
            // At its size what follows is not the file's: it ends there, as the format's own
            // reader has it, and its CRC is the judge.
            Err(Error::Unreadable(_)) if self.pos >= self.file_end => Ok(true),
            Err(e) => Err(e),
        }
    }

    /// New tables, or the switch to PPMd, at the next byte.
    fn read_tables(&mut self, input: &mut Input) -> Result<(), Error> {
        input.bits.align();
        let field = input.bits.peek16();
        if field & 0x8000 != 0 {
            self.ppm_mode = true;
            return self.ppm_start(input);
        }
        self.ppm_mode = false;
        self.low = LowBits::default();
        if field & 0x4000 == 0 {
            self.lengths = [0; ALL];
        }
        input.bits.skip(2);
        let t = tables::read(&mut input.bits, &mut self.lengths)?;
        self.tables_read = true;
        if input.over() {
            return Err(ends_early());
        }
        self.tables = Some(t);
        Ok(())
    }

    /// A filter's record in an LZ block: its first byte, its length, its bytes.
    fn lz_filter(&mut self, input: &mut Input) -> Result<(), Error> {
        let b = &mut input.bits;
        let first = b.take(8) as u8;
        let len = match (first & 7) + 1 {
            7 => b.take(8) + 7,
            8 => b.take(16),
            n => u32::from(n),
        };
        if len == 0 {
            return Err(damaged("a filter's record"));
        }
        let code: Vec<u8> = (0..len).map(|_| b.take(8) as u8).collect();
        if input.over() {
            return Err(ends_early());
        }
        self.vm.add(first, &code, self.pos)
    }

    /// The switch to PPMd.
    fn ppm_start(&mut self, _input: &mut Input) -> Result<(), Error> {
        Err(Error::Method("a RAR 4 archive packed with PPMd".into()))
    }

    /// One symbol of a PPMd block; whether the file ends with it.
    fn ppm_step(&mut self, _input: &mut Input) -> Result<bool, Error> {
        Err(Error::Method("a RAR 4 archive packed with PPMd".into()))
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

    /// Hands on what is unpacked, each filter's block undone once it is whole (and the
    /// filters after it given the same block, over what the first made of it), up to the
    /// first block that is not whole yet.
    fn flush(&mut self, sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        while self.written < self.pos {
            let Some(f) = self.vm.waiting.front().copied() else {
                return self.write(self.pos, sink);
            };
            if f.start < self.written {
                // It overlaps what is written already: no filter.
                self.vm.waiting.pop_front();
                continue;
            }
            if f.start > self.written {
                self.write(f.start.min(self.pos), sink)?;
                continue;
            }
            let end = f.start + u64::from(f.len);
            if end > self.pos {
                break;
            }
            self.vm.waiting.pop_front();
            self.out.clear();
            self.win.copy_out(f.start, end, &mut self.out)?;
            // The format counts the place in its file in 32 bits.
            let offset = (f.start - self.file_start) as u32;
            vm::run(&f, offset, &mut self.out, &mut self.spare);
            while let Some(next) = self.vm.waiting.front().copied() {
                if next.start != f.start || next.len as usize != self.out.len() {
                    break;
                }
                self.vm.waiting.pop_front();
                vm::run(&next, offset, &mut self.out, &mut self.spare);
            }
            give(&self.out, f.start, self.file_end, sink)?;
            self.written = end;
        }
        Ok(())
    }

    /// Hands on the bytes unpacked up to `to`, as they are.
    fn write(
        &mut self,
        to: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let to_give = to.min(self.file_end.max(self.written));
        self.out.clear();
        self.win.copy_out(self.written, to_give, &mut self.out)?;
        give(&self.out, self.written, self.file_end, sink)?;
        self.written = to;
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod writer;

#[cfg(test)]
mod tests;
