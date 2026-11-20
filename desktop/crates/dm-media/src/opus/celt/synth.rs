// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! CELT's sound out of its spectrum (RFC 6716, 4.3.6 and 4.3.7): the inverse MDCT of each
//! block, windowed over CELT's short overlap and added to the block before; the pitch
//! post-filter, a comb filter whose period and gain the frame names; and the de-emphasis.
//!
//! The inverse MDCT is a DCT-IV of the block's coefficients, done as a complex FFT of a
//! quarter of the transform's size between two rotations; the FFT is a plain mixed-radix one
//! (CELT's sizes are 2^a * 3 * 5).

use std::f64::consts::PI;
use std::sync::OnceLock;

/// The overlap between frames, in samples.
pub(super) const OVERLAP: usize = 120;

/// The window over the overlap: `sin(pi/2 * sin^2(pi/2 * (i + 1/2) / overlap))`.
pub(super) fn window() -> &'static [f32; OVERLAP] {
    static WINDOW: OnceLock<[f32; OVERLAP]> = OnceLock::new();
    WINDOW.get_or_init(|| {
        let mut w = [0.0f32; OVERLAP];
        for (i, v) in w.iter_mut().enumerate() {
            let inner = (0.5 * PI * (i as f64 + 0.5) / OVERLAP as f64).sin();
            *v = (0.5 * PI * inner * inner).sin() as f32;
        }
        w
    })
}

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

