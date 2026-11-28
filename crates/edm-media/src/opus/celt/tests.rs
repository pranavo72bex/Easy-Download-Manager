// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! CELT's pieces against known answers: the codebook cost table and the caps, the PVQ index
//! order, the split angle's exact cosine, the inverse MDCT against its definition, and a
//! silent frame.

use super::*;

/// The codebook cost table holds 392 bytes for CELT's mode, as the standard's reference
/// tables do; a band of one line costs 7 eighths whatever its pulses (a sign), and the rows
/// the other answers here come from.
#[test]
fn the_pulse_cost_table_is_the_standards() {
    assert_eq!(alloc::cache_size(), 392);
    let one = alloc::cost_row(-1, 8);
    assert_eq!(one.len(), 41);
    assert!(one[1..].iter().all(|b| *b == 7));
    assert_eq!(
        alloc::cost_row(0, 9)[..10],
        [40, 15, 23, 28, 31, 34, 36, 38, 39, 41]
    );
    assert_eq!(alloc::cost_row(3, 20), [4, 67, 127, 182, 234]);
    let bits: Vec<i32> = [0, 10, 60, 100, 200, 300]
        .iter()
        .map(|b| alloc::bits_to_pulses(20, 3, *b))
        .collect();
    assert_eq!(bits, [0, 0, 1, 2, 3, 4]);
    assert_eq!(alloc::pulses_to_bits(20, 3, 1), 68);
    assert_eq!(alloc::pulses_to_bits(20, 3, 4), 235);
    let wide = Span {
        start: 0,
        end: 21,
        channels: 2,
        lm: 3,
    };
    assert_eq!(alloc::caps(&wide)[..3], [1072, 1072, 1072]);
    let short = Span {
        channels: 1,
        lm: 0,
        ..wide
    };
    assert_eq!(alloc::caps(&short)[20], 555);
}

/// Codeword indices count the vectors with the first line largest and positive first, and
/// every index of a small codebook gives a different vector of the right number of pulses.
#[test]
fn pvq_indices_give_every_vector_once_in_the_standards_order() {
    let mut y = [0i32; 3];
    let size = pvq::pvq_size(3, 2);
    assert_eq!(size, 18);
    let order: Vec<[i32; 3]> = (0..size)
        .map(|i| {
            pvq::pulse_vector(i, 2, &mut y);
            y
        })
        .collect();
    assert_eq!(
        order[..5],
        [[2, 0, 0], [1, 1, 0], [1, 0, 1], [1, 0, -1], [1, -1, 0]]
    );
    assert_eq!(order[13], [-2, 0, 0]);
    for (n, k) in [(2usize, 5usize), (4, 3), (5, 4), (8, 2), (16, 1)] {
        let mut seen = std::collections::HashSet::new();
        let mut v = vec![0i32; n];
        for i in 0..pvq::pvq_size(n, k) {
            let energy = pvq::pulse_vector(i, k, &mut v);
            assert_eq!(
                v.iter().map(|p| p.unsigned_abs() as usize).sum::<usize>(),
                k
            );
            assert_eq!(energy, v.iter().map(|p| (p * p) as f32).sum::<f32>());
            assert!(seen.insert(v.clone()), "{v:?} twice for ({n}, {k})");
        }
    }
}

/// The split angle's cosine and log-tangent, exactly as the standard computes them.
#[test]
fn the_split_angle_is_exact() {
    assert_eq!(pvq::bitexact_cos(8192), 23171);
    assert_eq!(pvq::bitexact_cos(4096), 30274);
    assert_eq!(pvq::bitexact_cos(16383), 3);
    assert_eq!(pvq::bitexact_log2tan(23171, 23171), 0);
    assert_eq!(pvq::bitexact_log2tan(32767, 1000), 10305);
    assert_eq!(pvq::bitexact_log2tan(1000, 32767), -10305);
    for v in [0u32, 1, 2, 3, 4, 15, 16, 17, 1 << 20, u32::MAX] {
        let r = u64::from(pvq::isqrt(v));
        assert!(
            r * r <= u64::from(v) && (r + 1) * (r + 1) > u64::from(v),
            "{v}"
        );
    }
}

