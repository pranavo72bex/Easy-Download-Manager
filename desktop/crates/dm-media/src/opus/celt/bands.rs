// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A CELT frame's band shapes (RFC 6716, 4.3.4 and 4.3.4.4 to 4.3.4.6): each band's bits are
//! spent on a PVQ codeword, or the band is split in two halves (or two channels) by an angle
//! and each half takes its share; a band with no pulses is folded from the bands below it, or
//! filled with noise. The time-frequency changes and the stereo merge are undone here too.

use super::alloc::{bits_to_pulses, cost_row, pulses_of, pulses_to_bits, Allocation, BITRES};
use super::pvq::{
    bitexact_cos, bitexact_log2tan, deinterleave, frac_mul16, haar1, interleave, isqrt, lcg,
    renormalise, unquantize, SPREAD_AGGRESSIVE,
};
use super::tables::{BANDS, BIT_DEINTERLEAVE, BIT_INTERLEAVE, EBANDS, EXP2_TABLE8, LOG_N};
use super::Span;
use crate::opus::range::RangeDecoder;

/// The split angle's offset against its fair share of bits, eighths per line.
const QTHETA_OFFSET: i32 = 4;
/// [`QTHETA_OFFSET`] for a stereo pair of two lines.
const QTHETA_OFFSET_TWOPHASE: i32 = 16;

/// What a frame's bands are decoded with, besides the allocation's numbers.
pub(super) struct BandFrame<'a> {
    pub(super) span: &'a Span,
    pub(super) alloc: &'a Allocation,
    /// Each band's time-frequency change.
    pub(super) tf_res: &'a [i32; BANDS],
    pub(super) spread: u32,
    pub(super) transient: bool,
    /// The frame's bits, eighths, less the anti-collapse bit's reservation.
    pub(super) total_bits: i32,
}

/// One band (or part of one) to decode: its bits (eighths), its short blocks, the length it
/// is scaled to and which blocks may be folded into (`fill`).
#[derive(Clone, Copy)]
struct Part {
    bits: i32,
    blocks: usize,
    gain: f32,
    fill: u32,
}

/// A split's angle and what follows from it.
struct Theta {
    inv: bool,
    imid: i32,
    iside: i32,
    delta: i32,
    itheta: i32,
    qalloc: i32,
}

/// The state the bands share as they are decoded in order.
struct Ctx<'d, 'b> {
    dec: &'d mut RangeDecoder<'b>,
    band: usize,
    intensity: usize,
    spread: u32,
    tf_change: i32,
    remaining_bits: i32,
    seed: u32,
}

/// How finely a split's angle is coded (RFC 6716, 4.3.4.5).
fn compute_qn(n: i32, b: i32, offset: i32, pulse_cap: i32, stereo: bool) -> i32 {
    let mut n2 = 2 * n - 1;
    if stereo && n == 2 {
        n2 -= 1;
    }
    let n2 = n2.max(1);
    let qb = ((b + n2 * offset) / n2)
        .min(b - pulse_cap - (4 << BITRES))
        .min(8 << BITRES);
    if qb < (1 << BITRES) >> 1 {
        return 1;
    }
    let qn = EXP2_TABLE8[(qb & 7) as usize] >> (14 - (qb >> BITRES));
    ((qn + 1) >> 1) << 1
}

/// The two halves of a split mixed back from mid and side, normalized (`stereo_merge`).
fn stereo_merge(x: &mut [f32], y: &mut [f32], mid: f32) {
    let mut xp = 0.0f32;
    let mut side = 0.0f32;
    for (a, b) in x.iter().zip(y.iter()) {
        xp += b * a;
        side += b * b;
    }
    xp *= mid;
    let el = mid * mid + side - 2.0 * xp;
    let er = mid * mid + side + 2.0 * xp;
    if er < 6e-4 || el < 6e-4 {
        y.copy_from_slice(x);
        return;
    }
    let lgain = 1.0 / el.sqrt();
    let rgain = 1.0 / er.sqrt();
    for (a, b) in x.iter_mut().zip(y.iter_mut()) {
        let l = mid * *a;
        let r = *b;
        *a = lgain * (l - r);
        *b = rgain * (l + r);
    }
}

