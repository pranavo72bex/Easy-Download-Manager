// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How a CELT frame's bits are shared among its bands (RFC 6716, 4.3.3): the encoder sends
//! only a few numbers (boosts, a trim, skipped bands, the stereo choice), and the decoder works
//! out the rest exactly as the encoder did, to the eighth of a bit. Also the table of what a
//! band's PVQ codebook costs at each number of pulses (4.3.4.1), worked out once.

use std::sync::OnceLock;

use super::pvq::pvq_rows;
use super::tables::{ALLOCATION, BANDS, CAPS, EBANDS, LOG2_FRAC, LOG_N};
use super::Span;
use crate::opus::range::RangeDecoder;

/// Eighths of a bit: allocation is counted in them.
pub(super) const BITRES: i32 = 3;
/// The most fine-energy bits a band gets.
pub(super) const MAX_FINE_BITS: i32 = 8;
/// The fine energy's offset against its fair share (eighths of a bit per line).
const FINE_OFFSET: i32 = 21;
/// The most pulse counts a codebook size is tabled for.
const MAX_PSEUDO: usize = 40;

/// The number of pulses the `i`th entry of a codebook's cost table stands for: 1 to 8 one by
/// one, then on a coarser scale up to 128.
pub(super) fn pulses_of(i: i32) -> i32 {
    if i < 8 {
        i
    } else {
        (8 + (i & 7)) << ((i >> 3) - 1)
    }
}

/// `log2(val)` with `frac` fractional bits, rounded up (the codebook cost's measure).
fn log2_frac(val: u32, frac: i32) -> i32 {
    let bits = 32 - val.leading_zeros() as i32;
    if val & val.wrapping_sub(1) == 0 {
        return (bits - 1) << frac;
    }
    let mut v = u64::from(val);
    if bits > 16 {
        v = ((v - 1) >> (bits - 16)) + 1;
    } else {
        v <<= 16 - bits;
    }
    let mut l = (bits - 1) << frac;
    let mut f = frac;
    loop {
        let b = (v >> 16) as i32;
        l += b << f;
        v = (v + b as u64) >> b;
        v = (v * v + 0x7FFF) >> 15;
        if f <= 0 {
            break;
        }
        f -= 1;
    }
    l + i32::from(v > 0x8000)
}

/// Each band's codebook costs, by frame size: for each `lm` from -1 to 3 and band, where its
/// row starts in `bits`; a row is its largest pulse index, then the cost of each, in eighths
/// of a bit, less one.
struct PulseCache {
    index: [i32; BANDS * 5],
    bits: Vec<u8>,
}

fn build_cache() -> PulseCache {
    let mut index = [-1i32; BANDS * 5];
    let mut bits: Vec<u8> = Vec::new();
    let mut seen: Vec<(usize, i32)> = Vec::new();
    for i in 0..5 {
        for j in 0..BANDS {
            let n = ((EBANDS[j + 1] - EBANDS[j]) << i) >> 1;
            if n == 0 {
                continue;
            }
            if let Some(&(_, at)) = seen.iter().find(|(size, _)| *size == n) {
                index[i * BANDS + j] = at;
                continue;
            }
            let at = bits.len() as i32;
            let kmax = pulses_of(MAX_PSEUDO as i32 + 1) as usize;
            let rows = pvq_rows(n, kmax);
            let counts = rows.get(n * (kmax + 1)..).unwrap_or(&[]);
            let count = |k: i32| counts.get(k as usize).copied().unwrap_or(u64::MAX);
            let mut k = 0;
            while k < MAX_PSEUDO as i32 && count(pulses_of(k + 1)) <= u64::from(u32::MAX) {
                k += 1;
            }
            bits.push(k as u8);
            for p in 1..=k {
                let v = count(pulses_of(p)).min(u64::from(u32::MAX)) as u32;
                bits.push((log2_frac(v, BITRES) - 1).clamp(0, 255) as u8);
            }
            seen.push((n, at));
            index[i * BANDS + j] = at;
        }
    }
    PulseCache { index, bits }
}

fn cache() -> &'static PulseCache {
    static CACHE: OnceLock<PulseCache> = OnceLock::new();
    CACHE.get_or_init(build_cache)
}

/// The cost table's size, all rows together.
#[cfg(test)]
pub(super) fn cache_size() -> usize {
    cache().bits.len()
}

