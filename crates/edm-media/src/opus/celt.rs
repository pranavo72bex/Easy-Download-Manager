// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! CELT, Opus's transform layer (RFC 6716, section 4.3), EDM's own decoder of it: what a
//! music or high-rate stream (YouTube's sound at its best) is coded with.
//!
//! A frame (2.5 to 20 ms) is read in the standard's order: a silence flag, the pitch
//! post-filter's settings, whether it is transient (short blocks), the band energies
//! (`energy`), the time-frequency changes and the spreading, the bits' allocation (`alloc`),
//! the fine energies, each band's shape (`bands`, `pvq`), the anti-collapse noise and the last
//! fine bits. The spectrum is then scaled by the energies and made into sound (`synth`): the
//! inverse MDCT with overlap-add, the post-filter and the de-emphasis. The arithmetic of every
//! symbol read is the standard's exact integer arithmetic, so a frame is read as it was
//! written; the sound itself is made in 32-bit floats.

mod alloc;
mod bands;
mod energy;
mod pvq;
mod synth;
mod tables;

use super::range::RangeDecoder;
use alloc::BITRES;
use bands::BandFrame;
use energy::History;
use synth::{Filter, OVERLAP};
use tables::{BANDS, EBANDS, E_MEANS, SPREAD_ICDF, TAPSET_ICDF, TRIM_ICDF};

/// Lines in a 2.5 ms block (and samples at 48 kHz).
const SHORT: usize = 120;
/// The history kept per channel for the post-filter and the overlap.
const BUFFER: usize = 2048;

/// The bands and frame size a frame covers, and its channels.
struct Span {
    start: usize,
    end: usize,
    channels: usize,
    lm: usize,
}

/// What a frame is, from its packet's table of contents.
pub(crate) struct Frame {
    /// Channels coded in the frame (1 or 2).
    pub(crate) channels: usize,
    /// The frame's size: 120 samples times `2^lm`, `lm` 0 to 3.
    pub(crate) lm: usize,
    /// The first band coded (17 in a hybrid frame, else 0).
    pub(crate) start: usize,
    /// The band after the last coded, by the packet's bandwidth.
    pub(crate) end: usize,
}

/// What a frame's first symbols say.
struct Header {
    silence: bool,
    post: Filter,
    transient: bool,
    intra: bool,
}

/// A CELT decoder's state between frames, for `channels` channels of output.
pub(crate) struct Celt {
    channels: usize,
    /// Each channel's sound: the history, the frame being made and the overlap after it.
    mem: Vec<Vec<f32>>,
    /// The band energies of the frame before, both channels.
    energy: [f32; 2 * BANDS],
    /// The frame before's energies, and the one before that, for the anti-collapse.
    log1: [f32; 2 * BANDS],
    log2: [f32; 2 * BANDS],
    /// The noise generator's seed: the last frame's final range.
    rng: u32,
    post: Filter,
    post_old: Filter,
    preemph: [f32; 2],
}

impl Celt {
    /// A decoder giving `channels` (1 or 2) channels of sound.
    pub(crate) fn new(channels: usize) -> Celt {
        let channels = channels.clamp(1, 2);
        Celt {
            channels,
            mem: vec![vec![0.0; BUFFER + OVERLAP]; channels],
            energy: [0.0; 2 * BANDS],
            log1: [-28.0; 2 * BANDS],
            log2: [-28.0; 2 * BANDS],
            rng: 0,
            post: Filter::default(),
            post_old: Filter::default(),
            preemph: [0.0; 2],
        }
    }