impl Ctx<'_, '_> {
    /// A split's angle, read (RFC 6716, 4.3.4.5): its bits come out of `part`, and the blocks
    /// that may be folded into follow from it.
    fn theta(&mut self, n: usize, part: &mut Part, blocks0: usize, lm: i32, stereo: bool) -> Theta {
        let i = self.band;
        let pulse_cap = LOG_N[i] + lm * (1 << BITRES);
        let twophase = stereo && n == 2;
        let offset = (pulse_cap >> 1)
            - if twophase {
                QTHETA_OFFSET_TWOPHASE
            } else {
                QTHETA_OFFSET
            };
        let mut qn = compute_qn(n as i32, part.bits, offset, pulse_cap, stereo);
        if stereo && i >= self.intensity {
            qn = 1;
        }
        let tell = self.dec.tell_frac();
        let mut itheta = 0i32;
        let mut inv = false;
        if qn != 1 {
            itheta = if stereo && n > 2 {
                self.step_theta(qn)
            } else if blocks0 > 1 || stereo {
                self.dec.uint((qn + 1) as u32) as i32
            } else {
                self.triangular_theta(qn)
            };
            itheta = itheta * 16384 / qn;
        } else if stereo && part.bits > (2 << BITRES) && self.remaining_bits > (2 << BITRES) {
            inv = self.dec.bit_logp(2);
        }
        let qalloc = self.dec.tell_frac() - tell;
        part.bits -= qalloc;
        let all = (1u32 << part.blocks) - 1;
        let (imid, iside, delta) = if itheta == 0 {
            part.fill &= all;
            (32767, 0, -16384)
        } else if itheta == 16384 {
            part.fill &= all << part.blocks;
            (0, 32767, 16384)
        } else {
            let imid = bitexact_cos(itheta);
            let iside = bitexact_cos(16384 - itheta);
            let delta = frac_mul16((n as i32 - 1) << 7, bitexact_log2tan(iside, imid));
            (imid, iside, delta)
        };
        Theta {
            inv,
            imid,
            iside,
            delta,
            itheta,
            qalloc,
        }
    }

    /// A stereo angle: a step distribution, three times as likely up to the middle.
    fn step_theta(&mut self, qn: i32) -> i32 {
        let p0 = 3u32;
        let x0 = (qn / 2) as u32;
        let ft = p0 * (x0 + 1) + x0;
        let fs = self.dec.decode(ft);
        let x = if fs < (x0 + 1) * p0 {
            fs / p0
        } else {
            x0 + 1 + (fs - (x0 + 1) * p0)
        };
        let (fl, fh) = if x <= x0 {
            (p0 * x, p0 * (x + 1))
        } else {
            ((x - 1 - x0) + (x0 + 1) * p0, (x - x0) + (x0 + 1) * p0)
        };
        self.dec.update(fl, fh, ft);
        x as i32
    }

    /// A split angle in time: a triangular distribution, likeliest in the middle.
    fn triangular_theta(&mut self, qn: i32) -> i32 {
        let qn = qn as u32;
        let half = qn >> 1;
        let ft = (half + 1) * (half + 1);
        let fm = self.dec.decode(ft);
        let (itheta, fl, fs) = if fm < (half * (half + 1)) >> 1 {
            let t = (isqrt(8 * fm + 1) - 1) >> 1;
            (t, (t * (t + 1)) >> 1, t + 1)
        } else {
            let t = (2 * (qn + 1)).saturating_sub(isqrt(8 * (ft - fm - 1) + 1)) >> 1;
            let fs = (qn + 1).saturating_sub(t);
            let fl = ft.saturating_sub(((qn + 1 - t.min(qn + 1)) * (qn + 2 - t.min(qn + 1))) >> 1);
            (t, fl, fs)
        };
        self.dec.update(fl, fl + fs, ft);
        itheta as i32
    }

    /// A sign bit, where there are bits for it.
    fn sign(&mut self) -> f32 {
        let mut negative = false;
        if self.remaining_bits >= 1 << BITRES {
            negative = self.dec.bits(1) == 1;
            self.remaining_bits -= 1 << BITRES;
        }
        if negative {
            -1.0
        } else {
            1.0
        }
    }

