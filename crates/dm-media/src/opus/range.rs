// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Opus's range decoder (RFC 6716, section 4.1): every symbol of a frame is read through it,
//! from the front of the frame, and raw bits from its back.
//!
//! Its state is two 32-bit numbers, the range `rng` and the value `val` within it, renewed a
//! byte at a time once the range falls to 2^23 or less. A read past either end of the frame
//! reads zeros, as the standard says, so no input makes it fail; what was read is counted in
//! [`RangeDecoder::tell`], which tells a frame that has run out of bits.

/// The range decoder over one frame's bytes.
pub(crate) struct RangeDecoder<'a> {
    buf: &'a [u8],
    /// The frame's length in bytes (smaller than `buf` where the frame's end holds another
    /// part, as Opus's redundant frames do).
    storage: usize,
    /// Bytes read from the front.
    offs: usize,
    /// Bytes read from the back, for raw bits.
    end_offs: usize,
    /// Raw bits read from the back but not used yet, lowest first.
    end_window: u32,
    nend_bits: u32,
    /// Bits read so far, as the standard counts them for `tell`.
    nbits_total: i32,
    rng: u32,
    val: u32,
    /// The byte read last (its low bit is the next value's top bit).
    rem: u32,
    /// The scale of the symbol being read, between `decode` and `update`.
    ext: u32,
}

/// Bits a renewal shifts in at a time.
const SYM_BITS: u32 = 8;
/// The range is renewed once it falls to this or less.
const CODE_BOT: u32 = 1 << 23;
/// The value's top bit, outside its 31 bits.
const CODE_TOP: u32 = 1 << 31;
/// The bits of the first byte that are not in the first value (RFC 6716, 4.1.1).
const CODE_EXTRA: u32 = 7;

