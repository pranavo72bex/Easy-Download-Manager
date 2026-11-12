// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's own LZMA and LZMA2 decoders, written from the formats' public description (the LZMA
//! specification that comes with the public-domain LZMA SDK, and the LZMA2 chunk layout), for
//! the 7z reader (gaps row F10).
//!
//! Both are readers: each `read` decodes at most what the caller's buffer holds, so a solid
//! folder of several gigabytes is unpacked in a fixed amount of memory (the dictionary, grown
//! only as far as bytes are actually written). The input is outside data, so every count is
//! checked: a distance before the first byte, a chunk that says one size and holds another,
//! input that runs out or runs past its end are errors, never a panic or a read out of bounds.

use std::io::{self, Read};

/// An error in the packed data: the archive is damaged or is not what it says.
fn damaged(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the packed data is damaged ({what})"),
    )
}

/// Bytes of packed input, read through a buffer of its own, counted, and stopped at a limit
/// (an LZMA2 chunk's end) so a damaged chunk cannot read into the next one.
pub(super) struct Input {
    reader: Box<dyn Read>,
    buf: Box<[u8]>,
    at: usize,
    len: usize,
    /// Bytes taken so far.
    used: u64,
    /// The most `used` may reach; `u64::MAX` where nothing nearer is known.
    limit: u64,
}

impl Input {
    pub(super) fn new(reader: Box<dyn Read>) -> Input {
        Input {
            reader,
            buf: vec![0u8; 64 * 1024].into_boxed_slice(),
            at: 0,
            len: 0,
            used: 0,
            limit: u64::MAX,
        }
    }

    /// The next byte.
    fn byte(&mut self) -> io::Result<u8> {
        if self.used >= self.limit {
            return Err(damaged("a chunk runs past its own size"));
        }
        if self.at >= self.len {
            let n = self.reader.read(&mut self.buf)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the archive ends in the middle of its packed data",
                ));
            }
            self.at = 0;
            self.len = n;
        }
        let b = self.buf.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        self.used += 1;
        Ok(b)
    }
}

/// The dictionary: the bytes written last, which a match copies from. It grows as bytes are
/// written, up to its size, and then wraps, so a small file never costs a large dictionary.
pub(super) struct Window {
    buf: Vec<u8>,
    cap: usize,
    /// Where the next byte goes.
    pos: usize,
    /// Bytes written since the last reset (an LZMA2 dictionary reset); its low bits pick
    /// the literal and position contexts.
    total: u64,
}

impl Window {
    pub(super) fn new(size: u32) -> Window {
        Window {
            buf: Vec::new(),
            // The format's smallest dictionary is 4 KiB.
            cap: (size as usize).max(4096),
            pos: 0,
            total: 0,
        }
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.pos = 0;
        self.total = 0;
    }

    /// How many bytes back a match may reach.
    fn filled(&self) -> usize {
        self.buf.len()
    }

    fn put(&mut self, b: u8) {
        match self.buf.get_mut(self.pos) {
            Some(slot) => *slot = b,
            // Still growing: `pos` is the end.
            None => self.buf.push(b),
        }
        self.pos += 1;
        if self.pos >= self.cap {
            self.pos = 0;
        }
        self.total += 1;
    }

    /// The byte `dist` back (1 is the last one written). The caller has checked
    /// `dist <= filled()`.
    fn back(&self, dist: usize) -> u8 {
        let i = if self.pos >= dist {
            self.pos - dist
        } else {
            // Only a full window wraps, so its length is its size here.
            (self.buf.len() + self.pos).wrapping_sub(dist)
        };
        self.buf.get(i).copied().unwrap_or(0)
    }

    /// Copies the last `out.len()` bytes written into `out` (at most the window's size).
    fn recent(&self, out: &mut [u8]) {
        let n = out.len();
        if self.pos >= n {
            if let Some(src) = self.buf.get(self.pos - n..self.pos) {
                out.copy_from_slice(src);
            }
        } else {
            // The bytes wrap: the end of the buffer, then its start.
            let tail = n - self.pos;
            let from = self.buf.len().saturating_sub(tail);
            if let (Some(a), Some(b)) = (self.buf.get(from..), self.buf.get(..self.pos)) {
                let (first, second) = out.split_at_mut(a.len().min(n));
                first.copy_from_slice(a.get(..first.len()).unwrap_or_default());
                second.copy_from_slice(b.get(..second.len()).unwrap_or_default());
            }
        }
    }
}

