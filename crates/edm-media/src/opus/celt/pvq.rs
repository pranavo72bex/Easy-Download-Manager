// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A band's shape (RFC 6716, 4.3.4): a pyramid vector of whole numbers read from its codeword
//! index, scaled to unit length and spread; and the small transforms the bands are reordered
//! by (Haar, Hadamard), the split angle's exact cosine and the noise generator.

use super::tables::ORDERY;
use crate::opus::range::RangeDecoder;

/// No spreading, light, normal, aggressive (RFC 6716, 4.3.4.3).
pub(super) const SPREAD_NONE: u32 = 0;
/// See [`SPREAD_NONE`].
pub(super) const SPREAD_NORMAL: u32 = 2;
/// See [`SPREAD_NONE`].
pub(super) const SPREAD_AGGRESSIVE: u32 = 3;

/// The next value of the decoder's noise generator (a linear congruential one).
pub(super) fn lcg(seed: u32) -> u32 {
    seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// `V(d, m)` for every dimension `d` up to `n` and `m` up to `k`, a row a dimension: how many
/// vectors of `d` whole numbers have magnitudes that sum to `m` (`u64::MAX` where more).
pub(super) fn pvq_rows(n: usize, k: usize) -> Vec<u64> {
    let width = k + 1;
    let mut rows = vec![0u64; (n + 1) * width];
    if let Some(first) = rows.first_mut() {
        *first = 1;
    }
    for d in 1..=n {
        let (above, here) = rows.split_at_mut(d * width);
        let above = &above[(d - 1) * width..];
        let here = &mut here[..width];
        here[0] = 1;
        for m in 1..width {
            here[m] = above[m]
                .saturating_add(here[m - 1])
                .saturating_add(above[m - 1]);
        }
    }
    rows
}

/// How many codewords a band of `n` lines and `k` pulses has, `V(n, k)`.
pub(super) fn pvq_size(n: usize, k: usize) -> u64 {
    pvq_rows(n, k).last().copied().unwrap_or(1)
}

/// The pulse vector `y` (`k` pulses) a codeword index stands for (RFC 6716, 4.3.4.2); returns
/// the sum of the squares. Index 0 is all `k` pulses on the first line, positive; vectors are
/// counted with the first line's value largest and positive first, as `V(n, k)` splits them.
pub(super) fn pulse_vector(index: u64, k: usize, y: &mut [i32]) -> f32 {
    let n = y.len();
    let width = k + 1;
    let rows = pvq_rows(n, k);
    let mut index = index;
    let mut left = k;
    let mut energy = 0.0f32;
    for (j, value) in y.iter_mut().enumerate() {
        if left == 0 {
            *value = 0;
            continue;
        }
        let row = &rows[(n - j - 1) * width..(n - j) * width];
        // Vectors whose first value is 0 or positive: U(d, left + 1).
        let positive: u64 = row[..=left]
            .iter()
            .fold(0u64, |sum, v| sum.saturating_add(*v));
        let negative = index >= positive;
        if negative {
            index -= positive;
        }
        let was = left;
        let mut before = positive - row[left];
        while before > index && left > 0 {
            left -= 1;
            before -= row[left];
        }
        index -= before;
        let magnitude = (was - left) as i32;
        *value = if negative { -magnitude } else { magnitude };
        energy += (magnitude * magnitude) as f32;
    }
    energy
}

/// A band's shape from its codeword: `k` pulses, scaled to length `gain` and spread
/// (`alg_unquant`); returns which of its `blocks` short blocks got a pulse.
pub(super) fn unquantize(
    dec: &mut RangeDecoder<'_>,
    x: &mut [f32],
    k: usize,
    spread: u32,
    blocks: usize,
    gain: f32,
) -> u32 {
    let mut iy = vec![0i32; x.len()];
    let size = pvq_size(x.len(), k).min(u64::from(u32::MAX)) as u32;
    let index = u64::from(dec.uint(size));
    let energy = pulse_vector(index, k, &mut iy);
    let g = gain / energy.max(1.0).sqrt();
    for (out, pulse) in x.iter_mut().zip(&iy) {
        *out = g * *pulse as f32;
    }
    rotate(x, -1, blocks, k, spread);
    if blocks <= 1 {
        return 1;
    }
    let per = x.len() / blocks;
    let mut mask = 0u32;
    for (b, chunk) in iy.chunks(per.max(1)).take(blocks).enumerate() {
        if chunk.iter().any(|v| *v != 0) {
            mask |= 1 << b;
        }
    }
    mask
}

/// One pass of the spreading rotation over pairs `stride` apart, both ways.
fn rotate_pass(x: &mut [f32], stride: usize, c: f32, s: f32) {
    let len = x.len();
    if len <= stride {
        return;
    }
    for i in 0..len - stride {
        let x1 = x[i];
        let x2 = x[i + stride];
        x[i + stride] = c * x2 + s * x1;
        x[i] = c * x1 - s * x2;
    }
    if len < 2 * stride + 1 {
        return;
    }
    for i in (0..len - 2 * stride).rev() {
        let x1 = x[i];
        let x2 = x[i + stride];
        x[i + stride] = c * x2 + s * x1;
        x[i] = c * x1 - s * x2;
    }
}

/// The spreading rotation (RFC 6716, 4.3.4.3), undone (`direction` -1) as the decoder does.
pub(super) fn rotate(x: &mut [f32], direction: i32, stride: usize, k: usize, spread: u32) {
    let len = x.len();
    if 2 * k >= len || spread == SPREAD_NONE {
        return;
    }
    let factor = [15.0f32, 10.0, 5.0][(spread as usize).clamp(1, 3) - 1];
    let gain = len as f32 / (len as f32 + factor * k as f32);
    let theta = 0.5 * gain * gain;
    let c = (0.5 * std::f32::consts::PI * theta).cos();
    let s = (0.5 * std::f32::consts::PI * (1.0 - theta)).cos();
    let stride = stride.max(1);
    let mut stride2 = 0;
    if len >= 8 * stride {
        stride2 = 1;
        while (stride2 * stride2 + stride2) * stride + (stride >> 2) < len {
            stride2 += 1;
        }
    }
    let each = len / stride;
    for block in x.chunks_mut(each.max(1)).take(stride) {
        if direction < 0 {
            if stride2 > 0 {
                rotate_pass(block, stride2, s, c);
            }
            rotate_pass(block, 1, c, s);
        } else {
            rotate_pass(block, 1, c, -s);
            if stride2 > 0 {
                rotate_pass(block, stride2, s, -c);
            }
        }
    }
}

/// Scales `x` to length `gain`.
pub(super) fn renormalise(x: &mut [f32], gain: f32) {
    let energy: f32 = 1e-15 + x.iter().map(|v| v * v).sum::<f32>();
    let g = gain / energy.sqrt();
    for v in x.iter_mut() {
        *v *= g;
    }
}

/// One level of the Haar transform over `n0` values `stride` apart.
pub(super) fn haar1(x: &mut [f32], n0: usize, stride: usize) {
    let half = std::f32::consts::FRAC_1_SQRT_2;
    for i in 0..stride {
        for j in 0..n0 / 2 {
            let (a, b) = (stride * 2 * j + i, stride * (2 * j + 1) + i);
            if let (Some(&va), Some(&vb)) = (x.get(a), x.get(b)) {
                x[a] = half * (va + vb);
                x[b] = half * (va - vb);
            }
        }
    }
}

/// Where block `i` of `stride` goes in the Hadamard order.
fn ordery(stride: usize, i: usize) -> usize {
    ORDERY.get(stride.wrapping_sub(2) + i).copied().unwrap_or(i)
}

/// Puts `stride` interleaved blocks of `n0` values one after another (in the Hadamard order
/// where `hadamard`).
pub(super) fn deinterleave(x: &mut [f32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    let mut tmp = vec![0.0f32; n];
    for i in 0..stride {
        let row = if hadamard { ordery(stride, i) } else { i };
        for j in 0..n0 {
            tmp[row * n0 + j] = x[j * stride + i];
        }
    }
    x[..n].copy_from_slice(&tmp);
}

/// Undoes [`deinterleave`].
pub(super) fn interleave(x: &mut [f32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    let mut tmp = vec![0.0f32; n];
    for i in 0..stride {
        let row = if hadamard { ordery(stride, i) } else { i };
        for j in 0..n0 {
            tmp[j * stride + i] = x[row * n0 + j];
        }
    }
    x[..n].copy_from_slice(&tmp);
}

/// `(16384 + a * b) >> 15` on 16-bit `a` and `b`, as the standard's exact arithmetic does.
pub(super) fn frac_mul16(a: i32, b: i32) -> i32 {
    (16384 + i32::from(a as i16) * i32::from(b as i16)) >> 15
}

/// The split angle's cosine, exactly as the standard computes it (`x` of 16384 is a right
/// angle; 32767 is 1).
pub(super) fn bitexact_cos(x: i32) -> i32 {
    let tmp = (4096 + x * x) >> 13;
    let x2 = tmp;
    let x2 = (32767 - x2) + frac_mul16(x2, -7651 + frac_mul16(x2, 8277 + frac_mul16(-626, x2)));
    1 + x2
}

/// `log2(sin / cos)` of the split angle in Q11, exactly as the standard computes it.
pub(super) fn bitexact_log2tan(isin: i32, icos: i32) -> i32 {
    let lc = 32 - (icos.max(1) as u32).leading_zeros() as i32;
    let ls = 32 - (isin.max(1) as u32).leading_zeros() as i32;
    let icos = icos << (15 - lc);
    let isin = isin << (15 - ls);
    (ls - lc) * (1 << 11) + frac_mul16(isin, frac_mul16(isin, -2597) + 7932)
        - frac_mul16(icos, frac_mul16(icos, -2597) + 7932)
}

/// The whole square root of `v`.
pub(super) fn isqrt(v: u32) -> u32 {
    let mut r = (f64::from(v)).sqrt() as u32;
    while u64::from(r) * u64::from(r) > u64::from(v) {
        r -= 1;
    }
    while u64::from(r + 1) * u64::from(r + 1) <= u64::from(v) {
        r += 1;
    }
    r
}
