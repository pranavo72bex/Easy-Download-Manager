// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A video's AAC sound read to samples on the computers whose system has no decoder that EDM
//! reads (Linux), so a video's sound can be saved as MP3 there too.
//!
//! EDM reads the M4A itself: the AudioSpecificConfig out of its `esds`, its sample tables, and
//! its edit list for where the sound starts and ends (the MP4 muxer's reader). Each AAC frame is
//! decoded by symphonia's AAC-LC decoder (`symphonia-codec-aac`, MPL-2.0, used unmodified as a
//! dependency; `docs/decisions.md`, 2026-11-06). Sound with SBR/PS (HE-AAC), more than two
//! channels or 960-sample frames is refused with the reason, before anything is decoded.

mod bits;
mod tables;

use crate::mp3::decode::Pcm;
use crate::mp4mux::{tracks_in_file, Track};
use crate::Error;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use symphonia_codec_aac::AacDecoder;
use symphonia_core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia_core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia_core::packet::PacketRef;
use symphonia_core::units::{Duration, Timestamp};

/// What an `esds` says about the sound.
#[derive(Debug, PartialEq, Eq)]
struct Config {
    rate_index: usize,
    rate: u32,
    channels: usize,
}

/// The AudioSpecificConfig of AAC-LC, 2 bytes or more.
fn parse_config(bytes: &[u8]) -> Result<Config, Error> {
    let mut bits = bits::Bits::new(bytes);
    let mut object = bits.bits(5)?;
    if object == 31 {
        object = 32 + bits.bits(6)?;
    }
    if object != 2 {
        return Err(format!("AAC object type {object} (only AAC-LC is converted to MP3)").into());
    }
    let rate_index = bits.bits(4)? as usize;
    let rate = if rate_index == 15 {
        bits.bits(24)?
    } else {
        *tables::RATES
            .get(rate_index)
            .ok_or("a sample rate AAC does not have")?
    };
    let channels = bits.bits(4)? as usize;
    if bits.flag()? {
        return Err("AAC with 960-sample frames is not supported".into());
    }
    if channels == 0 || channels > 2 {
        return Err("only mono and stereo sound is converted to MP3".into());
    }
    Ok(Config {
        rate_index,
        rate,
        channels,
    })
}

/// Walks the descriptors of an `esds` payload for the decoder-specific info.
fn descriptor_info(mut data: &[u8]) -> Option<&[u8]> {
    // The full box's version and flags.
    data = data.get(4..)?;
    loop {
        let tag = *data.first()?;
        let mut at = 1;
        let mut length = 0usize;
        for _ in 0..4 {
            let byte = *data.get(at)?;
            at += 1;
            length = (length << 7) | usize::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                break;
            }
        }
        let body = data.get(at..at.checked_add(length)?)?;
        match tag {
            0x03 => data = body.get(3..)?,
            0x04 => data = body.get(13..)?,
            0x05 => return Some(body),
            _ => data = data.get(at + length..)?,
        }
    }
}

/// The audio sample entry's `esds` payload and the entry's codec name in a whole `stsd` box.
fn sample_entry(stsd: &[u8]) -> Option<([u8; 4], &[u8])> {
    let entry = stsd.get(16..)?;
    let name: [u8; 4] = entry.get(4..8)?.try_into().ok()?;
    // 8 bytes of box header, 28 of audio sample entry, then child boxes.
    let mut children = entry.get(36..)?;
    while children.len() >= 8 {
        let size = u32::from_be_bytes(children.get(0..4)?.try_into().ok()?) as usize;
        let kind = children.get(4..8)?;
        if size < 8 || size > children.len() {
            return None;
        }
        if kind == b"esds" {
            return Some((name, children.get(8..size)?));
        }
        children = children.get(size..)?;
    }
    Some((name, &[]))
}

fn audio_track(path: &Path) -> Result<(Track, Config, Vec<u8>), Error> {
    let tracks = tracks_in_file(path)?;
    let track = tracks
        .into_iter()
        .find(|t| t.hdlr.get(16..20) == Some(b"soun"))
        .ok_or("the file has no sound track")?;
    let (name, esds) =
        sample_entry(&track.stsd).ok_or("the sound track's description is unreadable")?;
    if &name != b"mp4a" {
        return Err(format!("sound coded as {}, not AAC", String::from_utf8_lossy(&name)).into());
    }
    let info = descriptor_info(esds).ok_or("an AAC track with no decoder configuration")?;
    let config = parse_config(info)?;
    let info = info.to_vec();
    Ok((track, config, info))
}

/// The largest AAC frame read: far above the 6,144 bits a channel of one may hold.
const MOST_FRAME: u32 = 1 << 20;