/// A probability that the next bit is 0, out of 2048; every one starts even.
const HALF: u16 = 1024;

/// The range decoder: the arithmetic decoding underneath LZMA.
#[derive(Default)]
struct Range {
    range: u32,
    code: u32,
}

impl Range {
    /// Starts on a stream: a zero byte, then the first four bytes of the code.
    fn init(&mut self, input: &mut Input) -> io::Result<()> {
        if input.byte()? != 0 {
            return Err(damaged("a compressed stream does not start as one"));
        }
        self.range = u32::MAX;
        self.code = 0;
        for _ in 0..4 {
            self.code = (self.code << 8) | u32::from(input.byte()?);
        }
        if self.code == self.range {
            return Err(damaged("a compressed stream does not start as one"));
        }
        Ok(())
    }

    fn normalize(&mut self, input: &mut Input) -> io::Result<()> {
        if self.range < 1 << 24 {
            self.range <<= 8;
            self.code = (self.code << 8) | u32::from(input.byte()?);
        }
        Ok(())
    }

    /// One bit by its probability, which it then moves toward what was seen.
    fn bit(&mut self, p: &mut u16, input: &mut Input) -> io::Result<u32> {
        // `range >> 11` is under 2^21 and `p` under 2^11, so this does not overflow.
        let bound = (self.range >> 11) * u32::from(*p);
        let bit = if self.code < bound {
            self.range = bound;
            *p += (2048 - *p) >> 5;
            0
        } else {
            self.range -= bound;
            self.code -= bound;
            *p -= *p >> 5;
            1
        };
        self.normalize(input)?;
        Ok(bit)
    }

    /// `count` bits each as likely 0 as 1, the highest first.
    fn direct(&mut self, count: u32, input: &mut Input) -> io::Result<u32> {
        let mut out = 0u32;
        for _ in 0..count {
            self.range >>= 1;
            self.code = self.code.wrapping_sub(self.range);
            // All ones where the subtraction went below zero (the bit was 0), else zero.
            let t = 0u32.wrapping_sub(self.code >> 31);
            self.code = self.code.wrapping_add(self.range & t);
            self.normalize(input)?;
            out = (out << 1).wrapping_add(t.wrapping_add(1));
        }
        Ok(out)
    }

    /// A number of `bits` bits through a tree of probabilities, the highest bit first.
    /// `probs` has `1 << bits` entries.
    fn tree(&mut self, probs: &mut [u16], bits: u32, input: &mut Input) -> io::Result<u32> {
        let mut m = 1usize;
        for _ in 0..bits {
            let p = probs
                .get_mut(m)
                .ok_or_else(|| damaged("a probability out of range"))?;
            m = (m << 1) | self.bit(p, input)? as usize;
        }
        Ok((m - (1 << bits)) as u32)
    }

    /// As [`Range::tree`], the lowest bit first.
    fn reverse(&mut self, probs: &mut [u16], bits: u32, input: &mut Input) -> io::Result<u32> {
        let mut m = 1usize;
        let mut out = 0u32;
        for i in 0..bits {
            let p = probs
                .get_mut(m)
                .ok_or_else(|| damaged("a probability out of range"))?;
            let bit = self.bit(p, input)?;
            m = (m << 1) | bit as usize;
            out |= bit << i;
        }
        Ok(out)
    }
}

/// The probabilities a length is read with: 2 to 273, as 0 to 271.
struct LenProbs {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 16],
    mid: [[u16; 8]; 16],
    high: [u16; 256],
}

impl LenProbs {
    fn new() -> LenProbs {
        LenProbs {
            choice: HALF,
            choice2: HALF,
            low: [[HALF; 8]; 16],
            mid: [[HALF; 8]; 16],
            high: [HALF; 256],
        }
    }

