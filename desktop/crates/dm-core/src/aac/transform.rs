//! The filterbank: inverse MDCT by FFT, the window shapes, and overlap-add.

use std::f64::consts::PI;

/// In-place radix-2 FFT of `re`/`im` (length a power of two); `inverse` flips the sign of the
/// exponent (no scaling).
fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len().min(im.len());
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let angle = sign * 2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (angle * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// The standard's inverse MDCT: `spec.len()` lines to twice as many samples,
/// `x[n] = 2/N0 * sum spec[k] cos(2 pi/N0 (n + n0)(k + 1/2))`, N0 = 2 * lines,
/// n0 = (N0/2 + 1)/2. `spec.len()` must be a power of two.
pub(super) fn imdct(spec: &[f64]) -> Vec<f64> {
    let lines = spec.len();
    let n0 = 2 * lines;
    let shift = (n0 as f64 / 2.0 + 1.0) / 2.0;
    let mut re = vec![0.0; n0];
    let mut im = vec![0.0; n0];
    for (k, &value) in spec.iter().enumerate() {
        let phase = 2.0 * PI * shift * (k as f64 + 0.5) / n0 as f64;
        re[k] = value * phase.cos();
        im[k] = value * phase.sin();
    }
    fft(&mut re, &mut im, true);
    (0..n0)
        .map(|n| {
            let phase = PI * n as f64 / n0 as f64;
            (re[n] * phase.cos() - im[n] * phase.sin()) * 2.0 / n0 as f64
        })
        .collect()
}

/// The sine window of `n` samples.
pub(super) fn sine(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| (PI / n as f64 * (i as f64 + 0.5)).sin())
        .collect()
}

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..60 {
        term *= (x / (2.0 * k as f64)).powi(2);
        sum += term;
    }
    sum
}

/// The Kaiser-Bessel-derived window of `n` samples (`alpha` 4 for long, 6 for short blocks).
pub(super) fn kbd(n: usize, alpha: f64) -> Vec<f64> {
    let half = n / 2;
    let kaiser: Vec<f64> = (0..=half)
        .map(|i| {
            let r = 2.0 * i as f64 / half as f64 - 1.0;
            bessel_i0(PI * alpha * (1.0 - r * r).max(0.0).sqrt())
        })
        .collect();
    let total: f64 = kaiser.iter().sum();
    let mut running = 0.0;
    let mut window = Vec::with_capacity(n);
    for value in kaiser.iter().take(half) {
        running += value;
        window.push((running / total).sqrt());
    }
    let first = window.clone();
    window.extend(first.iter().rev());
    window
}

/// A window of `n` samples in the shape a frame names: 0 sine, 1 Kaiser-Bessel-derived.
pub(super) fn window(kind: u8, n: usize) -> Vec<f64> {
    if kind == 0 {
        sine(n)
    } else {
        kbd(n, if n > 256 { 4.0 } else { 6.0 })
    }
}

pub(super) const ONLY_LONG: u8 = 0;
pub(super) const LONG_START: u8 = 1;
pub(super) const EIGHT_SHORT: u8 = 2;
pub(super) const LONG_STOP: u8 = 3;