    /// A band of one line: its sign alone, for each channel (`quant_band_n1`).
    fn single(
        &mut self,
        x: &mut [f32],
        y: Option<&mut [f32]>,
        lowband_out: Option<&mut [f32]>,
    ) -> u32 {
        let first = self.sign();
        if let Some(v) = x.first_mut() {
            *v = first;
        }
        if let Some(y) = y {
            let second = self.sign();
            if let Some(v) = y.first_mut() {
                *v = second;
            }
        }
        if let (Some(out), Some(v)) = (lowband_out.and_then(|o| o.first_mut()), x.first()) {
            *out = *v;
        }
        1
    }

    /// A band (or a split part of one) of `x.len()` lines in the time-frequency layout the
    /// partition works in (`quant_partition`): split by an angle while its bits are more than
    /// its codebook takes, else one codeword, or folding, or noise.
    fn partition(&mut self, x: &mut [f32], part: Part, lowband: Option<&[f32]>, lm: i32) -> u32 {
        let n = x.len();
        let i = self.band;
        let row = cost_row(lm, i);
        let largest = usize::from(row.first().copied().unwrap_or(0));
        let top = i32::from(row.get(largest).copied().unwrap_or(0));
        if lm != -1 && part.bits > top + 12 && n > 2 {
            return self.split(x, part, lowband, lm);
        }
        let mut q = bits_to_pulses(i, lm, part.bits);
        let mut curr = pulses_to_bits(i, lm, q);
        self.remaining_bits -= curr;
        while self.remaining_bits < 0 && q > 0 {
            self.remaining_bits += curr;
            q -= 1;
            curr = pulses_to_bits(i, lm, q);
            self.remaining_bits -= curr;
        }
        if q != 0 {
            let k = pulses_of(q) as usize;
            return unquantize(self.dec, x, k, self.spread, part.blocks, part.gain);
        }
        let mask = (1u32 << part.blocks) - 1;
        let fill = part.fill & mask;
        if fill == 0 {
            x.fill(0.0);
            return 0;
        }
        let cm = match lowband {
            None => {
                for v in x.iter_mut() {
                    self.seed = lcg(self.seed);
                    *v = ((self.seed as i32) >> 20) as f32;
                }
                mask
            }
            Some(low) => {
                for (v, l) in x.iter_mut().zip(low) {
                    self.seed = lcg(self.seed);
                    let noise = if self.seed & 0x8000 != 0 {
                        1.0 / 256.0
                    } else {
                        -1.0 / 256.0
                    };
                    *v = l + noise;
                }
                fill
            }
        };
        renormalise(x, part.gain);
        cm
    }

    /// [`Self::partition`]'s split: the two halves by the angle read, the larger first.
    fn split(&mut self, x: &mut [f32], part: Part, lowband: Option<&[f32]>, lm: i32) -> u32 {
        let half = x.len() / 2;
        let lm = lm - 1;
        let blocks0 = part.blocks;
        let mut part = part;
        if part.blocks == 1 {
            part.fill = (part.fill & 1) | (part.fill << 1);
        }
        part.blocks = (part.blocks + 1) >> 1;
        let theta = self.theta(half, &mut part, blocks0, lm, false);
        let mid = theta.imid as f32 / 32768.0;
        let side = theta.iside as f32 / 32768.0;
        let mut delta = theta.delta;
        if blocks0 > 1 && (theta.itheta & 0x3fff) != 0 {
            if theta.itheta > 8192 {
                delta -= delta >> (4 - lm);
            } else {
                delta = (delta + (((half as i32) << BITRES) >> (5 - lm))).min(0);
            }
        }
        let b = part.bits;
        let mut mbits = ((b - delta) / 2).min(b).max(0);
        let mut sbits = b - mbits;
        self.remaining_bits -= theta.qalloc;
        let (xa, xb) = x.split_at_mut(half);
        let low_a = lowband.and_then(|l| l.get(..half));
        let low_b = lowband.and_then(|l| l.get(half..));
        let mid_part = |bits| Part {
            bits,
            gain: part.gain * mid,
            ..part
        };
        let side_part = |bits| Part {
            bits,
            gain: part.gain * side,
            fill: part.fill >> part.blocks,
            ..part
        };
        let rebalance = self.remaining_bits;
        if mbits >= sbits {
            let first = self.partition(xa, mid_part(mbits), low_a, lm);
            let spent = mbits - (rebalance - self.remaining_bits);
            if spent > (3 << BITRES) && theta.itheta != 0 {
                sbits += spent - (3 << BITRES);
            }
            first | (self.partition(xb, side_part(sbits), low_b, lm) << (blocks0 >> 1))
        } else {
            let first = self.partition(xb, side_part(sbits), low_b, lm) << (blocks0 >> 1);
            let spent = sbits - (rebalance - self.remaining_bits);
            if spent > (3 << BITRES) && theta.itheta != 16384 {
                mbits += spent - (3 << BITRES);
            }
            first | self.partition(xa, mid_part(mbits), low_a, lm)
        }
    }