    fn decode(&mut self, rc: &mut Range, pos_state: usize, input: &mut Input) -> io::Result<u32> {
        // `pos_state` is masked by `pb` (at most 4 bits), so it is under 16.
        let ps = pos_state & 15;
        if rc.bit(&mut self.choice, input)? == 0 {
            return rc.tree(&mut self.low[ps], 3, input);
        }
        if rc.bit(&mut self.choice2, input)? == 0 {
            return Ok(8 + rc.tree(&mut self.mid[ps], 3, input)?);
        }
        Ok(16 + rc.tree(&mut self.high, 8, input)?)
    }
}

/// Twelve states (what the last few symbols were) times sixteen positions.
const STATES: usize = 12;

/// Every probability LZMA keeps.
struct Probs {
    is_match: [u16; STATES * 16],
    is_rep: [u16; STATES],
    is_rep_g0: [u16; STATES],
    is_rep_g1: [u16; STATES],
    is_rep_g2: [u16; STATES],
    is_rep0_long: [u16; STATES * 16],
    pos_slot: [[u16; 64]; 4],
    /// Distances with slots 4 to 13: 1 + 128 - 14 entries.
    pos_special: [u16; 115],
    align: [u16; 16],
    len: LenProbs,
    rep_len: LenProbs,
    /// 0x300 for each literal context.
    literal: Vec<u16>,
}

impl Probs {
    fn new(lc: u32, lp: u32) -> Probs {
        Probs {
            is_match: [HALF; STATES * 16],
            is_rep: [HALF; STATES],
            is_rep_g0: [HALF; STATES],
            is_rep_g1: [HALF; STATES],
            is_rep_g2: [HALF; STATES],
            is_rep0_long: [HALF; STATES * 16],
            pos_slot: [[HALF; 64]; 4],
            pos_special: [HALF; 115],
            align: [HALF; 16],
            len: LenProbs::new(),
            rep_len: LenProbs::new(),
            literal: vec![HALF; 0x300 << (lc + lp)],
        }
    }
}

/// The literal, position and position-bit counts, from a properties byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Props {
    pub(super) lc: u32,
    pub(super) lp: u32,
    pub(super) pb: u32,
}

impl Props {
    /// `lc + lp * 9 + pb * 45`, the format's packing of the three.
    pub(super) fn from_byte(b: u8) -> io::Result<Props> {
        if b >= 9 * 5 * 5 {
            return Err(damaged("its properties are out of range"));
        }
        let b = u32::from(b);
        Ok(Props {
            lc: b % 9,
            lp: (b / 9) % 5,
            pb: b / 45,
        })
    }
}

/// The LZMA decoder's state: its probabilities, the state machine and the last four
/// distances. The dictionary and the range decoder are kept apart, as LZMA2 resets each
/// on its own.
struct Lzma {
    props: Props,
    probs: Box<Probs>,
    state: usize,
    reps: [u32; 4],
    /// Bytes of a match still to copy, where a call stopped in the middle of one.
    pending: usize,
}

impl Lzma {
    fn new(props: Props) -> Lzma {
        Lzma {
            props,
            probs: Box::new(Probs::new(props.lc, props.lp)),
            state: 0,
            reps: [0; 4],
            pending: 0,
        }
    }

    /// Every probability back to even, the state and distances to zero.
    fn reset(&mut self, props: Props) {
        self.props = props;
        *self.probs = Probs::new(props.lc, props.lp);
        self.state = 0;
        self.reps = [0; 4];
        self.pending = 0;
    }

