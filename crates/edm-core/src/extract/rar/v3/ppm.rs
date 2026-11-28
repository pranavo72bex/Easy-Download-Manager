// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! PPMd variant H, the context model RAR 4 may pack a block with, and the range coder it is
//! read through, from the model's public description.
//!
//! Each context (the last few bytes) keeps the bytes seen after it with their counts. A byte
//! is coded under the longest context that has it; where that context has not seen it, an
//! escape is coded and the next shorter context tried, the bytes already ruled out masked.
//! A context of one byte only is coded by a table of binary contexts; an escape's
//! probability comes from SEE contexts that learn it. After each byte the contexts are
//! extended (`update_model`), within the memory the stream gave (`alloc.rs`); when it is
//! full the model starts again. The decoder does exactly what the packer did, so the same
//! model is built on both sides.
//!
//! Every place read from the model's memory is checked as the format's own reader checks
//! it; damaged data ends the block (`None`), never a panic.

mod alloc;

use self::alloc::Alloc;
use super::super::huffman::Bits;

/// The longest context.
pub(super) const MAX_ORDER: u32 = 64;
const PERIOD_BITS: u32 = 7;
const TOT_BITS: u32 = 14;
const INTERVAL: u32 = 1 << 7;
const BIN_SCALE: u32 = 1 << TOT_BITS;
const MAX_FREQ: u32 = 124;
/// The range coder's bounds.
const TOP: u32 = 1 << 24;
const BOT: u32 = 1 << 15;
/// A state's size: its byte, its count and its successor.
const STATE: u32 = 6;

/// Escapes for an exponential distribution of bytes, by a binary context's count.
const EXP_ESCAPE: [u8; 16] = [25, 14, 9, 7, 5, 5, 4, 4, 4, 3, 3, 3, 2, 2, 2, 2];
const INIT_BIN_ESC: [u32; 8] = [
    0x3cdd, 0x1f3f, 0x59bf, 0x48f3, 0x64a1, 0x5abc, 0x6632, 0x6051,
];

/// The range coder's decoding side, reading bytes of the packed stream.
#[derive(Default)]
struct Coder {
    low: u32,
    code: u32,
    range: u32,
    low_count: u32,
    high_count: u32,
    scale: u32,
}

impl Coder {
    fn init(&mut self, bits: &mut Bits) {
        self.low = 0;
        self.code = 0;
        self.range = u32::MAX;
        for _ in 0..4 {
            self.code = (self.code << 8) | bits.take(8);
        }
    }

    /// Where the code falls among `scale` counts; `None` on damaged data.
    fn count(&mut self) -> Option<u32> {
        if self.scale == 0 {
            return None;
        }
        self.range /= self.scale;
        (self.range != 0).then(|| self.code.wrapping_sub(self.low) / self.range)
    }

    /// Where the code falls among 2^`shift` counts.
    fn shift_count(&mut self, shift: u32) -> Option<u32> {
        self.range >>= shift;
        (self.range != 0).then(|| self.code.wrapping_sub(self.low) / self.range)
    }

    /// Takes the counts from `low_count` to `high_count`.
    fn decode(&mut self) {
        self.low = self
            .low
            .wrapping_add(self.range.wrapping_mul(self.low_count));
        self.range = self
            .range
            .wrapping_mul(self.high_count.wrapping_sub(self.low_count));
    }

    fn normalize(&mut self, bits: &mut Bits) {
        loop {
            if self.low ^ self.low.wrapping_add(self.range) >= TOP {
                if self.range >= BOT {
                    return;
                }
                self.range = self.low.wrapping_neg() & (BOT - 1);
            }
            self.code = (self.code << 8) | bits.take(8);
            self.range <<= 8;
            self.low <<= 8;
        }
    }
}

/// A context that learns the probability of an escape.
#[derive(Debug, Clone, Copy, Default)]
struct See {
    summ: u16,
    shift: u8,
    count: u8,
}

impl See {
    fn new(init: u32) -> See {
        See {
            summ: (init << (PERIOD_BITS - 4)) as u16,
            shift: (PERIOD_BITS - 4) as u8,
            count: 4,
        }
    }