    /// Decodes one frame from `dec` into `out`: `120 << lm` samples a channel, interleaved,
    /// at the scale of 16-bit samples.
    pub(crate) fn decode(&mut self, dec: &mut RangeDecoder<'_>, frame: &Frame, out: &mut [f32]) {
        let span = Span {
            start: frame.start.min(BANDS - 1),
            end: frame.end.clamp(frame.start.min(BANDS - 1) + 1, BANDS),
            channels: frame.channels.clamp(1, 2),
            lm: frame.lm.min(3),
        };
        let n = SHORT << span.lm;
        if span.channels == 1 {
            let (first, second) = self.energy.split_at_mut(BANDS);
            for (a, b) in first.iter_mut().zip(second.iter()) {
                *a = a.max(*b);
            }
        }
        let total_bits = dec.total_bits();
        let head = header(dec, &span);
        energy::coarse(dec, &span, &mut self.energy, head.intra);
        let tf_res = energy::tf_decode(dec, &span, head.transient);
        let spread = if dec.tell() + 4 <= total_bits {
            dec.icdf(&SPREAD_ICDF, 5) as u32
        } else {
            pvq::SPREAD_NORMAL
        };
        let (alloc, anti_rsv) = allocation(dec, &span, head.transient);
        energy::fine(dec, &span, &mut self.energy, &alloc.fine);
        let mut x = vec![0.0f32; span.channels * n];
        let mut collapse = [0u8; 2 * BANDS];
        let band_frame = BandFrame {
            span: &span,
            alloc: &alloc,
            tf_res: &tf_res,
            spread,
            transient: head.transient,
            total_bits: (total_bits << BITRES) - anti_rsv,
        };
        {
            let (left, right) = x.split_at_mut(n);
            let right = (span.channels == 2).then_some(right);
            self.rng = bands::decode_bands(dec, &band_frame, left, right, &mut collapse, self.rng);
        }
        let anti_collapse = anti_rsv > 0 && dec.bits(1) == 1;
        let left = total_bits - dec.tell();
        let (fine, priority) = (&alloc.fine, &alloc.fine_priority);
        energy::finalise(dec, &span, &mut self.energy, fine, priority, left);
        if anti_collapse {
            let history = History {
                now: &self.energy,
                prev1: &self.log1,
                prev2: &self.log2,
            };
            energy::anti_collapse(&span, &mut x, &collapse, &history, &alloc.pulses, self.rng);
        }
        if head.silence {
            for e in self.energy.iter_mut().take(span.channels * BANDS) {
                *e = -28.0;
            }
        }
        self.synthesize(&x, &span, &head, out);
        self.keep_energies(&span, head.transient);
        self.rng = dec.range();
    }

    /// The frame's spectrum `x` made into sound in `out`, post-filtered and de-emphasized.
    fn synthesize(&mut self, x: &[f32], span: &Span, head: &Header, out: &mut [f32]) {
        let n = SHORT << span.lm;
        let cc = self.channels;
        for mem in self.mem.iter_mut() {
            mem.copy_within(n..BUFFER + OVERLAP, 0);
        }
        let mut freq: Vec<Vec<f32>> = (0..span.channels)
            .map(|c| {
                let shape = x.get(c * n..(c + 1) * n).unwrap_or(&[]);
                let energies = &self.energy[c * BANDS..(c + 1) * BANDS];
                denormalise(shape, energies, span, head.silence)
            })
            .collect();
        if cc == 2 && span.channels == 1 {
            let copy = freq[0].clone();
            freq.push(copy);
        } else if cc == 1 && span.channels == 2 {
            let right = freq.pop().unwrap_or_default();
            for (l, r) in freq[0].iter_mut().zip(right) {
                *l = 0.5 * *l + 0.5 * r;
            }
        }
        let blocks = if head.transient { 1 << span.lm } else { 1 };
        for (mem, f) in self.mem.iter_mut().zip(&freq) {
            synth::inverse_mdct(f, blocks, &mut mem[BUFFER - n..]);
        }
        for mem in self.mem.iter_mut() {
            let at = BUFFER - n;
            synth::comb_filter(mem, at, SHORT, self.post_old, self.post, OVERLAP);
            if span.lm != 0 {
                synth::comb_filter(mem, at + SHORT, n - SHORT, self.post, head.post, OVERLAP);
            }
        }
        self.post_old = self.post;
        self.post = head.post;
        if span.lm != 0 {
            self.post_old = self.post;
        }
        for (c, mem) in self.mem.iter().enumerate() {
            let Some(channel_out) = out.get_mut(c..) else {
                break;
            };
            synth::deemphasis(
                &mem[BUFFER - n..BUFFER],
                &mut self.preemph[c],
                channel_out,
                cc,
            );
        }
    }