    /// A whole band of one channel (`quant_band`): its time-frequency change made on what it
    /// folds from, decoded by [`Self::partition`], and the change undone on the result; what it
    /// leaves for the bands above to fold from goes to `lowband_out`.
    fn band(
        &mut self,
        x: &mut [f32],
        part: Part,
        lowband: Option<&mut [f32]>,
        lowband_out: Option<&mut [f32]>,
        lm: i32,
    ) -> u32 {
        let n0 = x.len();
        if n0 == 1 {
            return self.single(x, None, lowband_out);
        }
        let mut part = part;
        let mut lowband = lowband;
        let long_blocks = part.blocks == 1;
        let mut n_b = n0 / part.blocks.max(1);
        let mut tf_change = self.tf_change;
        let recombine = tf_change.max(0) as usize;
        for k in 0..recombine {
            if let Some(low) = lowband.as_deref_mut() {
                haar1(low, n0 >> k, 1 << k);
            }
            let lo = BIT_INTERLEAVE[(part.fill & 0xF) as usize];
            let hi = BIT_INTERLEAVE[((part.fill >> 4) & 0xF) as usize];
            part.fill = u32::from(lo) | (u32::from(hi) << 2);
        }
        part.blocks >>= recombine;
        part.blocks = part.blocks.max(1);
        n_b <<= recombine;
        let mut time_divide = 0;
        while n_b & 1 == 0 && tf_change < 0 {
            if let Some(low) = lowband.as_deref_mut() {
                haar1(low, n_b, part.blocks);
            }
            part.fill |= part.fill << part.blocks;
            part.blocks <<= 1;
            n_b >>= 1;
            time_divide += 1;
            tf_change += 1;
        }
        let blocks0 = part.blocks;
        let n_b0 = n_b;
        if let Some(low) = lowband.as_deref_mut().filter(|_| blocks0 > 1) {
            deinterleave(low, n_b >> recombine, blocks0 << recombine, long_blocks);
        }
        let mut cm = self.partition(x, part, lowband.as_deref(), lm);
        if blocks0 > 1 {
            interleave(x, n_b >> recombine, blocks0 << recombine, long_blocks);
        }
        let mut n_b = n_b0;
        let mut blocks = blocks0;
        for _ in 0..time_divide {
            blocks >>= 1;
            n_b <<= 1;
            cm |= cm >> blocks;
            haar1(x, n_b, blocks);
        }
        for k in 0..recombine {
            cm = u32::from(BIT_DEINTERLEAVE.get(cm as usize).copied().unwrap_or(0xFF));
            haar1(x, n0 >> k, 1 << k);
        }
        blocks <<= recombine;
        if let Some(out) = lowband_out {
            let scale = (n0 as f32).sqrt();
            for (o, v) in out.iter_mut().zip(x.iter()) {
                *o = scale * v;
            }
        }
        cm & ((1u32 << blocks) - 1)
    }

