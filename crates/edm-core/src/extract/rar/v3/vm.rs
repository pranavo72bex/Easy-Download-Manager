// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! RAR 4's filters. The format carries each filter as a small program for a virtual machine,
//! but its packer only ever writes a few standard ones, which a reader knows by the program's
//! length and CRC32: E8, E8E9, DELTA, RGB, AUDIO and ITANIUM. EDM runs no program: it knows
//! the standard ones by that mark and undoes E8, E8E9, DELTA, RGB and AUDIO in its own code;
//! ITANIUM and any other program are refused in words.
//!
//! A filter's record says which filter (a new one, with its program, or one given before),
//! where its block starts from the place it is read, the block's length (or the length that
//! filter had last), and its registers; a filter of RAR 4 lives for the stream, while the
//! blocks waiting for it are dropped at each file's start.

use std::collections::VecDeque;

use super::super::super::Error;
use super::super::filters;
use super::super::header::damaged;
use super::super::huffman::Bits;

/// The machine's memory: no filter's block is larger.
pub(super) const MEMORY: u32 = 0x4_0000;
/// The most filters known, and blocks waiting, at once.
const MOST: usize = 8192;

/// A standard filter, by what it undoes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    E8,
    E8E9,
    Delta,
    Rgb,
    Audio,
}

/// A block waiting for its filter: where it starts in the stream, its length, the filter
/// and its first seven registers.
#[derive(Debug, Clone, Copy)]
pub(super) struct Pending {
    pub start: u64,
    pub len: u32,
    kind: Kind,
    regs: [u32; 7],
}

/// The filters given so far in a stream, and the blocks waiting.
#[derive(Default)]
pub(super) struct Vm {
    kinds: Vec<Kind>,
    /// Each filter's last block length.
    lens: Vec<u32>,
    last: usize,
    pub waiting: VecDeque<Pending>,
}

/// A number in a filter's record: two bits for its size, then 4, 8 (or a negative 8), 16 or
/// 32 bits.
pub(super) fn number(b: &mut Bits) -> u32 {
    let data = b.peek16();
    match data & 0xc000 {
        0 => {
            b.skip(6);
            (data >> 10) & 0x0f
        }
        0x4000 if data & 0x3c00 == 0 => {
            b.skip(14);
            0xffff_ff00 | ((data >> 2) & 0xff)
        }
        0x4000 => {
            b.skip(10);
            (data >> 6) & 0xff
        }
        0x8000 => {
            b.skip(2);
            let v = b.peek16();
            b.skip(16);
            v
        }
        _ => {
            b.skip(2);
            let high = b.peek16() << 16;
            b.skip(16);
            let low = b.peek16();
            b.skip(16);
            high | low
        }
    }
}

/// Which standard filter `program` is: its first byte is the XOR of the others, and its
/// length and CRC32 are the standard one's.
pub(super) fn recognise(program: &[u8]) -> Result<Kind, Error> {
    let xor = program.iter().skip(1).fold(0u8, |a, &b| a ^ b);
    if program.first() == Some(&xor) {
        match (program.len(), crc32fast::hash(program)) {
            (53, 0xad57_6887) => return Ok(Kind::E8),
            (57, 0x3cd7_e57e) => return Ok(Kind::E8E9),
            (29, 0x0e06_077d) => return Ok(Kind::Delta),
            (149, 0x1c2c_5dc8) => return Ok(Kind::Rgb),
            (216, 0xbc85_e701) => return Ok(Kind::Audio),
            (120, 0x3769_893f) => {
                return Err(Error::Method("RAR 4's filter for Itanium code".into()));
            }
            _ => {}
        }
    }
    Err(Error::Method(
        "a RAR 4 filter program that is none of the standard ones".into(),
    ))
}

impl Vm {
    /// Forgets every filter, as a stream that is not solid starts.
    pub(super) fn reset(&mut self) {
        self.kinds.clear();
        self.lens.clear();
        self.last = 0;
        self.waiting.clear();
    }