    /// Decodes up to `want` bytes into `win`; says how many it made and whether it met the
    /// end marker (after which it makes no more).
    fn decode(
        &mut self,
        rc: &mut Range,
        input: &mut Input,
        win: &mut Window,
        want: usize,
    ) -> io::Result<(usize, bool)> {
        let mut made = 0usize;
        let pb_mask = (1usize << self.props.pb) - 1;
        // Finish a match a previous call stopped in.
        while self.pending > 0 && made < want {
            win.put(win.back(self.reps[0] as usize + 1));
            self.pending -= 1;
            made += 1;
        }
        while made < want {
            let pos_state = (win.total as usize) & pb_mask;
            // `state` is under 12 and `pos_state` under 16: inside the 192 entries.
            let at = (self.state << 4) | pos_state;
            if rc.bit(&mut self.probs.is_match[at], input)? == 0 {
                self.literal(rc, input, win)?;
                self.state = match self.state {
                    0..=3 => 0,
                    4..=9 => self.state - 3,
                    _ => self.state - 6,
                };
                made += 1;
                continue;
            }
            let len;
            if rc.bit(&mut self.probs.is_rep[self.state], input)? != 0 {
                // A repeat of one of the last four distances.
                if win.filled() == 0 {
                    return Err(damaged("a repeat before any byte"));
                }
                if rc.bit(&mut self.probs.is_rep_g0[self.state], input)? == 0 {
                    if rc.bit(&mut self.probs.is_rep0_long[at], input)? == 0 {
                        // One byte from the last distance.
                        if self.reps[0] as usize >= win.filled() {
                            return Err(damaged("a distance before the first byte"));
                        }
                        self.state = if self.state < 7 { 9 } else { 11 };
                        win.put(win.back(self.reps[0] as usize + 1));
                        made += 1;
                        continue;
                    }
                } else {
                    let dist;
                    if rc.bit(&mut self.probs.is_rep_g1[self.state], input)? == 0 {
                        dist = self.reps[1];
                    } else {
                        if rc.bit(&mut self.probs.is_rep_g2[self.state], input)? == 0 {
                            dist = self.reps[2];
                        } else {
                            dist = self.reps[3];
                            self.reps[3] = self.reps[2];
                        }
                        self.reps[2] = self.reps[1];
                    }
                    self.reps[1] = self.reps[0];
                    self.reps[0] = dist;
                }
                len = self.probs.rep_len.decode(rc, pos_state, input)?;
                self.state = if self.state < 7 { 8 } else { 11 };
            } else {
                // A new distance.
                self.reps[3] = self.reps[2];
                self.reps[2] = self.reps[1];
                self.reps[1] = self.reps[0];
                len = self.probs.len.decode(rc, pos_state, input)?;
                self.state = if self.state < 7 { 7 } else { 10 };
                self.reps[0] = self.distance(rc, len, input)?;
                if self.reps[0] == u32::MAX {
                    // The end marker.
                    return Ok((made, true));
                }
            }
            if self.reps[0] as usize >= win.filled() {
                return Err(damaged("a distance before the first byte"));
            }
            let len = len as usize + 2;
            let now = len.min(want - made);
            let dist = self.reps[0] as usize + 1;
            for _ in 0..now {
                win.put(win.back(dist));
            }
            made += now;
            self.pending = len - now;
        }
        Ok((made, false))
    }

    fn literal(&mut self, rc: &mut Range, input: &mut Input, win: &mut Window) -> io::Result<()> {
        let Props { lc, lp, .. } = self.props;
        let prev = if win.filled() > 0 { win.back(1) } else { 0 };
        let context = ((((win.total as u32) & ((1 << lp) - 1)) << lc)
            + (u32::from(prev) >> (8 - lc))) as usize;
        let base = context * 0x300;
        let probs = self
            .probs
            .literal
            .get_mut(base..base + 0x300)
            .ok_or_else(|| damaged("a literal out of range"))?;
        let mut sym = 1usize;
        if self.state >= 7 {
            // After a match the byte at the last distance guides the first bits.
            if self.reps[0] as usize >= win.filled() {
                return Err(damaged("a distance before the first byte"));
            }
            let mut match_byte = win.back(self.reps[0] as usize + 1) as usize;
            while sym < 0x100 {
                let match_bit = (match_byte >> 7) & 1;
                match_byte <<= 1;
                // `sym` is under 0x100, so this is under 0x300.
                let bit = rc.bit(&mut probs[((1 + match_bit) << 8) + sym], input)? as usize;
                sym = (sym << 1) | bit;
                if match_bit != bit {
                    break;
                }
            }
        }
        while sym < 0x100 {
            sym = (sym << 1) | rc.bit(&mut probs[sym], input)? as usize;
        }
        win.put((sym - 0x100) as u8);
        Ok(())
    }