impl Complex {
    fn times(self, o: Complex) -> Complex {
        Complex {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }

    fn plus(self, o: Complex) -> Complex {
        Complex {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }
}

/// A forward complex FFT of one size, unscaled.
struct Fft {
    n: usize,
    factors: Vec<usize>,
    /// `e^(-2 pi i k / n)`.
    twiddles: Vec<Complex>,
}

impl Fft {
    fn new(n: usize) -> Fft {
        let mut factors = Vec::new();
        let mut rest = n.max(1);
        for p in [4, 2, 3, 5] {
            while rest % p == 0 {
                factors.push(p);
                rest /= p;
            }
        }
        if rest > 1 {
            factors.push(rest);
        }
        let twiddles = (0..n)
            .map(|k| {
                let a = -2.0 * PI * k as f64 / n as f64;
                Complex {
                    re: a.cos() as f32,
                    im: a.sin() as f32,
                }
            })
            .collect();
        Fft {
            n,
            factors,
            twiddles,
        }
    }

    /// The DFT of `x` into `out` (both `n` long).
    fn run(&self, x: &[Complex], out: &mut [Complex]) {
        self.step(x, 1, out, 0);
    }

    /// The DFT of `x[0], x[stride], ...` (`out.len()` of them) into `out`, by decimation in
    /// time over the factor at `level`.
    fn step(&self, x: &[Complex], stride: usize, out: &mut [Complex], level: usize) {
        let n = out.len();
        let Some(&p) = self.factors.get(level) else {
            if let (Some(o), Some(v)) = (out.first_mut(), x.first()) {
                *o = *v;
            }
            return;
        };
        let m = n / p;
        for (r, part) in out.chunks_mut(m.max(1)).take(p).enumerate() {
            self.step(
                x.get(r * stride..).unwrap_or(&[]),
                stride * p,
                part,
                level + 1,
            );
        }
        let step = self.n / n.max(1);
        let mut tmp = vec![Complex::default(); p];
        for k in 0..m {
            for (r, t) in tmp.iter_mut().enumerate() {
                *t = out[r * m + k];
            }
            for q in 0..p {
                let at = k + q * m;
                let mut sum = Complex::default();
                for (r, t) in tmp.iter().enumerate() {
                    sum = sum.plus(t.times(self.twiddles[(r * at * step) % self.n]));
                }
                out[at] = sum;
            }
        }
    }
}

/// The inverse MDCT of `n` coefficients, as CELT's overlap-add takes it.
struct Imdct {
    n: usize,
    fft: Fft,
    /// `e^(-i pi (4k + 1) / (4n))`, before and after the FFT.
    rotation: Vec<Complex>,
}

impl Imdct {
    fn new(n: usize) -> Imdct {
        let rotation = (0..n / 2)
            .map(|k| {
                let a = -PI * (4 * k + 1) as f64 / (4 * n) as f64;
                Complex {
                    re: a.cos() as f32,
                    im: a.sin() as f32,
                }
            })
            .collect();
        Imdct {
            n,
            fft: Fft::new(n / 2),
            rotation,
        }
    }

    /// The coefficients `coeffs[first]`, `coeffs[first + stride]`, ... turned into the
    /// middle half of their inverse MDCT, `out[..n]`.
    fn run(&self, coeffs: &[f32], first: usize, stride: usize, out: &mut [f32]) {
        let n = self.n;
        let at = |k: usize| coeffs.get(first + k * stride).copied().unwrap_or(0.0);
        let folded: Vec<Complex> = self
            .rotation
            .iter()
            .enumerate()
            .map(|(k, r)| {
                Complex {
                    re: at(2 * k),
                    im: at(n - 1 - 2 * k),
                }
                .times(*r)
            })
            .collect();
        let mut spectrum = vec![Complex::default(); n / 2];
        self.fft.run(&folded, &mut spectrum);
        for (k, (z, r)) in spectrum.iter().zip(&self.rotation).enumerate() {
            let c = z.times(*r);
            out[2 * k] = c.im;
            out[n - 1 - 2 * k] = -c.re;
        }
    }
}

/// The inverse MDCTs of CELT's four block sizes (120 to 960 coefficients).
fn transforms() -> &'static [Imdct; 4] {
    static IMDCT: OnceLock<[Imdct; 4]> = OnceLock::new();
    IMDCT.get_or_init(|| {
        [
            Imdct::new(120),
            Imdct::new(240),
            Imdct::new(480),
            Imdct::new(960),
        ]
    })
}

/// A frame's spectrum `freq` (its `blocks` short blocks interleaved) turned into sound and
/// overlapped with what is in `out` (the frame's samples and the overlap after them).
pub(super) fn inverse_mdct(freq: &[f32], blocks: usize, out: &mut [f32]) {
    let blocks = blocks.max(1);
    let nb = freq.len() / blocks;
    let Some(imdct) = transforms().iter().find(|t| t.n == nb) else {
        return;
    };
    let w = window();
    let half = OVERLAP / 2;
    let mut block = vec![0.0f32; nb];
    for b in 0..blocks {
        imdct.run(freq, b, blocks, &mut block);
        let at = nb * b;
        let Some(region) = out.get_mut(at..at + nb + OVERLAP) else {
            return;
        };
        region[half..half + nb].copy_from_slice(&block);
        // The time-domain aliasing cancelled against the block before, windowed.
        for i in 0..half {
            let x1 = region[OVERLAP - 1 - i];
            let x2 = region[i];
            let (w1, w2) = (w[i], w[OVERLAP - 1 - i]);
            region[i] = w2 * x2 - w1 * x1;
            region[OVERLAP - 1 - i] = w1 * x2 + w2 * x1;
        }
    }
}

/// The pitch post-filter's settings for a frame: its period in samples, its gain and its
/// tap set.
#[derive(Clone, Copy, Default)]
pub(super) struct Filter {
    pub(super) period: usize,
    pub(super) gain: f32,
    pub(super) tapset: usize,
}

impl Filter {
    fn taps(self) -> [f32; 3] {
        let row = super::tables::POSTFILTER_TAPS[self.tapset.min(2)];
        row.map(|t| self.gain * t / 32768.0)
    }
}

/// The comb filter over `buf[at..at + n]`, in place (it feeds back on what it made): from
/// `old` to `new` over the first `overlap` samples, then `new` (RFC 6716, 4.3.7.1).
pub(super) fn comb_filter(
    buf: &mut [f32],
    at: usize,
    n: usize,
    old: Filter,
    new: Filter,
    overlap: usize,
) {
    if old.gain == 0.0 && new.gain == 0.0 {
        return;
    }
    let t0 = old.period.max(15);
    let t1 = new.period.max(15);
    if at < t0.max(t1) + 2 || at + n > buf.len() {
        return;
    }
    let g0 = old.taps();
    let g1 = new.taps();
    let unchanged = old.gain == new.gain && t0 == t1 && old.tapset == new.tapset;
    let overlap = if unchanged { 0 } else { overlap.min(n) };
    let w = window();
    for i in 0..n {
        let p = at + i;
        let newer = g1[0] * buf[p - t1]
            + g1[1] * (buf[p + 1 - t1] + buf[p - 1 - t1])
            + g1[2] * (buf[p + 2 - t1] + buf[p - 2 - t1]);
        if i < overlap {
            let f = w[i] * w[i];
            let older = g0[0] * buf[p - t0]
                + g0[1] * (buf[p + 1 - t0] + buf[p - 1 - t0])
                + g0[2] * (buf[p + 2 - t0] + buf[p - 2 - t0]);
            buf[p] += (1.0 - f) * older + f * newer;
        } else if new.gain == 0.0 {
            break;
        } else {
            buf[p] += newer;
        }
    }
}

/// The pre-emphasis CELT's encoder applies, undone: `y[n] = x[n] + 0.85 y[n-1]`.
const PREEMPH: f32 = 27853.0 / 32768.0;

/// `x` de-emphasized into every `stride`th value of `out`, from `state`, which it keeps.
pub(super) fn deemphasis(x: &[f32], state: &mut f32, out: &mut [f32], stride: usize) {
    let mut m = *state;
    for (o, v) in out.iter_mut().step_by(stride.max(1)).zip(x) {
        let tmp = v + 1e-30 + m;
        m = PREEMPH * tmp;
        *o = tmp;
    }
    *state = m;
}