/// The inverse MDCT (through its FFT) gives the middle of the transform's definition,
/// `y[n] = sum X[k] cos(pi / N (n + 1/2 + N/2) (k + 1/2))`, for every block size.
#[test]
fn the_inverse_mdct_is_its_definition() {
    for n in [120usize, 240, 480, 960] {
        let coeffs: Vec<f32> = (0..n)
            .map(|k| (((k * 7919 + 13) % 1000) as f32 / 500.0 - 1.0) * 100.0)
            .collect();
        // The block alone, after a block of silence, with no window over its middle.
        let mut out = vec![0.0f32; n + OVERLAP];
        synth::inverse_mdct(&coeffs, 1, &mut out);
        let w = synth::window();
        for j in (OVERLAP..n + OVERLAP / 2).step_by(7) {
            let at = j as f64 - (OVERLAP / 2) as f64 + (n / 2) as f64;
            let want: f64 = coeffs
                .iter()
                .enumerate()
                .map(|(k, x)| {
                    let phase = std::f64::consts::PI / n as f64
                        * (at + 0.5 + n as f64 / 2.0)
                        * (k as f64 + 0.5);
                    f64::from(*x) * phase.cos()
                })
                .sum();
            let got = f64::from(out[j]);
            assert!(
                (got - want).abs() < 0.05 * (1.0 + want.abs()) + 1.0,
                "{n} {j}: {got} {want}"
            );
        }
        assert!(w.iter().all(|v| (0.0..=1.0).contains(v)));
        // Power complementary: w[i]^2 + w[overlap - 1 - i]^2 = 1.
        for i in 0..OVERLAP {
            let sum = w[i] * w[i] + w[OVERLAP - 1 - i] * w[OVERLAP - 1 - i];
            assert!((sum - 1.0).abs() < 1e-5);
        }
    }
}

/// A silent frame (its first symbol the silence flag) decodes to silence, and leaves the
/// state ready for the next: twenty of them, mono and stereo, at every frame size.
#[test]
fn a_silent_frame_is_silence() {
    for channels in [1usize, 2] {
        for lm in 0..4 {
            let mut celt = Celt::new(channels);
            let n = SHORT << lm;
            let mut out = vec![1.0f32; n * channels];
            for _ in 0..20 {
                let mut dec = RangeDecoder::new(&[0xFF, 0xFE]);
                let frame = Frame {
                    channels,
                    lm,
                    start: 0,
                    end: 21,
                };
                celt.decode(&mut dec, &frame, &mut out);
                assert!(out.iter().all(|v| v.abs() < 1e-3), "{channels} {lm}");
                assert!(dec.tell() <= 16);
            }
        }
    }
}

/// Frames of any bytes decode without a panic, and their sound stays finite.
#[test]
fn any_bytes_decode_without_a_panic() {
    let mut seed = 0x1234_5678u32;
    for len in [0usize, 1, 2, 3, 5, 8, 20, 60, 120, 400, 1275] {
        for lm in 0..4 {
            for channels in [1usize, 2] {
                let bytes: Vec<u8> = (0..len)
                    .map(|_| {
                        seed = pvq::lcg(seed);
                        (seed >> 24) as u8
                    })
                    .collect();
                let mut celt = Celt::new(2);
                let mut out = vec![0.0f32; (SHORT << lm) * 2];
                for end in [13, 17, 19, 21] {
                    let mut dec = RangeDecoder::new(&bytes);
                    let frame = Frame {
                        channels,
                        lm,
                        start: 0,
                        end,
                    };
                    celt.decode(&mut dec, &frame, &mut out);
                    assert!(out.iter().all(|v| v.is_finite()));
                }
            }
        }
    }
}