    /// A band of both channels coded as mid and side by an angle (`quant_band_stereo`).
    fn stereo(
        &mut self,
        x: &mut [f32],
        y: &mut [f32],
        part: Part,
        lowband: Option<&mut [f32]>,
        lowband_out: Option<&mut [f32]>,
        lm: i32,
    ) -> u32 {
        let n = x.len();
        if n == 1 {
            return self.single(x, Some(y), lowband_out);
        }
        let orig_fill = part.fill;
        let mut part = part;
        let blocks = part.blocks;
        let theta = self.theta(n, &mut part, blocks, lm, true);
        let mid = theta.imid as f32 / 32768.0;
        let side = theta.iside as f32 / 32768.0;
        let cm;
        if n == 2 {
            let sbits = if theta.itheta != 0 && theta.itheta != 16384 {
                1 << BITRES
            } else {
                0
            };
            let mbits = part.bits - sbits;
            self.remaining_bits -= theta.qalloc + sbits;
            let sign = if sbits != 0 && self.dec.bits(1) == 1 {
                -1.0
            } else {
                1.0
            };
            let inner = Part {
                bits: mbits,
                gain: 1.0,
                fill: orig_fill,
                ..part
            };
            let (x2, y2) = if theta.itheta > 8192 {
                (&mut *y, &mut *x)
            } else {
                (&mut *x, &mut *y)
            };
            cm = self.band(x2, inner, lowband, lowband_out, lm);
            y2[0] = -sign * x2[1];
            y2[1] = sign * x2[0];
            for v in x.iter_mut() {
                *v *= mid;
            }
            for v in y.iter_mut() {
                *v *= side;
            }
            for (a, b) in x.iter_mut().zip(y.iter_mut()) {
                let tmp = *a;
                *a = tmp - *b;
                *b = tmp + *b;
            }
        } else {
            let b = part.bits;
            let mut mbits = ((b - theta.delta) / 2).min(b).max(0);
            let mut sbits = b - mbits;
            self.remaining_bits -= theta.qalloc;
            let mid_part = |bits| Part {
                bits,
                gain: 1.0,
                ..part
            };
            let side_part = |bits| Part {
                bits,
                gain: side,
                fill: part.fill >> part.blocks,
                ..part
            };
            let rebalance = self.remaining_bits;
            if mbits >= sbits {
                let first = self.band(x, mid_part(mbits), lowband, lowband_out, lm);
                let spent = mbits - (rebalance - self.remaining_bits);
                if spent > (3 << BITRES) && theta.itheta != 0 {
                    sbits += spent - (3 << BITRES);
                }
                cm = first | self.band(y, side_part(sbits), None, None, lm);
            } else {
                let first = self.band(y, side_part(sbits), None, None, lm);
                let spent = sbits - (rebalance - self.remaining_bits);
                if spent > (3 << BITRES) && theta.itheta != 16384 {
                    mbits += spent - (3 << BITRES);
                }
                cm = first | self.band(x, mid_part(mbits), lowband, lowband_out, lm);
            }
            stereo_merge(x, y, mid);
        }
        if theta.inv {
            for v in y.iter_mut() {
                *v = -*v;
            }
        }
        cm
    }
}

/// `n` lines of `buf` from `at`, copied into `scratch` for a band to fold from.
fn take_lowband<'s>(
    buf: &[f32],
    at: Option<usize>,
    n: usize,
    scratch: &'s mut Vec<f32>,
) -> Option<&'s mut [f32]> {
    let at = at?;
    let src = buf.get(at..at + n)?;
    scratch.clear();
    scratch.extend_from_slice(src);
    Some(scratch.as_mut_slice())
}