    /// A match's distance, less one; `u32::MAX` is the end marker.
    fn distance(&mut self, rc: &mut Range, len: u32, input: &mut Input) -> io::Result<u32> {
        let len_state = (len as usize).min(3);
        let slot = rc.tree(&mut self.probs.pos_slot[len_state], 6, input)?;
        if slot < 4 {
            return Ok(slot);
        }
        let direct = (slot >> 1) - 1;
        let mut dist = (2 | (slot & 1)) << direct;
        if slot < 14 {
            // Slots 4 to 13 have their low bits in a table of their own, at `dist - slot`.
            let base = (dist - slot) as usize;
            let probs = self
                .probs
                .pos_special
                .get_mut(base..)
                .ok_or_else(|| damaged("a distance out of range"))?;
            dist = dist.wrapping_add(rc.reverse(probs, direct, input)?);
        } else {
            dist = dist.wrapping_add(rc.direct(direct - 4, input)? << 4);
            dist = dist.wrapping_add(rc.reverse(&mut self.probs.align, 4, input)?);
        }
        Ok(dist)
    }
}

/// An LZMA stream (7z's coder 03 01 01) of a known unpacked size, as a reader.
pub(super) struct LzmaReader {
    input: Input,
    rc: Range,
    lz: Lzma,
    win: Window,
    left: u64,
}

impl LzmaReader {
    /// `props` are the coder's five property bytes: the packed `lc`, `lp`, `pb`, then the
    /// dictionary size.
    pub(super) fn new(reader: Box<dyn Read>, props: &[u8], size: u64) -> io::Result<LzmaReader> {
        let (&first, dict) = props
            .split_first()
            .filter(|(_, d)| d.len() == 4)
            .ok_or_else(|| damaged("its properties are not five bytes"))?;
        let dict = u32::from_le_bytes([dict[0], dict[1], dict[2], dict[3]]);
        let mut input = Input::new(reader);
        let mut rc = Range::default();
        if size > 0 {
            rc.init(&mut input)?;
        }
        Ok(LzmaReader {
            input,
            rc,
            lz: Lzma::new(Props::from_byte(first)?),
            win: Window::new(dict),
            left: size,
        })
    }
}

impl Read for LzmaReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let want = (buf.len() as u64).min(self.left).min(self.win.cap as u64) as usize;
        if want == 0 {
            return Ok(0);
        }
        let (made, marker) = self
            .lz
            .decode(&mut self.rc, &mut self.input, &mut self.win, want)?;
        // The end marker may close the stream only where its size says it ends.
        if made == 0 || (marker && made as u64 != self.left) {
            return Err(damaged("the stream ends before all its bytes"));
        }
        if let Some(out) = buf.get_mut(..made) {
            self.win.recent(out);
        }
        self.left -= made as u64;
        Ok(made)
    }
}

/// Where an LZMA2 stream is.
enum Chunk {
    /// Before a chunk's control byte.
    Control,
    /// Inside a compressed chunk: bytes it still makes, and where its packed bytes end.
    Lzma { left: usize, end: u64 },
    /// Inside a stored chunk: bytes still to copy.
    Copy { left: usize },
    /// After the end byte.
    Done,
}

/// An LZMA2 stream (7z's coder 21) of a known unpacked size, as a reader: a run of chunks,
/// each stored or LZMA-compressed, which may reset the dictionary, the state, or the
/// properties as it starts.
pub(super) struct Lzma2Reader {
    input: Input,
    rc: Range,
    lz: Lzma,
    win: Window,
    chunk: Chunk,
    need_dict_reset: bool,
    need_props: bool,
    left: u64,
}

impl Lzma2Reader {
    /// `prop` is the coder's one property byte: the dictionary size, as a code up to 40.
    pub(super) fn new(reader: Box<dyn Read>, prop: u8, size: u64) -> io::Result<Lzma2Reader> {
        if prop > 40 {
            return Err(damaged("its dictionary size is out of range"));
        }
        let dict = if prop == 40 {
            u32::MAX
        } else {
            (2 | u32::from(prop & 1)) << (prop / 2 + 11)
        };
        let props = Props {
            lc: 0,
            lp: 0,
            pb: 0,
        };
        Ok(Lzma2Reader {
            input: Input::new(reader),
            rc: Range::default(),
            lz: Lzma::new(props),
            win: Window::new(dict),
            chunk: Chunk::Control,
            need_dict_reset: true,
            need_props: true,
            left: size,
        })
    }

