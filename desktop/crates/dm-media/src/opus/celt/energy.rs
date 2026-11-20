// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A CELT frame's band energies (RFC 6716, 4.3.2), base-2 logarithms of each band's
//! amplitude: a coarse value per band, predicted from the frame before and the band below and
//! coded by a Laplace distribution, then finer bits; also the time-frequency flags (4.3.4.5)
//! and the anti-collapse noise (4.3.5), which hang on the energies too.

use super::alloc::{BITRES, MAX_FINE_BITS};
use super::pvq::{lcg, renormalise};
use super::tables::{
    BANDS, BETA_COEF, BETA_INTRA, EBANDS, ENERGY_MODEL, PRED_COEF, SMALL_ENERGY_ICDF, TF_SELECT,
};
use super::Span;
use crate::opus::range::{laplace, RangeDecoder};

/// The coarse energies (`unquant_coarse_energy`), added to `old` (both channels' bands, the
/// second at `BANDS`).
pub(super) fn coarse(dec: &mut RangeDecoder<'_>, span: &Span, old: &mut [f32], intra: bool) {
    let model = &ENERGY_MODEL[span.lm][usize::from(intra)];
    let (coef, beta) = if intra {
        (0.0, BETA_INTRA / 32768.0)
    } else {
        (PRED_COEF[span.lm] / 32768.0, BETA_COEF[span.lm] / 32768.0)
    };
    let budget = dec.total_bits();
    let mut prev = [0.0f32; 2];
    for i in span.start..span.end {
        for (c, carry) in prev.iter_mut().enumerate().take(span.channels) {
            let tell = dec.tell();
            let qi = if budget - tell >= 15 {
                let at = 2 * i.min(20);
                let fs = u32::from(model[at]) << 7;
                let decay = u32::from(model[at + 1]) << 6;
                laplace(dec, fs, decay)
            } else if budget - tell >= 2 {
                let q = dec.icdf(&SMALL_ENERGY_ICDF, 2) as i32;
                (q >> 1) ^ -(q & 1)
            } else if budget - tell >= 1 {
                -i32::from(dec.bit_logp(1))
            } else {
                -1
            };
            let q = qi as f32;
            let k = i + c * BANDS;
            let before = old[k].max(-9.0);
            old[k] = coef * before + *carry + q;
            *carry += q - beta * q;
        }
    }
}

/// The fine energies' first bits (`unquant_fine_energy`).
pub(super) fn fine(dec: &mut RangeDecoder<'_>, span: &Span, old: &mut [f32], bits: &[i32]) {
    for i in span.start..span.end {
        let b = bits[i];
        if !(1..=MAX_FINE_BITS).contains(&b) {
            continue;
        }
        for c in 0..span.channels {
            let q2 = dec.bits(b as u32) as f32;
            let offset = (q2 + 0.5) * (1 << (14 - b)) as f32 / 16384.0 - 0.5;
            old[i + c * BANDS] += offset;
        }
    }
}

/// The bits left at the frame's end, one per band and channel while they last, by priority
/// (`unquant_energy_finalise`).
pub(super) fn finalise(
    dec: &mut RangeDecoder<'_>,
    span: &Span,
    old: &mut [f32],
    bits: &[i32],
    priority: &[i32],
    left: i32,
) {
    let c = span.channels as i32;
    let mut left = left;
    for prio in 0..2 {
        for i in span.start..span.end {
            if left < c {
                break;
            }
            if !(0..MAX_FINE_BITS).contains(&bits[i]) || priority[i] != prio {
                continue;
            }
            for ch in 0..span.channels {
                let q2 = dec.bits(1) as f32;
                let offset = (q2 - 0.5) * (1 << (14 - bits[i] - 1)) as f32 / 16384.0;
                old[i + ch * BANDS] += offset;
                left -= 1;
            }
        }
    }
}