/// Every band's shape, both channels (`quant_all_bands`), into `x` (and `y` for stereo), and
/// each band's collapse mask; returns the noise generator's state after.
pub(super) fn decode_bands(
    dec: &mut RangeDecoder<'_>,
    frame: &BandFrame<'_>,
    x: &mut [f32],
    mut y: Option<&mut [f32]>,
    collapse: &mut [u8],
    seed: u32,
) -> u32 {
    let span = frame.span;
    let alloc = frame.alloc;
    let (start, end, lm) = (span.start, span.end, span.lm);
    let m = 1usize << lm;
    let blocks = if frame.transient { m } else { 1 };
    let c = if y.is_some() { 2 } else { 1 };
    let norm_offset = m * EBANDS[start];
    let norm_len = m * EBANDS[BANDS - 1] - norm_offset;
    let mut norm = vec![0f32; norm_len];
    let mut norm2 = vec![0f32; norm_len];
    let mut scratch = Vec::new();
    let mut lowband_offset = 0usize;
    let mut update_lowband = true;
    let mut dual_stereo = alloc.dual_stereo;
    let mut balance = alloc.balance;
    let mut ctx = Ctx {
        dec,
        band: start,
        intensity: alloc.intensity,
        spread: frame.spread,
        tf_change: 0,
        remaining_bits: 0,
        seed,
    };
    for i in start..end {
        ctx.band = i;
        let last = i == end - 1;
        let band_start = m * EBANDS[i];
        let n = m * EBANDS[i + 1] - band_start;
        let tell = ctx.dec.tell_frac();
        if i != start {
            balance -= tell;
        }
        let remaining = frame.total_bits - tell - 1;
        ctx.remaining_bits = remaining;
        let b = if i < alloc.coded_bands {
            let share = balance / ((alloc.coded_bands - i).min(3) as i32);
            (remaining + 1).min(alloc.pulses[i] + share).clamp(0, 16383)
        } else {
            0
        };
        if (band_start >= n + norm_offset || i == start + 1)
            && (update_lowband || lowband_offset == 0)
        {
            lowband_offset = i;
        }
        if i == start + 1 {
            // A hybrid frame's first band copied up, so the second can fold from it.
            let n1 = m * (EBANDS[start + 1] - EBANDS[start]);
            let n2 = m * (EBANDS[start + 2] - EBANDS[start + 1]);
            if n2 > n1 && 2 * n1 >= n2 {
                norm.copy_within(2 * n1 - n2..n1, n1);
                if dual_stereo {
                    norm2.copy_within(2 * n1 - n2..n1, n1);
                }
            }
        }
        ctx.tf_change = frame.tf_res[i];
        let full = (1u32 << blocks) - 1;
        let (mut x_cm, mut y_cm) = (full, full);
        let mut effective_lowband = None;
        if lowband_offset != 0
            && (frame.spread != SPREAD_AGGRESSIVE || blocks > 1 || ctx.tf_change < 0)
        {
            let eff = (m * EBANDS[lowband_offset]).saturating_sub(norm_offset + n);
            effective_lowband = Some(eff);
            let mut fold_start = lowband_offset;
            loop {
                fold_start -= 1;
                if fold_start == 0 || m * EBANDS[fold_start] <= eff + norm_offset {
                    break;
                }
            }
            let mut fold_end = lowband_offset - 1;
            loop {
                fold_end += 1;
                if fold_end >= i || m * EBANDS[fold_end] >= eff + norm_offset + n {
                    break;
                }
            }
            x_cm = 0;
            y_cm = 0;
            for f in fold_start..fold_end.max(fold_start + 1) {
                x_cm |= u32::from(collapse[f * c]);
                y_cm |= u32::from(collapse[f * c + c - 1]);
            }
        }
        if dual_stereo && i == alloc.intensity {
            dual_stereo = false;
            for (a, b) in norm
                .iter_mut()
                .zip(norm2.iter())
                .take(band_start - norm_offset)
            {
                *a = 0.5 * (*a + *b);
            }
        }
        let out_at = band_start - norm_offset;
        let xb = &mut x[band_start..band_start + n];
        let lm = lm as i32;
        if dual_stereo {
            let low = take_lowband(&norm, effective_lowband, n, &mut scratch);
            let out = if last {
                None
            } else {
                norm.get_mut(out_at..out_at + n)
            };
            let part = Part {
                bits: b / 2,
                blocks,
                gain: 1.0,
                fill: x_cm,
            };
            x_cm = ctx.band(xb, part, low, out, lm);
            if let Some(y) = y.as_deref_mut() {
                let low = take_lowband(&norm2, effective_lowband, n, &mut scratch);
                let out = if last {
                    None
                } else {
                    norm2.get_mut(out_at..out_at + n)
                };
                let part = Part { fill: y_cm, ..part };
                y_cm = ctx.band(&mut y[band_start..band_start + n], part, low, out, lm);
            }
        } else {
            let low = take_lowband(&norm, effective_lowband, n, &mut scratch);
            let out = if last {
                None
            } else {
                norm.get_mut(out_at..out_at + n)
            };
            let part = Part {
                bits: b,
                blocks,
                gain: 1.0,
                fill: x_cm | y_cm,
            };
            x_cm = match y.as_deref_mut() {
                Some(y) => ctx.stereo(xb, &mut y[band_start..band_start + n], part, low, out, lm),
                None => ctx.band(xb, part, low, out, lm),
            };
            y_cm = x_cm;
        }
        collapse[i * c] = x_cm as u8;
        collapse[i * c + c - 1] = y_cm as u8;
        balance += alloc.pulses[i] + tell;
        update_lowband = b > ((n as i32) << BITRES);
    }
    ctx.seed
}