    /// The energies kept for the next frames, and those outside the coded bands cleared.
    fn keep_energies(&mut self, span: &Span, transient: bool) {
        if span.channels == 1 {
            let (first, second) = self.energy.split_at_mut(BANDS);
            second.copy_from_slice(first);
        }
        if transient {
            for (l, e) in self.log1.iter_mut().zip(self.energy.iter()) {
                *l = l.min(*e);
            }
        } else {
            self.log2 = self.log1;
            self.log1 = self.energy;
        }
        for c in 0..2 {
            for i in (0..span.start).chain(span.end..BANDS) {
                let k = c * BANDS + i;
                self.energy[k] = 0.0;
                self.log1[k] = -28.0;
                self.log2[k] = -28.0;
            }
        }
    }
}

/// The frame's first symbols: silence, the post-filter, transient, intra (RFC 6716, 4.3.1
/// and table 56).
fn header(dec: &mut RangeDecoder<'_>, span: &Span) -> Header {
    let total_bits = dec.total_bits();
    let mut tell = dec.tell();
    let silence = tell >= total_bits || (tell == 1 && dec.bit_logp(15));
    if silence {
        dec.skip_to(total_bits);
        tell = total_bits;
    }
    let mut post = Filter::default();
    if span.start == 0 && tell + 16 <= total_bits {
        if dec.bit_logp(1) {
            let octave = dec.uint(6);
            post.period = ((16u32 << octave) + dec.bits(4 + octave)).saturating_sub(1) as usize;
            let qg = dec.bits(3);
            if dec.tell() + 2 <= total_bits {
                post.tapset = dec.icdf(&TAPSET_ICDF, 2);
            }
            post.gain = 0.09375 * (qg + 1) as f32;
        }
        tell = dec.tell();
    }
    let mut transient = false;
    if span.lm > 0 && tell + 3 <= total_bits {
        transient = dec.bit_logp(3);
        tell = dec.tell();
    }
    let intra = tell + 3 <= total_bits && dec.bit_logp(3);
    Header {
        silence,
        post,
        transient,
        intra,
    }
}

/// The band boosts and the trim read, and the bits shared (RFC 6716, 4.3.3); with the bit
/// kept for the anti-collapse flag (eighths).
fn allocation(
    dec: &mut RangeDecoder<'_>,
    span: &Span,
    transient: bool,
) -> (alloc::Allocation, i32) {
    let cap = alloc::caps(span);
    let mut offsets = [0i32; BANDS];
    let mut dynalloc_logp = 6;
    let mut total = dec.total_bits() << BITRES;
    let mut tell = dec.tell_frac();
    for i in span.start..span.end {
        let quanta = energy::boost_quanta(span, i);
        let mut logp = dynalloc_logp;
        let mut boost = 0;
        while tell + (logp << BITRES) < total && boost < cap[i] {
            let flag = dec.bit_logp(logp as u32);
            tell = dec.tell_frac();
            if !flag {
                break;
            }
            boost += quanta;
            total -= quanta;
            logp = 1;
        }
        offsets[i] = boost;
        if boost > 0 {
            dynalloc_logp = (dynalloc_logp - 1).max(2);
        }
    }
    let trim = if tell + (6 << BITRES) <= total {
        dec.icdf(&TRIM_ICDF, 7) as i32
    } else {
        5
    };
    let mut bits = (dec.total_bits() << BITRES) - dec.tell_frac() - 1;
    let lm = span.lm as i32;
    let anti_rsv = if transient && lm >= 2 && bits >= (lm + 2) << BITRES {
        1 << BITRES
    } else {
        0
    };
    bits -= anti_rsv;
    let alloc = alloc::allocate(dec, span, &offsets, &cap, trim, bits);
    (alloc, anti_rsv)
}

/// A channel's band shapes scaled by its energies: its spectrum (`denormalise_bands`).
fn denormalise(x: &[f32], energies: &[f32], span: &Span, silence: bool) -> Vec<f32> {
    let m = 1usize << span.lm;
    let mut freq = vec![0.0f32; SHORT * m];
    if silence {
        return freq;
    }
    for i in span.start..span.end {
        let gain = (energies[i] + E_MEANS[i]).min(32.0).exp2();
        let (from, to) = (m * EBANDS[i], m * EBANDS[i + 1]);
        if let (Some(f), Some(shape)) = (freq.get_mut(from..to), x.get(from..to)) {
            for (out, v) in f.iter_mut().zip(shape) {
                *out = v * gain;
            }
        }
    }
    freq
}

#[cfg(test)]
mod tests;