/// Each band's time-frequency change (`tf_decode`, RFC 6716, 4.3.4.5).
pub(super) fn tf_decode(dec: &mut RangeDecoder<'_>, span: &Span, transient: bool) -> [i32; BANDS] {
    let mut res = [0i32; BANDS];
    let mut budget = dec.total_bits();
    let mut tell = dec.tell();
    let lm = span.lm;
    let mut logp = if transient { 2 } else { 4 };
    let reserve = lm > 0 && tell + logp < budget;
    budget -= i32::from(reserve);
    let mut changed = 0usize;
    let mut curr = 0usize;
    for r in res.iter_mut().take(span.end).skip(span.start) {
        if tell + logp <= budget {
            curr ^= usize::from(dec.bit_logp(logp as u32));
            tell = dec.tell();
            changed |= curr;
        }
        *r = curr as i32;
        logp = if transient { 4 } else { 5 };
    }
    let table = &TF_SELECT[lm];
    let base = 4 * usize::from(transient);
    let select = reserve && table[base + changed] != table[base + 2 + changed] && dec.bit_logp(1);
    let at = base + 2 * usize::from(select);
    for r in res.iter_mut().take(span.end).skip(span.start) {
        *r = i32::from(table[at + *r as usize]);
    }
    res
}

/// What the frames before left of each band's energy, for the anti-collapse noise.
pub(super) struct History<'a> {
    /// The energies just decoded.
    pub(super) now: &'a [f32],
    /// The frame before's.
    pub(super) prev1: &'a [f32],
    /// The frame before that's.
    pub(super) prev2: &'a [f32],
}

/// Noise put in the short blocks of a transient frame's bands that got no pulse
/// (`anti_collapse`, RFC 6716, 4.3.5), from the noise generator at `seed`.
pub(super) fn anti_collapse(
    span: &Span,
    x: &mut [f32],
    collapse: &[u8],
    history: &History<'_>,
    pulses: &[i32],
    seed: u32,
) {
    let lm = span.lm;
    let c_count = span.channels;
    let size = x.len() / c_count.max(1);
    let mut seed = seed;
    for i in span.start..span.end {
        let n0 = EBANDS[i + 1] - EBANDS[i];
        let depth = ((1 + pulses[i]) / n0 as i32) >> lm;
        let thresh = 0.5 * (-0.125 * depth as f32).exp2();
        let sqrt_1 = 1.0 / ((n0 << lm) as f32).sqrt();
        for c in 0..c_count {
            let mut prev1 = history.prev1[c * BANDS + i];
            let mut prev2 = history.prev2[c * BANDS + i];
            if c_count == 1 {
                prev1 = prev1.max(history.prev1[BANDS + i]);
                prev2 = prev2.max(history.prev2[BANDS + i]);
            }
            let ediff = (history.now[c * BANDS + i] - prev1.min(prev2)).max(0.0);
            let mut r = 2.0 * (-ediff).exp2();
            if lm == 3 {
                r *= std::f32::consts::SQRT_2;
            }
            let r = r.min(thresh) * sqrt_1;
            let base = c * size + (EBANDS[i] << lm);
            let band = &mut x[base..base + (n0 << lm)];
            let mut renormalize = false;
            for k in 0..1usize << lm {
                if u32::from(collapse[i * c_count + c]) & (1 << k) != 0 {
                    continue;
                }
                for j in 0..n0 {
                    seed = lcg(seed);
                    band[(j << lm) + k] = if seed & 0x8000 != 0 { r } else { -r };
                }
                renormalize = true;
            }
            if renormalize {
                renormalise(band, 1.0);
            }
        }
    }
}

/// The bits (eighths) a band's boost costs at a time: six bits, but not more than one a line
/// nor less than an eighth.
pub(super) fn boost_quanta(span: &Span, band: usize) -> i32 {
    let width = ((span.channels * (EBANDS[band + 1] - EBANDS[band])) << span.lm) as i32;
    (width << BITRES).min((6 << BITRES).max(width))
}
