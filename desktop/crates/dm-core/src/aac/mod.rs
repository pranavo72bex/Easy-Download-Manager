// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A video's AAC sound read to samples on the computers whose system has no decoder that dm
//! reads (Linux), so a video's sound can be saved as MP3 there too.
//!
//! dm reads the M4A itself: the AudioSpecificConfig out of its `esds`, its sample tables, and
//! its edit list for where the sound starts and ends (the MP4 muxer's reader). Each AAC frame is
//! decoded by symphonia's AAC-LC decoder (`symphonia-codec-aac`, MPL-2.0, used unmodified as a
//! dependency; `docs/decisions.md`, 2026-11-06). Sound with SBR/PS (HE-AAC), more than two
//! channels or 960-sample frames is refused with the reason, before anything is decoded.

mod bits;
mod tables;

use crate::mp3::decode::Pcm;
use crate::mp4mux::{tracks_in_file, Track};
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
fn parse_config(bytes: &[u8]) -> Result<Config, String> {
    let mut bits = bits::Bits::new(bytes);
    let mut object = bits.bits(5)?;
    if object == 31 {
        object = 32 + bits.bits(6)?;
    }
    if object != 2 {
        return Err(format!(
            "AAC object type {object} (only AAC-LC is converted to MP3)"
        ));
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

fn audio_track(path: &Path) -> Result<(Track, Config, Vec<u8>), String> {
    let tracks = tracks_in_file(path)?;
    let track = tracks
        .into_iter()
        .find(|t| t.hdlr.get(16..20) == Some(b"soun"))
        .ok_or("the file has no sound track")?;
    let (name, esds) =
        sample_entry(&track.stsd).ok_or("the sound track's description is unreadable")?;
    if &name != b"mp4a" {
        return Err(format!(
            "sound coded as {}, not AAC",
            String::from_utf8_lossy(&name)
        ));
    }
    let info = descriptor_info(esds).ok_or("an AAC track with no decoder configuration")?;
    let config = parse_config(info)?;
    let info = info.to_vec();
    Ok((track, config, info))
}

/// `path`'s sound (an M4A, or an MP4's sound track) as 16-bit samples.
pub fn read(path: &Path) -> Result<Pcm, String> {
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
        let skip = usize::try_from(edit.media_time.max(0)).unwrap_or(0) * per_frame;
        let keep = if edit.duration > 0 && track.edits_timescale > 0 {
            (edit.duration.saturating_mul(u64::from(config.rate))
                / u64::from(track.edits_timescale)) as usize
                * per_frame
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
mod tests {
    use super::*;

    #[test]
    fn a_stereo_lc_config_at_44100_is_understood() {
        // 00010 0100 0010 000: object 2, rate index 4, 2 channels, 1024-frame, no core, no ext.
        let config = parse_config(&[0x12, 0x10]).unwrap();
        assert_eq!(
            config,
            Config {
                rate_index: 4,
                rate: 44100,
                channels: 2
            }
        );
    }

    #[test]
    fn other_object_types_and_channel_counts_are_refused_with_a_reason() {
        // HE-AAC (object 5), 6 channels (config 6).
        assert!(parse_config(&[0x2b, 0x11])
            .unwrap_err()
            .contains("only AAC-LC"));
        assert!(parse_config(&[0x11, 0xb0])
            .unwrap_err()
            .contains("mono and stereo"));
        assert!(parse_config(&[0x12]).is_err());
    }

    #[test]
    fn the_decoder_specific_info_is_found_in_an_esds() {
        let esds: Vec<u8> = [
            &[0u8, 0, 0, 0][..],                // version and flags
            &[0x03, 0x16, 0, 1, 0],             // ES_Descr, id 1, no flags
            &[0x04, 0x11, 0x40, 0x15, 0, 0, 0], // DecoderConfig: AAC, audio stream
            &[0, 0, 0, 0, 0, 0, 0, 0],          // max and average bit rate
            &[0x05, 0x02, 0x12, 0x10],          // DecSpecificInfo
        ]
        .concat();
        assert_eq!(descriptor_info(&esds), Some(&[0x12u8, 0x10][..]));
        assert_eq!(descriptor_info(&esds[..9]), None);
    }

    #[test]
    fn a_file_that_is_not_an_mp4_is_refused_not_decoded() {
        let dir = std::env::temp_dir().join(format!("dm-aac-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("noise.m4a");
        std::fs::write(&path, b"not an mp4 at all").unwrap();
        assert!(read(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_fixture_m4a_is_decoded_to_its_length_with_sound_in_it() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/sound.f140.m4a");
        let (track, config, _) = audio_track(&fixture).unwrap();
        let pcm = read(&fixture).unwrap();
        assert_eq!(pcm.channels, config.channels);
        // 22050 Hz, a rate MP3 does not have, comes out at twice it.
        assert_eq!((config.rate, pcm.rate), (22050, 44100));
        // The edit list's length, give or take a frame, and not silence.
        let frames = pcm.samples.len() / pcm.channels / 2;
        assert!(
            frames + 1024 >= (track.samples.len() - 1) * 1024 - 2112,
            "{frames}"
        );
        let loud = pcm
            .samples
            .iter()
            .filter(|s| s.unsigned_abs() > 1000)
            .count();
        assert!(
            loud > frames / 100,
            "{loud} of {frames} samples are not near silence"
        );
    }

    #[test]
    fn a_tone_keeps_its_pitch_and_length_through_the_resampler() {
        let tone = |rate: u32, n: usize| -> Vec<i16> {
            (0..n)
                .flat_map(|i| {
                    let v =
                        (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / f64::from(rate)).sin();
                    [(v * 12000.0) as i16, (v * -6000.0) as i16]
                })
                .collect()
        };
        for (from, to) in [
            (22050, 44100),
            (24000, 48000),
            (16000, 48000),
            (96000, 48000),
            (11025, 44100),
        ] {
            let made = resample(&tone(from, from as usize), 2, from, to);
            let want = tone(to, to as usize);
            assert_eq!(made.len(), want.len(), "{from} to {to}");
            // Away from the ends, where the filter has half its taps.
            let (mut signal, mut noise) = (0f64, 0f64);
            for i in 200..want.len() - 200 {
                signal += f64::from(want[i]).powi(2);
                noise += (f64::from(made[i]) - f64::from(want[i])).powi(2);
            }
            let snr = 10.0 * (signal / noise).log10();
            assert!(snr > 50.0, "{from} to {to}: {snr:.1} dB");
        }
        assert_eq!(mp3_rate(44100), 44100);
        assert_eq!(mp3_rate(8000), 48000);
        assert_eq!(mp3_rate(7350), 44100);
    }

    /// Any platform: the fixture's AAC through the built-in decoder and dm's own MP3 encoder
    /// gives whole MPEG-1 Layer III frames, as long as the sound.
    #[test]
    fn the_fixture_becomes_an_mp3_through_the_built_in_decoder() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/tone.stereo.m4a");
        let pcm = read(&fixture).unwrap();
        let bytes = crate::mp3::encode_all(&pcm.samples, pcm.channels, pcm.rate).unwrap();
        let (mut at, mut frames) = (0usize, 0u64);
        while at + 4 <= bytes.len() {
            assert_eq!(
                (bytes[at], bytes[at + 1] & 0xFE),
                (0xFF, 0xFA),
                "no MPEG-1 Layer III frame at {at}"
            );
            at += 144 * 192_000 / 44100 + usize::from((bytes[at + 2] >> 1) & 1);
            frames += 1;
        }
        assert_eq!(at, bytes.len(), "frames do not tile the file");
        let seconds = frames as f64 * 1152.0 / 44100.0;
        assert!((seconds - 2.0).abs() < 0.1, "{seconds} s");
    }

    /// A stereo 44.1 kHz AAC-LC file (a 440 Hz tone left, a sweep right) decoded by the system
    /// (macOS AudioToolbox) and by symphonia: the two agree within the rounding of two decoders.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_decode_matches_the_systems_own() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/tone.stereo.m4a");
        let ours = read(&fixture).unwrap();
        let system = crate::mp3::decode::read(&fixture).unwrap();
        assert_eq!((ours.channels, ours.rate), (system.channels, system.rate));
        // The two may differ by where they start (priming samples): find the best lag.
        let (a, b) = (&ours.samples, &system.samples);
        let best = (0..2304usize)
            .flat_map(|lag| [(lag, true), (lag, false)])
            .map(|(lag, ahead)| {
                let (x, y) = if ahead {
                    (&a[lag.min(a.len())..], &b[..])
                } else {
                    (&a[..], &b[lag.min(b.len())..])
                };
                let n = x.len().min(y.len()).min(30_000);
                let (mut signal, mut noise) = (0f64, 0f64);
                for i in 0..n {
                    signal += f64::from(y[i]).powi(2);
                    noise += (f64::from(x[i]) - f64::from(y[i])).powi(2);
                }
                (10.0 * (signal / noise.max(1.0)).log10(), lag, ahead)
            })
            .fold(
                (f64::MIN, 0, true),
                |best, now| if now.0 > best.0 { now } else { best },
            );
        eprintln!(
            "SNR {:.1} dB against the system's decode (lag {} {})",
            best.0, best.1, best.2
        );
        assert!(
            best.0 > 40.0,
            "SNR {:.1} dB against the system's decode (lag {} {})",
            best.0,
            best.1,
            best.2
        );
        let len = |p: &Pcm| p.samples.len() as i64;
        assert!(
            (len(&ours) - len(&system)).abs() <= 4096,
            "{} against {}",
            len(&ours),
            len(&system)
        );
    }
}