    fn mean(&mut self) -> u32 {
        let r = u32::from(self.summ) >> self.shift;
        self.summ = self.summ.wrapping_sub(r as u16);
        r + u32::from(r == 0)
    }

    fn update(&mut self) {
        if u32::from(self.shift) < PERIOD_BITS {
            self.count = self.count.wrapping_sub(1);
            if self.count == 0 {
                self.summ = self.summ.wrapping_add(self.summ);
                self.count = (3u32 << self.shift) as u8;
                self.shift += 1;
            }
        }
    }
}

/// A state as it is copied about: a byte, its count, its successor.
#[derive(Debug, Clone, Copy, Default)]
struct St {
    sym: u8,
    freq: u8,
    succ: u32,
}

/// The model, its memory and its coder.
pub(super) struct Model {
    a: Alloc,
    coder: Coder,
    min_ctx: u32,
    max_ctx: u32,
    found: u32,
    num_masked: u32,
    init_esc: u32,
    order_fall: i32,
    max_order: i32,
    run_length: i32,
    init_rl: i32,
    esc_count: u8,
    prev_success: u32,
    hi_bits_flag: u32,
    char_mask: [u8; 256],
    ns2indx: [u8; 256],
    ns2bsindx: [u8; 256],
    hb2flag: [u8; 256],
    bin_summ: Vec<[u16; 64]>,
    see: Vec<[See; 16]>,
}

impl Model {
    /// A model of `size` bytes of memory, not yet started.
    pub(super) fn new(size: u32) -> Model {
        let mut ns2indx = [0u8; 256];
        let (mut m, mut k, mut step) = (3u32, 1u32, 1u32);
        for (i, slot) in ns2indx.iter_mut().enumerate() {
            if i < 3 {
                *slot = i as u8;
                continue;
            }
            *slot = m as u8;
            k -= 1;
            if k == 0 {
                step += 1;
                k = step;
                m += 1;
            }
        }
        let mut ns2bsindx = [6u8; 256];
        ns2bsindx[0] = 0;
        ns2bsindx[1] = 2;
        ns2bsindx[2..11].fill(4);
        let mut hb2flag = [0u8; 256];
        hb2flag[0x40..].fill(8);
        Model {
            a: Alloc::new(size),
            coder: Coder::default(),
            min_ctx: 0,
            max_ctx: 0,
            found: 0,
            num_masked: 0,
            init_esc: 0,
            order_fall: 0,
            max_order: 0,
            run_length: 0,
            init_rl: 0,
            esc_count: 0,
            prev_success: 0,
            hi_bits_flag: 0,
            char_mask: [0; 256],
            ns2indx,
            ns2bsindx,
            hb2flag,
            bin_summ: vec![[0; 64]; 128],
            see: vec![[See::default(); 16]; 25],
        }
    }

    /// Its memory's size.
    pub(super) fn size(&self) -> u32 {
        self.a.size()
    }

    /// Starts the model afresh with contexts of up to `order` bytes.
    pub(super) fn start(&mut self, order: u32) {
        self.esc_count = 1;
        self.max_order = order.min(MAX_ORDER) as i32;
        self.restart();
    }

    /// Starts the range coder on the next four bytes.
    pub(super) fn init_coder(&mut self, bits: &mut Bits) {
        self.coder.init(bits);
    }

    /// Whether there is a model to decode with.
    pub(super) fn ready(&self) -> bool {
        self.min_ctx != 0
    }

    // The model's memory: a state is its byte (0), count (1) and successor (2); a context
    // its number of states (0), then either its states' total count (2) and their place
    // (4), or its one state in their stead (2), then its suffix (8).

    fn sym(&self, s: u32) -> u8 {
        self.a.u8(s)
    }

    fn freq(&self, s: u32) -> u32 {
        u32::from(self.a.u8(s.wrapping_add(1)))
    }

    fn set_freq(&mut self, s: u32, f: u32) {
        self.a.set_u8(s.wrapping_add(1), f as u8);
    }

    fn succ(&self, s: u32) -> u32 {
        self.a.u32(s.wrapping_add(2))
    }