/// Band `band`'s cost row at frame size `lm` (-1 to 3): its largest pulse index first.
pub(super) fn cost_row(lm: i32, band: usize) -> &'static [u8] {
    let cache = cache();
    let at = cache
        .index
        .get(((lm + 1).max(0) as usize) * BANDS + band)
        .copied()
        .unwrap_or(-1);
    if at < 0 {
        return &[0];
    }
    let at = at as usize;
    let len = usize::from(cache.bits.get(at).copied().unwrap_or(0)) + 1;
    cache.bits.get(at..at + len).unwrap_or(&[0])
}

/// The pulse index whose cost is nearest `bits` (eighths) in band `band` at `lm`.
pub(super) fn bits_to_pulses(band: usize, lm: i32, bits: i32) -> i32 {
    let row = cost_row(lm, band);
    let cost = |i: usize| i32::from(row.get(i).copied().unwrap_or(0));
    let mut lo = 0usize;
    let mut hi = usize::from(row.first().copied().unwrap_or(0));
    let bits = bits - 1;
    for _ in 0..6 {
        let mid = (lo + hi + 1) >> 1;
        if cost(mid) >= bits {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let below = if lo == 0 { -1 } else { cost(lo) };
    if bits - below <= cost(hi) - bits {
        lo as i32
    } else {
        hi as i32
    }
}

/// What pulse index `pulses` costs in band `band` at `lm`, in eighths of a bit.
pub(super) fn pulses_to_bits(band: usize, lm: i32, pulses: i32) -> i32 {
    if pulses == 0 {
        return 0;
    }
    let row = cost_row(lm, band);
    i32::from(row.get(pulses as usize).copied().unwrap_or(0)) + 1
}

/// The most bits (eighths) each band can use in a frame of this shape.
pub(super) fn caps(span: &Span) -> [i32; BANDS] {
    let mut cap = [0i32; BANDS];
    let row = BANDS * (2 * span.lm + span.channels - 1);
    for (i, c) in cap.iter_mut().enumerate() {
        let n = ((EBANDS[i + 1] - EBANDS[i]) << span.lm) as i32;
        let base = i32::from(CAPS.get(row + i).copied().unwrap_or(0));
        *c = ((base + 64) * span.channels as i32 * n) >> 2;
    }
    cap
}

/// A frame's bits as the bands share them.
pub(super) struct Allocation {
    /// The last band coded, plus one; the rest are skipped.
    pub(super) coded_bands: usize,
    /// Each band's bits for its shape (PVQ), in eighths.
    pub(super) pulses: [i32; BANDS],
    /// Each band's fine-energy bits per channel.
    pub(super) fine: [i32; BANDS],
    /// Whether a band takes a leftover fine bit first (0) or second (1).
    pub(super) fine_priority: [i32; BANDS],
    /// Bits over the caps, handed on to the bands as they are decoded.
    pub(super) balance: i32,
    /// The first band coded as intensity stereo.
    pub(super) intensity: usize,
    /// Whether the channels are coded apart (dual stereo).
    pub(super) dual_stereo: bool,
}

/// The two allocation curves the search interpolates between, and what bounds them.
struct Curves {
    bits1: [i32; BANDS],
    bits2: [i32; BANDS],
    thresh: [i32; BANDS],
    skip_start: usize,
}

/// Shares `total` eighths of a bit among the bands (RFC 6716, 4.3.3), reading the skipped
/// bands, the intensity band and dual stereo where the frame codes them.
pub(super) fn allocate(
    dec: &mut RangeDecoder<'_>,
    span: &Span,
    offsets: &[i32; BANDS],
    cap: &[i32; BANDS],
    trim: i32,
    total: i32,
) -> Allocation {
    let (start, end, lm) = (span.start, span.end, span.lm as i32);
    let c = span.channels as i32;
    let mut total = total.max(0);
    let skip_rsv = if total >= 1 << BITRES { 1 << BITRES } else { 0 };
    total -= skip_rsv;
    let mut intensity_rsv = 0;
    let mut dual_rsv = 0;
    if c == 2 {
        intensity_rsv = LOG2_FRAC.get(end - start).copied().unwrap_or(0);
        if intensity_rsv > total {
            intensity_rsv = 0;
        } else {
            total -= intensity_rsv;
            dual_rsv = if total >= 1 << BITRES { 1 << BITRES } else { 0 };
            total -= dual_rsv;
        }
    }
    let mut curves = Curves {
        bits1: [0; BANDS],
        bits2: [0; BANDS],
        thresh: [0; BANDS],
        skip_start: start,
    };
    let mut trim_offset = [0i32; BANDS];
    for j in start..end {
        let width = (EBANDS[j + 1] - EBANDS[j]) as i32;
        curves.thresh[j] = (c << BITRES).max(((3 * width) << lm << BITRES) >> 4);
        trim_offset[j] =
            (c * width * (trim - 5 - lm) * (end - j - 1) as i32 * (1 << (lm + BITRES))) >> 6;
        if width << lm == 1 {
            trim_offset[j] -= c << BITRES;
        }
    }
    let quality = |q: usize, j: usize| {
        let width = (EBANDS[j + 1] - EBANDS[j]) as i32;
        let per = i32::from(ALLOCATION.get(q).map_or(0, |row| row[j]));
        let bits = (c * width * per) << lm >> 2;
        if bits > 0 {
            (bits + trim_offset[j]).max(0)
        } else {
            bits
        }
    };
    let mut lo = 1usize;
    let mut hi = ALLOCATION.len() - 1;
    loop {
        let mid = (lo + hi) >> 1;
        let mut done = false;
        let mut psum = 0;
        for j in (start..end).rev() {
            let bits = quality(mid, j) + offsets[j];
            if bits >= curves.thresh[j] || done {
                done = true;
                psum += bits.min(cap[j]);
            } else if bits >= c << BITRES {
                psum += c << BITRES;
            }
        }
        if psum > total {
            hi = mid.saturating_sub(1);
        } else {
            lo = mid + 1;
        }
        if lo > hi {
            break;
        }
    }
    let hi = lo;
    let lo = lo - 1;
    for j in start..end {
        let mut b1 = quality(lo, j);
        let mut b2 = if hi >= ALLOCATION.len() {
            cap[j]
        } else {
            quality(hi, j)
        };
        if lo > 0 {
            b1 += offsets[j];
        }
        b2 += offsets[j];
        if offsets[j] > 0 {
            curves.skip_start = j;
        }
        curves.bits1[j] = b1;
        curves.bits2[j] = (b2 - b1).max(0);
    }
    let reserved = Reserved {
        skip: skip_rsv,
        intensity: intensity_rsv,
        dual: dual_rsv,
    };
    interpolate(dec, span, &curves, cap, total, reserved)
}

/// Bits set aside for the skip flag and the stereo choices.
struct Reserved {
    skip: i32,
    intensity: i32,
    dual: i32,
}

/// The allocation between the two curves that fits `total`, the skipped bands read, and each
/// band's bits split into fine energy and shape (`interp_bits2pulses`).
fn interpolate(
    dec: &mut RangeDecoder<'_>,
    span: &Span,
    curves: &Curves,
    cap: &[i32; BANDS],
    total: i32,
    reserved: Reserved,
) -> Allocation {
    let (start, end) = (span.start, span.end);
    let c = span.channels as i32;
    let alloc_floor = c << BITRES;
    let mut total = total;
    let mut lo = 0;
    let mut hi = 1 << 6;
    for _ in 0..6 {
        let mid = (lo + hi) >> 1;
        let mut psum = 0;
        let mut done = false;
        for j in (start..end).rev() {
            let tmp = curves.bits1[j] + ((mid * curves.bits2[j]) >> 6);
            if tmp >= curves.thresh[j] || done {
                done = true;
                psum += tmp.min(cap[j]);
            } else if tmp >= alloc_floor {
                psum += alloc_floor;
            }
        }
        if psum > total {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let mut psum = 0;
    let mut done = false;
    let mut bits = [0i32; BANDS];
    for j in (start..end).rev() {
        let mut tmp = curves.bits1[j] + ((lo * curves.bits2[j]) >> 6);
        if tmp < curves.thresh[j] && !done {
            tmp = if tmp >= alloc_floor { alloc_floor } else { 0 };
        } else {
            done = true;
        }
        tmp = tmp.min(cap[j]);
        bits[j] = tmp;
        psum += tmp;
    }
    let mut intensity_rsv = reserved.intensity;
    let mut coded = end;
    loop {
        let j = coded - 1;
        if j <= curves.skip_start {
            total += reserved.skip;
            break;
        }
        let span_lines = (EBANDS[coded] - EBANDS[start]) as i32;
        let mut left = total - psum;
        let percoeff = left / span_lines.max(1);
        left -= span_lines * percoeff;
        let rem = (left - (EBANDS[j] - EBANDS[start]) as i32).max(0);
        let band_width = (EBANDS[coded] - EBANDS[j]) as i32;
        let mut band_bits = bits[j] + percoeff * band_width + rem;
        if band_bits >= curves.thresh[j].max(alloc_floor + (1 << BITRES)) {
            if dec.bit_logp(1) {
                break;
            }
            psum += 1 << BITRES;
            band_bits -= 1 << BITRES;
        }
        psum -= bits[j] + intensity_rsv;
        if intensity_rsv > 0 {
            intensity_rsv = LOG2_FRAC.get(j - start).copied().unwrap_or(0);
        }
        psum += intensity_rsv;
        if band_bits >= alloc_floor {
            psum += alloc_floor;
            bits[j] = alloc_floor;
        } else {
            bits[j] = 0;
        }
        coded -= 1;
    }
    let intensity = if intensity_rsv > 0 {
        start + dec.uint((coded + 1 - start) as u32) as usize
    } else {
        0
    };
    let mut dual_rsv = reserved.dual;
    if intensity <= start {
        total += dual_rsv;
        dual_rsv = 0;
    }
    let dual_stereo = dual_rsv > 0 && dec.bit_logp(1);
    let span_lines = (EBANDS[coded] - EBANDS[start]) as i32;
    let mut left = total - psum;
    let percoeff = left / span_lines.max(1);
    left -= span_lines * percoeff;
    for j in start..coded {
        bits[j] += percoeff * (EBANDS[j + 1] - EBANDS[j]) as i32;
    }
    for j in start..coded {
        let tmp = left.min((EBANDS[j + 1] - EBANDS[j]) as i32);
        bits[j] += tmp;
        left -= tmp;
    }
    let mut out = Allocation {
        coded_bands: coded,
        pulses: bits,
        fine: [0; BANDS],
        fine_priority: [0; BANDS],
        balance: 0,
        intensity,
        dual_stereo,
    };
    split_fine(span, cap, &mut out);
    out
}

/// Each coded band's bits split into fine energy and shape, and the skipped bands' bits
/// all given to fine energy.
fn split_fine(span: &Span, cap: &[i32; BANDS], out: &mut Allocation) {
    let (start, lm) = (span.start, span.lm as i32);
    let c = span.channels as i32;
    let stereo = i32::from(c > 1);
    let log_m = lm << BITRES;
    let mut balance = 0;
    for j in start..out.coded_bands {
        let n = ((EBANDS[j + 1] - EBANDS[j]) as i32) << lm;
        let bit = out.pulses[j] + balance;
        let mut excess = if n > 1 {
            (bit - cap[j]).max(0)
        } else {
            (bit - (c << BITRES)).max(0)
        };
        out.pulses[j] = bit - excess;
        if n > 1 {
            let extra_dof = c == 2 && n > 2 && !out.dual_stereo && j < out.intensity;
            let den = c * n + i32::from(extra_dof);
            let nclogn = den * (LOG_N[j] + log_m);
            let mut offset = (nclogn >> 1) - den * FINE_OFFSET;
            if n == 2 {
                offset += (den << BITRES) >> 2;
            }
            if out.pulses[j] + offset < (den * 2) << BITRES {
                offset += nclogn >> 2;
            } else if out.pulses[j] + offset < (den * 3) << BITRES {
                offset += nclogn >> 3;
            }
            let mut fine = (out.pulses[j] + offset + (den << (BITRES - 1))).max(0);
            fine = (fine / den.max(1)) >> BITRES;
            if c * fine > (out.pulses[j] >> BITRES) {
                fine = out.pulses[j] >> stereo >> BITRES;
            }
            fine = fine.min(MAX_FINE_BITS);
            out.fine[j] = fine;
            out.fine_priority[j] = i32::from(fine * (den << BITRES) >= out.pulses[j] + offset);
            out.pulses[j] -= (c * fine) << BITRES;
        } else {
            out.fine[j] = 0;
            out.fine_priority[j] = 1;
        }
        if excess > 0 {
            let extra_fine = (excess >> (stereo + BITRES)).min(MAX_FINE_BITS - out.fine[j]);
            out.fine[j] += extra_fine;
            let extra_bits = (extra_fine * c) << BITRES;
            out.fine_priority[j] = i32::from(extra_bits >= excess - balance);
            excess -= extra_bits;
        }
        balance = excess;
    }
    out.balance = balance;
    for j in out.coded_bands..span.end {
        out.fine[j] = out.pulses[j] >> stereo >> BITRES;
        out.pulses[j] = 0;
        out.fine_priority[j] = i32::from(out.fine[j] < 1);
    }
}