/// `path`'s sound (an M4A, or an MP4's sound track) as 16-bit samples.
pub fn read(path: &Path) -> Result<Pcm, Error> {
    let (track, config, info) = audio_track(path)?;
    let mut params = AudioCodecParameters::new();
    params
        .for_codec(CODEC_ID_AAC)
        .with_extra_data(info.into_boxed_slice());
    let mut decoder = AacDecoder::try_new(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("the AAC sound could not be decoded: {e}"))?;
    let mut file =
        File::open(path).map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    let mut samples: Vec<i16> = Vec::new();
    let mut frame: Vec<i16> = Vec::new();
    let mut data = Vec::new();
    for (n, sample) in track.samples.iter().enumerate() {
        // An AAC frame is a few kilobytes at most; a size the file says of 4 GiB was reserved
        // before its read failed (gaps rows H10, H35).
        if sample.size > MOST_FRAME {
            return Err(Error::from(format!(
                "the AAC sound's frame {n} says it is {} bytes",
                sample.size
            )));
        }
        data.resize(sample.size as usize, 0);
        file.seek(SeekFrom::Start(sample.offset))
            .and_then(|_| file.read_exact(&mut data))
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        let at = Timestamp::new(i64::try_from(n).unwrap_or(i64::MAX).saturating_mul(1024));
        let packet = PacketRef::new(0, at, Duration::new(1024), &data);
        let decoded = decoder
            .decode_ref(&packet)
            .map_err(|e| format!("the AAC sound could not be decoded at frame {n}: {e}"))?;
        decoded.copy_to_vec_interleaved(&mut frame);
        samples.extend_from_slice(&frame);
    }
    // The edit list drops the encoder's priming samples and any padding at the end.
    let per_frame = config.channels;
    if let Some(edit) = track.edits.first() {
        // Sizes the file's edit list gives, held to the line's end rather than overflowing
        // (gaps rows H10, H35): past the samples there are, they cut nothing more.
        let skip = usize::try_from(edit.media_time.max(0))
            .unwrap_or(usize::MAX)
            .saturating_mul(per_frame);
        let keep = if edit.duration > 0 && track.edits_timescale > 0 {
            usize::try_from(
                edit.duration.saturating_mul(u64::from(config.rate))
                    / u64::from(track.edits_timescale),
            )
            .unwrap_or(usize::MAX)
            .saturating_mul(per_frame)
        } else {
            usize::MAX
        };
        let start = skip.min(samples.len());
        let end = start.saturating_add(keep).min(samples.len());
        samples = samples.get(start..end).unwrap_or(&[]).to_vec();
    }
    let rate = mp3_rate(config.rate);
    if rate != config.rate {
        samples = resample(&samples, config.channels, config.rate, rate);
    }
    Ok(Pcm {
        samples,
        channels: config.channels,
        rate,
    })
}

/// The rate MP3 has that a sound of `native` Hz is made at: its own when MP3 has it, else
/// 44.1 kHz for the 11025 Hz family and 48 kHz for the rest, so the change is by a whole
/// ratio where one exists (22050 doubles).
fn mp3_rate(native: u32) -> u32 {
    match native {
        32000 | 44100 | 48000 => native,
        r if r % 11025 == 0 || r == 7350 => 44100,
        _ => 48000,
    }
}

/// Half the width of the resampler's windowed-sinc filter, in input samples at its cut-off.
const TAPS: f64 = 16.0;

/// Interleaved `samples` from `from` Hz to `to` Hz by a Lanczos-windowed sinc, cut off at the
/// lower of the two Nyquist frequencies so a lower rate gets no aliases.
fn resample(samples: &[i16], channels: usize, from: u32, to: u32) -> Vec<i16> {
    let frames = samples.len() / channels.max(1);
    if frames == 0 || from == 0 || to == 0 {
        return Vec::new();
    }
    let step = f64::from(from) / f64::from(to);
    let cutoff = (1.0 / step).min(1.0);
    let reach = (TAPS / cutoff).ceil() as i64;
    let out_frames = (frames as u64 * u64::from(to) / u64::from(from)) as usize;
    let mut out = Vec::with_capacity(out_frames * channels);
    let mut weights = Vec::with_capacity(2 * reach as usize + 1);
    for i in 0..out_frames {
        let t = i as f64 * step;
        let centre = t.floor() as i64;
        let first = (centre - reach + 1).max(0);
        let last = (centre + reach).min(frames as i64 - 1);
        weights.clear();
        let mut total = 0.0;
        for k in first..=last {
            let x = (t - k as f64) * cutoff;
            let w = sinc(x) * sinc(x / TAPS);
            weights.push(w);
            total += w;
        }
        for c in 0..channels {
            let mut sum = 0.0;
            for (j, w) in weights.iter().enumerate() {
                let k = first as usize + j;
                sum += w * f64::from(samples[k * channels + c]);
            }
            // Normalised by the weights' sum, so a constant stays the same constant.
            let value = if total.abs() > 1e-9 { sum / total } else { 0.0 };
            out.push(value.round().clamp(-32768.0, 32767.0) as i16);
        }
    }
    out
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

#[cfg(test)]
mod tests;