    fn set_succ(&mut self, s: u32, v: u32) {
        self.a.set_u32(s.wrapping_add(2), v);
    }

    fn state(&self, s: u32) -> St {
        St {
            sym: self.sym(s),
            freq: self.freq(s) as u8,
            succ: self.succ(s),
        }
    }

    fn put_state(&mut self, s: u32, st: St) {
        self.a.set_u8(s, st.sym);
        self.set_freq(s, u32::from(st.freq));
        self.set_succ(s, st.succ);
    }

    fn swap(&mut self, x: u32, y: u32) {
        let (sx, sy) = (self.state(x), self.state(y));
        self.put_state(x, sy);
        self.put_state(y, sx);
    }

    fn ns(&self, c: u32) -> u32 {
        u32::from(self.a.u16(c))
    }

    fn set_ns(&mut self, c: u32, n: u32) {
        self.a.set_u16(c, n as u16);
    }

    fn summ(&self, c: u32) -> u32 {
        u32::from(self.a.u16(c.wrapping_add(2)))
    }

    fn set_summ(&mut self, c: u32, v: u32) {
        self.a.set_u16(c.wrapping_add(2), v as u16);
    }

    fn stats(&self, c: u32) -> u32 {
        self.a.u32(c.wrapping_add(4))
    }

    fn set_stats(&mut self, c: u32, v: u32) {
        self.a.set_u32(c.wrapping_add(4), v);
    }

    fn suffix(&self, c: u32) -> u32 {
        self.a.u32(c.wrapping_add(8))
    }

    fn set_suffix(&mut self, c: u32, v: u32) {
        self.a.set_u32(c.wrapping_add(8), v);
    }

    /// The state for byte `sym` among context `c`'s; 0 if it has none.
    fn find(&self, c: u32, sym: u8) -> u32 {
        let stats = self.stats(c);
        (0..self.ns(c).min(256))
            .map(|i| stats.wrapping_add(STATE * i))
            .find(|&s| self.sym(s) == sym)
            .unwrap_or(0)
    }

    /// A place the model may read a context or states at: above the text, within memory.
    fn valid(&self, p: u32) -> bool {
        p > self.a.text && p <= self.a.end
    }

    fn clear_mask(&mut self) {
        self.esc_count = 1;
        self.char_mask = [0; 256];
    }

    /// The model as it starts, and as it starts again when its memory is full.
    fn restart(&mut self) {
        self.char_mask = [0; 256];
        self.a.init();
        self.init_rl = -self.max_order.min(12) - 1;
        let c = self.a.alloc_context();
        self.min_ctx = c;
        self.max_ctx = c;
        self.set_suffix(c, 0);
        self.order_fall = self.max_order;
        self.set_ns(c, 256);
        self.set_summ(c, 257);
        let stats = self.a.alloc_units(256 / 2);
        self.found = stats;
        self.set_stats(c, stats);
        self.run_length = self.init_rl;
        self.prev_success = 0;
        for i in 0..256u32 {
            let st = St {
                sym: i as u8,
                freq: 1,
                succ: 0,
            };
            self.put_state(stats + STATE * i, st);
        }
        for (i, row) in self.bin_summ.iter_mut().enumerate() {
            for (k, esc) in INIT_BIN_ESC.iter().enumerate() {
                for m in (0..64).step_by(8) {
                    row[k + m] = (BIN_SCALE - esc / (i as u32 + 2)) as u16;
                }
            }
        }
        for (i, row) in self.see.iter_mut().enumerate() {
            row.fill(See::new(5 * i as u32 + 10));
        }
    }