    /// A filter's record, `first` its first byte and `code` the bytes after it, read where
    /// `pos` bytes of the stream are unpacked.
    pub(super) fn add(&mut self, first: u8, code: &[u8], pos: u64) -> Result<(), Error> {
        let mut b = Bits::new(code.to_vec());
        let at = if first & 0x80 != 0 {
            match number(&mut b) {
                // Zero forgets all filters before; the first is then the new one.
                0 => {
                    self.reset();
                    0
                }
                n => (n - 1) as usize,
            }
        } else {
            self.last
        };
        if at > self.kinds.len() || at > self.lens.len() || at > MOST {
            return Err(damaged("a filter that was never given"));
        }
        self.last = at;
        let new = at == self.kinds.len();
        if new {
            self.lens.push(0);
        }
        if self.waiting.len() >= MOST {
            return Err(damaged("too many filters at once"));
        }
        let mut start = u64::from(number(&mut b));
        if first & 0x40 != 0 {
            start += 258;
        }
        let len = if first & 0x20 != 0 {
            let len = number(&mut b);
            if let Some(slot) = self.lens.get_mut(at) {
                *slot = len;
            }
            len
        } else {
            self.lens.get(at).copied().unwrap_or(0)
        };
        let mut regs = [0u32; 7];
        regs[4] = len;
        if first & 0x10 != 0 {
            let mask = b.take(7);
            for (i, reg) in regs.iter_mut().enumerate() {
                if mask & (1 << i) != 0 {
                    *reg = number(&mut b);
                }
            }
        }
        let kind = if new {
            let size = number(&mut b);
            if size == 0 || size >= 0x1_0000 || b.at() / 8 + u64::from(size) > code.len() as u64 {
                return Err(damaged("a filter's program"));
            }
            let program: Vec<u8> = (0..size).map(|_| b.take(8) as u8).collect();
            let kind = recognise(&program)?;
            self.kinds.push(kind);
            kind
        } else {
            match self.kinds.get(at) {
                Some(&k) => k,
                None => return Err(damaged("a filter that was never given")),
            }
        };
        // The data a program may be given after its record is no standard filter's.
        if len > MEMORY || regs[4] != len {
            return Err(damaged("a filter's block is larger than the format allows"));
        }
        self.waiting.push_back(Pending {
            start: pos + start,
            len,
            kind,
            regs,
        });
        Ok(())
    }
}

/// Undoes `f` over its block `data`, which starts `offset` bytes into its file; `spare` is
/// a second buffer. A filter whose registers the standard program would refuse leaves the
/// block as it is, as that program does.
pub(super) fn run(f: &Pending, offset: u32, data: &mut Vec<u8>, spare: &mut Vec<u8>) {
    let size = data.len() as u32;
    match f.kind {
        Kind::E8 | Kind::E8E9 if size >= 4 => {
            filters::x86_rar4(data, offset, f.kind == Kind::E8E9);
        }
        Kind::Delta if size <= MEMORY / 2 && (1..=1024).contains(&f.regs[0]) => {
            filters::undo(filters::DELTA, f.regs[0] as usize, 0, data, spare);
        }
        Kind::Rgb => rgb(data, spare, f.regs[0], f.regs[1]),
        Kind::Audio => audio(data, spare, f.regs[0]),
        _ => {}
    }
}