impl<'a> RangeDecoder<'a> {
    /// A decoder at the start of `buf` (RFC 6716, 4.1.1).
    pub(crate) fn new(buf: &'a [u8]) -> RangeDecoder<'a> {
        let mut dec = RangeDecoder {
            buf,
            storage: buf.len(),
            offs: 0,
            end_offs: 0,
            end_window: 0,
            nend_bits: 0,
            nbits_total: 9,
            rng: 1 << CODE_EXTRA,
            val: 0,
            rem: 0,
            ext: 0,
        };
        dec.rem = dec.read_byte();
        dec.val = dec.rng - 1 - (dec.rem >> (SYM_BITS - CODE_EXTRA));
        dec.normalize();
        dec
    }

    fn read_byte(&mut self) -> u32 {
        if self.offs < self.storage {
            let byte = self.buf.get(self.offs).copied().unwrap_or(0);
            self.offs += 1;
            u32::from(byte)
        } else {
            0
        }
    }

    fn read_byte_from_end(&mut self) -> u32 {
        if self.end_offs < self.storage {
            self.end_offs += 1;
            let byte = self.buf.get(self.storage - self.end_offs).copied();
            u32::from(byte.unwrap_or(0))
        } else {
            0
        }
    }

    /// Renews the range a byte at a time while it is 2^23 or less (RFC 6716, 4.1.2.1).
    fn normalize(&mut self) {
        while self.rng <= CODE_BOT {
            self.nbits_total += SYM_BITS as i32;
            self.rng <<= SYM_BITS;
            let last = self.rem;
            self.rem = self.read_byte();
            let sym = ((last << SYM_BITS) | self.rem) >> (SYM_BITS - CODE_EXTRA);
            self.val = ((self.val << SYM_BITS) + (0xFF & !sym)) & (CODE_TOP - 1);
        }
    }

    /// The frequency the next symbol falls at, out of `ft` (`ec_decode`); [`Self::update`]
    /// then takes the symbol out.
    pub(crate) fn decode(&mut self, ft: u32) -> u32 {
        let ft = ft.max(1);
        self.ext = (self.rng / ft).max(1);
        let s = self.val / self.ext;
        ft - (s + 1).min(ft)
    }

    /// [`Self::decode`] for a total of `2^bits`.
    pub(crate) fn decode_bin(&mut self, bits: u32) -> u32 {
        self.ext = (self.rng >> bits).max(1);
        let s = self.val / self.ext;
        (1 << bits) - (s + 1).min(1 << bits)
    }

    /// Takes out the symbol that covers `fl..fh` of `ft` (`ec_dec_update`).
    pub(crate) fn update(&mut self, fl: u32, fh: u32, ft: u32) {
        let s = self.ext.wrapping_mul(ft.wrapping_sub(fh));
        self.val = self.val.wrapping_sub(s);
        self.rng = if fl > 0 {
            self.ext.wrapping_mul(fh.wrapping_sub(fl))
        } else {
            self.rng.wrapping_sub(s)
        };
        self.normalize();
    }

    /// One bit whose chance of being 1 is `1 / 2^logp` (RFC 6716, 4.1.3.2).
    pub(crate) fn bit_logp(&mut self, logp: u32) -> bool {
        let r = self.rng;
        let d = self.val;
        let s = r >> logp;
        let one = d < s;
        if one {
            self.rng = s;
        } else {
            self.val = d - s;
            self.rng = r - s;
        }
        self.normalize();
        one
    }

    /// A symbol from an "inverse" cumulative table of `2^ftb` (RFC 6716, 4.1.3.3); the table
    /// ends in 0, which ends the search.
    pub(crate) fn icdf(&mut self, icdf: &[u8], ftb: u32) -> usize {
        let mut s = self.rng;
        let d = self.val;
        let r = s >> ftb;
        let mut t;
        let mut ret = 0;
        loop {
            t = s;
            s = r.wrapping_mul(u32::from(icdf.get(ret).copied().unwrap_or(0)));
            if d >= s {
                break;
            }
            ret += 1;
        }
        self.val = d.wrapping_sub(s);
        self.rng = t.wrapping_sub(s);
        self.normalize();
        ret
    }

    /// A whole number below `ft`, evenly likely (RFC 6716, 4.1.5); `ft` is at least 2. A
    /// value past the end, which only a damaged frame gives, reads as the largest.
    pub(crate) fn uint(&mut self, ft: u32) -> u32 {
        if ft < 2 {
            return 0;
        }
        let top = ft - 1;
        let ftb = 32 - top.leading_zeros();
        if ftb > 8 {
            let shift = ftb - 8;
            let ft1 = (top >> shift) + 1;
            let s = self.decode(ft1);
            self.update(s, s + 1, ft1);
            let t = (s << shift) | self.bits(shift);
            t.min(top)
        } else {
            let s = self.decode(ft);
            self.update(s, s + 1, ft);
            s
        }
    }

    /// `n` raw bits (at most 24) from the frame's back (RFC 6716, 4.1.4).
    pub(crate) fn bits(&mut self, n: u32) -> u32 {
        let n = n.min(24);
        let mut window = self.end_window;
        let mut available = self.nend_bits;
        if available < n {
            loop {
                window |= self.read_byte_from_end() << available;
                available += SYM_BITS;
                if available > 32 - SYM_BITS {
                    break;
                }
            }
        }
        let ret = window & ((1u32 << n) - 1);
        self.end_window = window >> n;
        self.nend_bits = available - n;
        self.nbits_total += n as i32;
        ret
    }

    /// Bits read so far, whole bits rounded up (RFC 6716, 4.1.6.1).
    pub(crate) fn tell(&self) -> i32 {
        self.nbits_total - (32 - self.rng.leading_zeros()) as i32
    }

    /// Bits read so far, in eighths of a bit, rounded up (RFC 6716, 4.1.6.2).
    pub(crate) fn tell_frac(&self) -> i32 {
        let mut lg = 32 - self.rng.leading_zeros();
        let mut r = self.rng >> (lg - 16);
        for _ in 0..3 {
            r = (r * r) >> 15;
            let b = r >> 16;
            lg = (lg << 1) | b;
            r >>= b;
        }
        (self.nbits_total << 3) - lg as i32
    }

    /// The frame's length in bits, as the decoder takes it.
    pub(crate) fn total_bits(&self) -> i32 {
        (self.storage as i32) * 8
    }

    /// Counts the frame as read up to `bits`: a silent frame takes the rest of it as read.
    pub(crate) fn skip_to(&mut self, bits: i32) {
        self.nbits_total += bits - self.tell();
    }

    /// The final state of the range, which an encoder reports for each frame: the same value
    /// means every symbol was read as it was written.
    pub(crate) fn range(&self) -> u32 {
        self.rng
    }
}

/// A Laplace-distributed whole number (RFC 6716, 4.3.2.1): `fs` is the chance of 0 out of
/// 2^15, `decay` how fast the rest fall off; what CELT's coarse band energies are coded with.
pub(crate) fn laplace(dec: &mut RangeDecoder<'_>, fs: u32, decay: u32) -> i32 {
    let mut val = 0i32;
    let fm = dec.decode_bin(15);
    let mut fl = 0u32;
    let mut fs = fs;
    if fm >= fs {
        val += 1;
        fl = fs;
        fs = (((32768 - 32 - fs) * (16384 - decay)) >> 15) + 1;
        while fs > 1 && fm >= fl + 2 * fs {
            fs *= 2;
            fl += fs;
            fs = (((fs - 2) * decay) >> 15) + 1;
            val += 1;
        }
        if fs <= 1 {
            let di = (fm - fl) >> 1;
            val += di as i32;
            fl += 2 * di;
        }
        if fm < fl + fs {
            val = -val;
        } else {
            fl += fs;
        }
    }
    dec.update(fl, (fl + fs).min(32768), 32768);
    val
}