/// One frame's 2048 windowed samples from its 1024 lines (long kinds) or eight blocks of 128
/// lines one after another (`EIGHT_SHORT`). The left half of a window has the previous frame's
/// shape, the right half the current one.
pub(super) fn frame_samples(spec: &[f64], sequence: u8, previous: u8, current: u8) -> Vec<f64> {
    let mut out = vec![0.0; 2048];
    if sequence == EIGHT_SHORT {
        let first = window(previous, 256);
        let later = window(current, 256);
        let right = window(current, 256);
        for block in 0..8 {
            let Some(lines) = spec.get(block * 128..block * 128 + 128) else {
                break;
            };
            let samples = imdct(lines);
            let left = if block == 0 { &first } else { &later };
            for (i, sample) in samples.iter().enumerate() {
                let w = if i < 128 {
                    left.get(i)
                } else {
                    right.get(i)
                };
                if let (Some(slot), Some(w)) = (out.get_mut(448 + block * 128 + i), w) {
                    *slot += sample * w;
                }
            }
        }
        return out;
    }
    let long_left = window(previous, 2048);
    let long_right = window(current, 2048);
    let short_left = window(previous, 256);
    let short_right = window(current, 256);
    let samples = imdct(spec);
    for (i, sample) in samples.iter().enumerate() {
        let w = if i < 1024 {
            if sequence == LONG_STOP {
                match i {
                    0..=447 => 0.0,
                    448..=575 => short_left.get(i - 448).copied().unwrap_or(0.0),
                    _ => 1.0,
                }
            } else {
                long_left.get(i).copied().unwrap_or(0.0)
            }
        } else if sequence == LONG_START {
            match i {
                1024..=1471 => 1.0,
                1472..=1599 => short_right.get(128 + i - 1472).copied().unwrap_or(0.0),
                _ => 0.0,
            }
        } else {
            long_right.get(i).copied().unwrap_or(0.0)
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = sample * w;
        }
    }
    out
}

/// Overlap-add: the first half of `frame` joins the half held back from the last frame, giving
/// 1024 finished samples; the second half is kept for the next.
pub(super) fn overlap(frame: &[f64], held: &mut Vec<f64>) -> Vec<f64> {
    held.resize(1024, 0.0);
    let out = (0..1024)
        .map(|i| frame.get(i).copied().unwrap_or(0.0) + held.get(i).copied().unwrap_or(0.0))
        .collect();
    for (i, slot) in held.iter_mut().enumerate() {
        *slot = frame.get(1024 + i).copied().unwrap_or(0.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(spec: &[f64]) -> Vec<f64> {
        let n0 = 2 * spec.len();
        let shift = (n0 as f64 / 2.0 + 1.0) / 2.0;
        (0..n0)
            .map(|n| {
                spec.iter()
                    .enumerate()
                    .map(|(k, v)| {
                        v * (2.0 * PI / n0 as f64 * (n as f64 + shift) * (k as f64 + 0.5)).cos()
                    })
                    .sum::<f64>()
                    * 2.0
                    / n0 as f64
            })
            .collect()
    }

    #[test]
    fn the_fft_imdct_equals_the_defining_sum() {
        for lines in [8usize, 16, 128] {
            let spec: Vec<f64> = (0..lines)
                .map(|i| ((i * 7 % 13) as f64 - 6.0) / 3.0)
                .collect();
            let fast = imdct(&spec);
            let slow = direct(&spec);
            assert_eq!(fast.len(), slow.len());
            for (a, b) in fast.iter().zip(&slow) {
                assert!((a - b).abs() < 1e-9, "{a} vs {b}");
            }
        }
    }

    #[test]
    fn windows_satisfy_the_princen_bradley_condition() {
        for w in [sine(256), kbd(256, 6.0), sine(2048), kbd(2048, 4.0)] {
            let n = w.len();
            for i in 0..n / 2 {
                let sum = w[i].powi(2) + w[i + n / 2].powi(2);
                assert!((sum - 1.0).abs() < 1e-9, "{sum}");
            }
        }
    }

    #[test]
    fn silence_stays_silent_across_every_window_sequence() {
        let mut held = Vec::new();
        for sequence in [ONLY_LONG, LONG_START, EIGHT_SHORT, LONG_STOP] {
            let frame = frame_samples(&vec![0.0; 1024], sequence, 0, 1);
            assert_eq!(frame.len(), 2048);
            let out = overlap(&frame, &mut held);
            assert_eq!(out.len(), 1024);
            assert!(out.iter().all(|v| *v == 0.0));
        }
    }

    #[test]
    fn the_long_start_window_ends_in_zeros() {
        let frame = frame_samples(&vec![1.0; 1024], LONG_START, 0, 0);
        assert_eq!(frame.len(), 2048);
        assert!(frame[1600..].iter().all(|v| *v == 0.0));
    }
}