    /// The next byte; `None` where the data is damaged.
    pub(super) fn decode_char(&mut self, bits: &mut Bits) -> Option<u8> {
        if !self.valid(self.min_ctx) {
            return None;
        }
        if self.ns(self.min_ctx) != 1 {
            if !self.valid(self.stats(self.min_ctx)) || !self.decode_symbol1() {
                return None;
            }
        } else {
            self.decode_bin_symbol()?;
        }
        self.coder.decode();
        while self.found == 0 {
            self.coder.normalize(bits);
            loop {
                self.order_fall += 1;
                self.min_ctx = self.suffix(self.min_ctx);
                if !self.valid(self.min_ctx) {
                    return None;
                }
                if self.ns(self.min_ctx) != self.num_masked {
                    break;
                }
            }
            if !self.decode_symbol2() {
                return None;
            }
            self.coder.decode();
        }
        let sym = self.sym(self.found);
        let succ = self.succ(self.found);
        if self.order_fall == 0 && succ > self.a.text {
            self.min_ctx = succ;
            self.max_ctx = succ;
        } else {
            self.update_model();
            if self.esc_count == 0 {
                self.clear_mask();
            }
        }
        self.coder.normalize(bits);
        Some(sym)
    }

    /// The binary context for the context of one state the model is in, by its count, the
    /// suffix's size, the bytes' high bits and the run so far.
    fn bin_context(&mut self) -> (usize, usize) {
        let c = self.min_ctx;
        let rs = c.wrapping_add(2);
        self.hi_bits_flag = u32::from(self.hb2flag[usize::from(self.sym(self.found))]);
        let suffix_ns = self.ns(self.suffix(c));
        let row = (self.freq(rs).wrapping_sub(1) as usize).min(127);
        let col = self.prev_success
            + u32::from(self.ns2bsindx[(suffix_ns.wrapping_sub(1) & 0xff) as usize])
            + self.hi_bits_flag
            + 2 * u32::from(self.hb2flag[usize::from(self.sym(rs))])
            + ((self.run_length >> 26) & 0x20) as u32;
        (row, (col as usize).min(63))
    }

    /// A byte under a context of one state, by its binary context.
    fn decode_bin_symbol(&mut self) -> Option<()> {
        let (row, col) = self.bin_context();
        let bs = u32::from(self.bin_summ[row][col]);
        let count = self.coder.shift_count(TOT_BITS)?;
        if count < bs {
            (self.coder.low_count, self.coder.high_count) = (0, bs);
            self.bin_hit(row, col);
        } else {
            (self.coder.low_count, self.coder.high_count) = (bs, BIN_SCALE);
            self.bin_miss(row, col);
        }
        Some(())
    }

    /// The one state's byte it was.
    fn bin_hit(&mut self, row: usize, col: usize) {
        let rs = self.min_ctx.wrapping_add(2);
        let bs = u32::from(self.bin_summ[row][col]);
        self.found = rs;
        let f = self.freq(rs);
        self.set_freq(rs, f + u32::from(f < 128));
        self.bin_summ[row][col] = (bs + INTERVAL - ((bs + 32) >> PERIOD_BITS)) as u16;
        self.prev_success = 1;
        self.run_length += 1;
    }

    /// An escape from a context of one state.
    fn bin_miss(&mut self, row: usize, col: usize) {
        let rs = self.min_ctx.wrapping_add(2);
        let bs = u32::from(self.bin_summ[row][col]);
        let bs = bs.wrapping_sub((bs + 32) >> PERIOD_BITS) as u16;
        self.bin_summ[row][col] = bs;
        self.init_esc = u32::from(EXP_ESCAPE[usize::from(bs >> 10).min(15)]);
        self.num_masked = 1;
        self.char_mask[usize::from(self.sym(rs))] = self.esc_count;
        self.prev_success = 0;
        self.found = 0;
    }

    /// A byte under a context of several states, none masked yet.
    fn decode_symbol1(&mut self) -> bool {
        let c = self.min_ctx;
        let ns = self.ns(c);
        if !(2..=256).contains(&ns) {
            return false;
        }
        self.coder.scale = self.summ(c);
        let mut p = self.stats(c);
        let Some(count) = self.coder.count() else {
            return false;
        };
        if count >= self.coder.scale {
            return false;
        }
        let mut hi = self.freq(p);
        if count < hi {
            (self.coder.low_count, self.coder.high_count) = (0, hi);
            self.first_hit(c, p);
            return true;
        }
        if self.found == 0 {
            return false;
        }
        self.prev_success = 0;
        for _ in 1..ns {
            p = p.wrapping_add(STATE);
            hi += self.freq(p);
            if hi > count {
                (self.coder.low_count, self.coder.high_count) = (hi - self.freq(p), hi);
                self.update1(c, p);
                return true;
            }
        }
        (self.coder.low_count, self.coder.high_count) = (hi, self.coder.scale);
        self.escape1(c);
        true
    }