/// RGB: three channels kept as differences from a prediction by the pixel to the left, the
/// one above and the one above to the left (`width` bytes a row, plus 3), green then taken
/// back out of red and blue from byte `red` on.
fn rgb(data: &mut Vec<u8>, spare: &mut Vec<u8>, width: u32, red: u32) {
    let size = data.len();
    let width = width.wrapping_sub(3) as usize;
    if size > (MEMORY / 2) as usize || size < 3 || width > size || red > 2 {
        return;
    }
    spare.clear();
    spare.resize(size, 0);
    let mut src = data.iter().copied();
    for channel in 0..3 {
        let mut prev = 0u32;
        let mut i = channel;
        while i < size {
            let predicted = if i >= width + 3 {
                let upper = u32::from(spare[i - width]);
                let upper_left = u32::from(spare[i - width - 3]);
                let p = prev.wrapping_add(upper).wrapping_sub(upper_left);
                let pa = (p.wrapping_sub(prev) as i32).unsigned_abs();
                let pb = (p.wrapping_sub(upper) as i32).unsigned_abs();
                let pc = (p.wrapping_sub(upper_left) as i32).unsigned_abs();
                if pa <= pb && pa <= pc {
                    prev
                } else if pb <= pc {
                    upper
                } else {
                    upper_left
                }
            } else {
                prev
            };
            let b = predicted.wrapping_sub(u32::from(src.next().unwrap_or(0))) as u8;
            spare[i] = b;
            prev = u32::from(b);
            i += 3;
        }
    }
    let mut i = red as usize;
    while i + 2 < size {
        let green = spare[i + 1];
        spare[i] = spare[i].wrapping_add(green);
        spare[i + 2] = spare[i + 2].wrapping_add(green);
        i += 3;
    }
    std::mem::swap(data, spare);
}

/// AUDIO: each channel's bytes kept as differences from a prediction by the three changes
/// before, whose weights adapt every 32 bytes to whichever would have erred least.
fn audio(data: &mut Vec<u8>, spare: &mut Vec<u8>, channels: u32) {
    let size = data.len();
    if size > (MEMORY / 2) as usize || channels == 0 || channels > 128 {
        return;
    }
    let channels = channels as usize;
    spare.clear();
    spare.resize(size, 0);
    let mut src = data.iter().copied();
    for channel in 0..channels {
        let (mut prev_byte, mut prev_delta) = (0u32, 0i32);
        let mut dif = [0u32; 7];
        let (mut d1, mut d2) = (0i32, 0i32);
        let mut k = [0i32; 3];
        let (mut i, mut count) = (channel, 0u32);
        while i < size {
            let d3 = d2;
            d2 = prev_delta.wrapping_sub(d1);
            d1 = prev_delta;
            let predicted = prev_byte
                .wrapping_mul(8)
                .wrapping_add((k[0] * d1) as u32)
                .wrapping_add((k[1] * d2) as u32)
                .wrapping_add((k[2] * d3) as u32);
            let predicted = (predicted >> 3) & 0xff;
            let cur = src.next().unwrap_or(0);
            let predicted = predicted.wrapping_sub(u32::from(cur));
            spare[i] = predicted as u8;
            prev_delta = i32::from(predicted.wrapping_sub(prev_byte) as u8 as i8);
            prev_byte = predicted;
            let d = i32::from(cur as i8) << 3;
            for (j, e) in [d, d - d1, d + d1, d - d2, d + d2, d - d3, d + d3]
                .into_iter()
                .enumerate()
            {
                dif[j] = dif[j].wrapping_add(e.unsigned_abs());
            }
            if count & 0x1f == 0 {
                adapt(&mut dif, &mut k);
            }
            i += channels;
            count += 1;
        }
    }
    std::mem::swap(data, spare);
}

/// AUDIO's weights moved one step towards the prediction that erred least, its errors reset.
fn adapt(dif: &mut [u32; 7], k: &mut [i32; 3]) {
    let (mut least, mut which) = (dif[0], 0usize);
    dif[0] = 0;
    for (j, d) in dif.iter_mut().enumerate().skip(1) {
        if *d < least {
            least = *d;
            which = j;
        }
        *d = 0;
    }
    if which == 0 {
        return;
    }
    let w = &mut k[(which - 1) / 2];
    if which % 2 == 1 {
        if *w >= -16 {
            *w -= 1;
        }
    } else if *w < 16 {
        *w += 1;
    }
}
