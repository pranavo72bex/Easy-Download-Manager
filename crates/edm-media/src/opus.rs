// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An Opus decoder of EDM's own (RFC 6716, with RFC 8251's corrections), so a video's sound
//! in a WebM (YouTube's formats 249 to 251) is made into an MP3 on a computer whose system has
//! no Opus decoder EDM calls: macOS (AudioToolbox reads no WebM) and Linux (gaps row Y25).
//! Windows uses Media Foundation's where it has one, and this where it does not.
//!
//! A packet's first byte names its mode, bandwidth, frame size and channels; its frames are
//! cut out by one of four codes (`packet`), and each is read by the range decoder (`range`)
//! and decoded by CELT (`celt`), Opus's transform coder. SILK, the speech coder that Opus's
//! SILK and hybrid modes use, is not written yet: a packet in either mode stops the decode
//! with that said in words.
//!
//! The decoder is the standard's own algorithm in EDM's code, written from the RFC's text and
//! tables; nothing of another decoder is in it. Checked against the RFC's test vectors: every
//! CELT packet in them is read to the same final range as the encoder's.

mod celt;
mod packet;
mod range;

use std::path::Path;

use crate::mp3::decode::Pcm;
use crate::Error;
use celt::{Celt, Frame};
use packet::Mode;
use range::RangeDecoder;

/// Why a packet in SILK or hybrid mode is not decoded.
pub(crate) const NO_SILK: &str = "this Opus track uses SILK, which EDM cannot decode yet";

/// An Opus decoder for one stream of one or two channels at 48 kHz.
pub(crate) struct Decoder {
    channels: usize,
    celt: Celt,
    /// The last packet's length in samples, for a packet that is missing.
    last_samples: usize,
    /// The range decoder's final state after the last frame, which an encoder reports too.
    #[cfg(test)]
    range: u32,
}

impl Decoder {
    /// A decoder giving `channels` (1 or 2) channels.
    pub(crate) fn new(channels: usize) -> Decoder {
        let channels = channels.clamp(1, 2);
        Decoder {
            channels,
            celt: Celt::new(channels),
            last_samples: 960,
            #[cfg(test)]
            range: 0,
        }
    }

    /// One packet's sound appended to `out`, interleaved, at the scale of 16-bit samples. An
    /// empty packet (one that was lost) is the last packet's length of the sound fading out.
    pub(crate) fn decode(&mut self, packet: &[u8], out: &mut Vec<f32>) -> Result<(), Error> {
        let Some(&first) = packet.first() else {
            let mut left = self.last_samples;
            while left > 0 {
                let lm = (0..4usize)
                    .rev()
                    .find(|&lm| (120 << lm) <= left)
                    .unwrap_or(0);
                self.frame(&[], lm, 21, false, out);
                left = left.saturating_sub(120 << lm);
            }
            return Ok(());
        };
        let toc = packet::toc(first);
        if toc.mode != Mode::Celt {
            return Err(NO_SILK.into());
        }
        let lm = match toc.frame_size {
            120 => 0,
            240 => 1,
            480 => 2,
            _ => 3,
        };
        let frames = packet::frames(packet)?;
        for frame in frames.iter().copied() {
            // A frame of one byte or none carries nothing: concealed as silence.
            let data = if frame.len() <= 1 { &[][..] } else { frame };
            self.frame(data, lm, toc.end_band, toc.stereo, out);
        }
        self.last_samples = toc.frame_size * frames.len();
        Ok(())
    }

    /// One CELT frame of `data` appended to `out`.
    fn frame(&mut self, data: &[u8], lm: usize, end: usize, stereo: bool, out: &mut Vec<f32>) {
        let at = out.len();
        out.resize(at + (120 << lm) * self.channels, 0.0);
        let mut dec = RangeDecoder::new(data);
        let frame = Frame {
            channels: if stereo { 2 } else { 1 },
            lm,
            start: 0,
            end,
        };
        self.celt.decode(&mut dec, &frame, &mut out[at..]);
        #[cfg(test)]
        {
            self.range = if data.is_empty() { 0 } else { dec.range() };
        }
    }
}

/// Whether every packet of the WebM at `path`'s sound is one this decoder reads (CELT, or
/// none): a track that uses SILK is not, and a download of one is kept as it came rather than
/// failing on its MP3 (`mp3::takes`).
pub(crate) fn decodable(path: &Path) -> bool {
    crate::mkvmux::sound_track(path).is_ok_and(|track| {
        track.codec == "A_OPUS"
            && track.packets.iter().all(|p| {
                p.first()
                    .is_none_or(|&toc| packet::toc(toc).mode == Mode::Celt)
            })
    })
}

/// The Opus sound of the WebM at `path` as 16-bit samples at 48 kHz: its packets decoded in
/// order, the encoder's delay (the header's pre-skip) taken off the start and its padding off
/// the end, the header's gain applied.
pub(crate) fn read(path: &Path) -> Result<Pcm, Error> {
    let track = crate::mkvmux::sound_track(path)?;
    if track.codec != "A_OPUS" {
        return Err(format!(
            "an MP3 is made from AAC or Opus sound, not {}",
            track.codec.trim_start_matches("A_")
        )
        .into());
    }
    let head = packet::head(&track.private)?;
    let mut decoder = Decoder::new(head.channels);
    let mut sound = Vec::new();
    let mut samples: Vec<i16> = Vec::new();
    for packet in &track.packets {
        sound.clear();
        decoder.decode(packet, &mut sound)?;
        samples.extend(
            sound
                .iter()
                .map(|v| (v * head.gain).round().clamp(-32768.0, 32767.0) as i16),
        );
    }
    let channels = head.channels;
    let skip = (head.pre_skip * channels).min(samples.len());
    samples.drain(..skip);
    if track.padding_ns > 0 {
        let padding_ns = u64::try_from(track.padding_ns).unwrap_or(0);
        let padding = padding_ns.saturating_mul(48_000) / 1_000_000_000;
        let padding = usize::try_from(padding).unwrap_or(usize::MAX);
        let keep = samples
            .len()
            .saturating_sub(padding.saturating_mul(channels));
        samples.truncate(keep);
    }
    Ok(Pcm {
        samples,
        channels,
        rate: 48_000,
    })
}

#[cfg(test)]
mod tests;