    /// The first state of context `c` was the byte.
    fn first_hit(&mut self, c: u32, p: u32) {
        let hi = self.freq(p);
        self.prev_success = u32::from(2 * hi > self.summ(c));
        self.run_length += self.prev_success as i32;
        self.found = p;
        self.set_freq(p, hi + 4);
        self.set_summ(c, self.summ(c) + 4);
        if hi + 4 > MAX_FREQ {
            self.rescale(c);
        }
    }

    /// Another state of context `c` was the byte.
    fn update1(&mut self, c: u32, p: u32) {
        self.found = p;
        self.set_freq(p, self.freq(p) + 4);
        self.set_summ(c, self.summ(c) + 4);
        let before = p.wrapping_sub(STATE);
        if self.freq(p) > self.freq(before) {
            self.swap(p, before);
            self.found = before;
            if self.freq(before) > MAX_FREQ {
                self.rescale(c);
            }
        }
    }

    /// An escape from context `c`: every byte of it is masked.
    fn escape1(&mut self, c: u32) {
        let ns = self.ns(c);
        self.hi_bits_flag = u32::from(self.hb2flag[usize::from(self.sym(self.found))]);
        self.num_masked = ns;
        self.found = 0;
        let stats = self.stats(c);
        for k in 0..ns {
            let s = stats.wrapping_add(STATE * k);
            self.char_mask[usize::from(self.sym(s))] = self.esc_count;
        }
        self.prev_success = 0;
    }

    /// The SEE context for an escape from context `c` with `diff` bytes not masked, its
    /// mean set as the coder's scale; `None` for the context of all 256 bytes.
    fn esc_freq(&mut self, c: u32, diff: u32) -> Option<(usize, usize)> {
        let ns = self.ns(c);
        if ns == 256 {
            self.coder.scale = 1;
            return None;
        }
        let suffix_ns = self.ns(self.suffix(c)) as i32;
        let row = usize::from(self.ns2indx[(diff.wrapping_sub(1) & 0xff) as usize]).min(24);
        let col = usize::from((diff as i32) < suffix_ns - ns as i32)
            + 2 * usize::from(self.summ(c) < 11 * ns)
            + 4 * usize::from(self.num_masked > diff)
            + self.hi_bits_flag as usize;
        let col = col.min(15);
        self.coder.scale = self.see[row][col].mean();
        Some((row, col))
    }

    /// The states of context `c` not masked, with their total count; `None` where the
    /// context does not hold together.
    fn unmasked(&self, c: u32) -> Option<(Vec<u32>, u32)> {
        let ns = self.ns(c);
        let diff = ns.wrapping_sub(self.num_masked);
        if diff == 0 || diff > 256 || ns > 256 {
            return None;
        }
        let stats = self.stats(c);
        let ps: Vec<u32> = (0..ns)
            .map(|k| stats.wrapping_add(STATE * k))
            .filter(|&s| self.char_mask[usize::from(self.sym(s))] != self.esc_count)
            .take(diff as usize)
            .collect();
        let hi = ps.iter().map(|&s| self.freq(s)).sum();
        (ps.len() == diff as usize).then_some((ps, hi))
    }

    /// A byte under a context some of whose bytes are masked.
    fn decode_symbol2(&mut self) -> bool {
        let c = self.min_ctx;
        let diff = self.ns(c).wrapping_sub(self.num_masked);
        let Some((ps, hi)) = self.unmasked(c) else {
            return false;
        };
        let see = self.esc_freq(c, diff);
        self.coder.scale += hi;
        let Some(count) = self.coder.count() else {
            return false;
        };
        if count >= self.coder.scale {
            return false;
        }
        if count >= hi {
            (self.coder.low_count, self.coder.high_count) = (hi, self.coder.scale);
            self.escape2(c, &ps, see);
            return true;
        }
        let mut acc = 0u32;
        for &s in &ps {
            acc += self.freq(s);
            if acc > count {
                (self.coder.low_count, self.coder.high_count) = (acc - self.freq(s), acc);
                self.found2(c, s, see);
                return true;
            }
        }
        false
    }