    /// Reads a control byte and the chunk header after it.
    fn start_chunk(&mut self) -> io::Result<()> {
        let control = self.input.byte()?;
        if control == 0 {
            if self.left != 0 {
                return Err(damaged("the stream ends before all its bytes"));
            }
            self.chunk = Chunk::Done;
            return Ok(());
        }
        if control >= 0xE0 || control == 1 {
            // A dictionary reset; the next compressed chunk must set its properties.
            self.need_props = true;
            self.need_dict_reset = false;
            self.win.reset();
        } else if self.need_dict_reset {
            return Err(damaged("the first chunk does not reset the dictionary"));
        }
        if control >= 0x80 {
            let b1 = self.input.byte()?;
            let b2 = self.input.byte()?;
            let unpacked =
                ((usize::from(control & 0x1F) << 16) | (usize::from(b1) << 8) | usize::from(b2))
                    + 1;
            let b3 = self.input.byte()?;
            let b4 = self.input.byte()?;
            let packed = ((u64::from(b3) << 8) | u64::from(b4)) + 1;
            if control >= 0xC0 {
                let props = Props::from_byte(self.input.byte()?)?;
                if props.lc + props.lp > 4 {
                    return Err(damaged("its properties are out of range"));
                }
                self.need_props = false;
                self.lz.reset(props);
            } else if self.need_props {
                return Err(damaged("a chunk has no properties"));
            } else if control >= 0xA0 {
                self.lz.reset(self.lz.props);
            }
            if unpacked as u64 > self.left {
                return Err(damaged("a chunk is larger than the stream"));
            }
            let end = self.input.used + packed;
            self.input.limit = end;
            self.rc.init(&mut self.input)?;
            self.chunk = Chunk::Lzma {
                left: unpacked,
                end,
            };
        } else {
            if control > 2 {
                return Err(damaged("a chunk of no known kind"));
            }
            let b1 = self.input.byte()?;
            let b2 = self.input.byte()?;
            let size = ((usize::from(b1) << 8) | usize::from(b2)) + 1;
            if size as u64 > self.left {
                return Err(damaged("a chunk is larger than the stream"));
            }
            self.chunk = Chunk::Copy { left: size };
        }
        Ok(())
    }
}

impl Read for Lzma2Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.left == 0 || buf.is_empty() {
                return Ok(0);
            }
            match self.chunk {
                Chunk::Done => return Err(damaged("the stream ends before all its bytes")),
                Chunk::Control => self.start_chunk()?,
                Chunk::Lzma { left, end } => {
                    let want = buf.len().min(left).min(self.win.cap);
                    let (made, marker) =
                        self.lz
                            .decode(&mut self.rc, &mut self.input, &mut self.win, want)?;
                    if marker || made == 0 {
                        return Err(damaged("a chunk ends before its size"));
                    }
                    if let Some(out) = buf.get_mut(..made) {
                        self.win.recent(out);
                    }
                    let left = left - made;
                    self.left -= made as u64;
                    if left == 0 {
                        // A chunk's bytes are exactly its packed size; a match may not
                        // run on into the next chunk.
                        if self.input.used != end || self.lz.pending != 0 {
                            return Err(damaged("a chunk is not the size it says"));
                        }
                        self.input.limit = u64::MAX;
                        self.chunk = Chunk::Control;
                    } else {
                        self.chunk = Chunk::Lzma { left, end };
                    }
                    return Ok(made);
                }
                Chunk::Copy { left } => {
                    let n = buf.len().min(left);
                    for slot in buf.iter_mut().take(n) {
                        let b = self.input.byte()?;
                        self.win.put(b);
                        *slot = b;
                    }
                    self.left -= n as u64;
                    self.chunk = if left == n {
                        Chunk::Control
                    } else {
                        Chunk::Copy { left: left - n }
                    };
                    return Ok(n);
                }
            }
        }
    }
}