    /// State `s` of context `c`, under SEE context `see`, was the byte.
    fn found2(&mut self, c: u32, s: u32, see: Option<(usize, usize)>) {
        if let Some((row, col)) = see {
            self.see[row][col].update();
        }
        self.update2(c, s);
    }

    /// An escape from context `c`, whose unmasked states `ps` are masked now.
    fn escape2(&mut self, c: u32, ps: &[u32], see: Option<(usize, usize)>) {
        for &s in ps {
            self.char_mask[usize::from(self.sym(s))] = self.esc_count;
        }
        if let Some((row, col)) = see {
            let e = &mut self.see[row][col];
            e.summ = e.summ.wrapping_add(self.coder.scale as u16);
        }
        self.num_masked = self.ns(c);
    }

    fn update2(&mut self, c: u32, p: u32) {
        self.found = p;
        let f = self.freq(p) + 4;
        self.set_freq(p, f);
        self.set_summ(c, self.summ(c) + 4);
        if f > MAX_FREQ {
            self.rescale(c);
        }
        self.esc_count = self.esc_count.wrapping_add(1);
        self.run_length = self.init_rl;
    }

    /// Halves context `c`'s counts, the found state first, the others kept in order of
    /// count; states whose count falls to zero are dropped.
    fn rescale(&mut self, c: u32) {
        let old_ns = self.ns(c);
        let stats = self.stats(c);
        if !(2..=256).contains(&old_ns) {
            return;
        }
        let mut p = self.found;
        while p > stats && p.wrapping_sub(stats).is_multiple_of(STATE) {
            self.swap(p, p - STATE);
            p -= STATE;
        }
        let p = stats;
        self.set_freq(p, self.freq(p) + 4);
        self.set_summ(c, self.summ(c) + 4);
        let mut esc = self.summ(c) as i32 - self.freq(p) as i32;
        let adder = u32::from(self.order_fall != 0);
        let f = (self.freq(p) + adder) >> 1;
        self.set_freq(p, f);
        let mut sum = f;
        let mut q = p;
        for _ in 1..old_ns {
            q += STATE;
            esc -= self.freq(q) as i32;
            let f = (self.freq(q) + adder) >> 1;
            self.set_freq(q, f);
            sum += f;
            if f > self.freq(q - STATE) {
                // Moved up past the states of lower counts.
                let tmp = self.state(q);
                let mut r = q;
                loop {
                    let prev = self.state(r - STATE);
                    self.put_state(r, prev);
                    r -= STATE;
                    if r == stats || u32::from(tmp.freq) <= self.freq(r - STATE) {
                        break;
                    }
                }
                self.put_state(r, tmp);
            }
        }
        self.set_summ(c, sum);
        let mut ns = old_ns;
        if self.freq(q) == 0 {
            let mut zeros = 0u32;
            while q > stats && self.freq(q) == 0 {
                zeros += 1;
                q -= STATE;
            }
            esc += zeros as i32;
            ns -= zeros;
            self.set_ns(c, ns);
            if ns == 1 {
                let mut tmp = self.state(stats);
                loop {
                    tmp.freq -= tmp.freq >> 1;
                    esc >>= 1;
                    if esc <= 1 {
                        break;
                    }
                }
                self.a.free_units(stats, (old_ns + 1) >> 1);
                self.found = c.wrapping_add(2);
                self.put_state(self.found, tmp);
                return;
            }
        }
        esc -= esc >> 1;
        self.set_summ(c, self.summ(c).wrapping_add(esc as u32));
        let (n0, n1) = ((old_ns + 1) >> 1, (ns + 1) >> 1);
        if n0 != n1 {
            let moved = self.a.shrink(stats, n0, n1);
            self.set_stats(c, moved);
        }
        self.found = self.stats(c);
    }

    /// The model started again when its memory is full.
    fn restart_full(&mut self) {
        self.restart();
        self.esc_count = 0;
    }
}

mod update;

#[cfg(test)]
mod tests;
