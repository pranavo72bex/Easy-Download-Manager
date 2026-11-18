// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// [`convert_bytes`] written to `target`, as dm-core's `mp3::convert` does; the MP3's length.
fn convert(source: &std::path::Path, target: &std::path::Path) -> Result<u64, String> {
    let bytes = convert_bytes(source, None)?;
    std::fs::write(target, &bytes).map_err(|e| e.to_string())?;
    Ok(bytes.len() as u64)
}

/// An MP3 made from a video carries its title, artist and page in an ID3v2.4 header;
/// with no words, none (gaps row Y25).
#[test]
fn an_mp3_carries_its_words_in_an_id3_header() {
    let tags = crate::mp4mux::Tags {
        title: "Me at the zoo".into(),
        artist: "jawed".into(),
        date: String::new(),
        source: "https://www.youtube.com/watch?v=jNQXAC9IVRw".into(),
        cover: b"\x89PNG\r\n\x1a\n a picture".to_vec(),
        ..Default::default()
    };
    let header = id3(&tags);
    assert_eq!(&header[..5], b"ID3\x04\x00");
    let size = header[6..10]
        .iter()
        .fold(0usize, |n, &b| (n << 7) | usize::from(b));
    assert_eq!(size, header.len() - 10);
    let text = String::from_utf8_lossy(&header);
    assert!(text.contains("TIT2") && text.contains("Me at the zoo"));
    // Its picture as the front cover (gaps row Y11).
    assert!(text.contains("APIC"));
    assert!(header.windows(12).any(|w| w == b"image/png\x00\x03\x00"));
    assert!(text.contains("TPE1") && text.contains("jawed"));
    assert!(text.contains("WOAS") && !text.contains("TDRC"));
    assert!(id3(&crate::mp4mux::Tags::default()).is_empty());
}

/// Two seconds of a tone in each channel (440 Hz left, 1 kHz right).
fn tones(rate: u32) -> Vec<i16> {
    let n = rate as usize * 2;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f64 / f64::from(rate);
        out.push(((2.0 * PI * 440.0 * t).sin() * 12000.0) as i16);
        out.push(((2.0 * PI * 1000.0 * t).sin() * 8000.0) as i16);
    }
    out
}

fn encode(samples: &[i16], channels: usize, rate: u32) -> Vec<u8> {
    let mut e = Encoder::new(channels, rate).unwrap();
    let mut out = e.push(samples);
    out.extend(e.finish());
    out
}

#[test]
fn every_frame_starts_with_its_sync_and_has_its_size() {
    let mp3 = encode(&tones(44100), 2, 44100);
    let mut at = 0;
    let mut frames = 0;
    while at + 4 <= mp3.len() {
        assert_eq!(mp3[at], 0xFF, "no sync at {at}");
        assert_eq!(mp3[at + 1] & 0xE0, 0xE0, "no sync at {at}");
        let padding = usize::from((mp3[at + 2] >> 1) & 1);
        at += 144 * 192_000 / 44100 + padding;
        frames += 1;
    }
    assert_eq!(at, mp3.len(), "frames do not tile the file");
    // Two seconds, the last part frame, and the flush.
    assert_eq!(frames, 88200 / FRAME + 2);
}

#[test]
fn rates_and_channels_it_cannot_make_are_refused_in_words() {
    assert!(Encoder::new(2, 22050).err().unwrap().contains("44.1"));
    assert!(Encoder::new(6, 44100).is_err());
}

/// Decoded by the system's own MP3 decoder, the sound is the sound
/// that went in: aligned, its signal-to-noise ratio is high.
#[cfg(target_os = "macos")]
#[test]
fn the_system_decoder_plays_back_what_went_in() {
    let dir = tempfile::tempdir().unwrap();
    for (channels, rate) in [(2usize, 44100u32), (1, 48000), (2, 32000)] {
        let input: Vec<i16> = if channels == 2 {
            tones(rate)
        } else {
            tones(rate).chunks(2).map(|p| p[0]).collect()
        };
        let mp3 = dir.path().join(format!("t{channels}{rate}.mp3"));
        let wav = dir.path().join(format!("t{channels}{rate}.wav"));
        std::fs::write(&mp3, encode(&input, channels, rate)).unwrap();
        let ok = std::process::Command::new("/usr/bin/afconvert")
            .arg(&mp3)
            .arg(&wav)
            .args(["-f", "WAVE", "-d", "LEI16"])
            .status()
            .unwrap()
            .success();
        assert!(ok, "the system would not decode it");
        let bytes = std::fs::read(&wav).unwrap();
        let data = bytes
            .windows(4)
            .position(|w| w == b"data")
            .expect("no data in the WAV")
            + 8;
        let out: Vec<i16> = bytes[data..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
        // The encoder and decoder's delay: the shift that matches best.
        let left = |s: &[i16], i: usize| f64::from(s[i * channels]);
        let span = rate as usize / 2;
        let start = rate as usize / 4;
        let mut best = (0usize, f64::MAX);
        for shift in 0..3000 {
            if (start + span + shift) * channels > out.len() {
                break;
            }
            let err: f64 = (start..start + span)
                .map(|i| (left(&out, i + shift) - left(&input, i)).powi(2))
                .sum();
            if err < best.1 {
                best = (shift, err);
            }
        }
        let signal: f64 = (start..start + span).map(|i| left(&input, i).powi(2)).sum();
        let snr = 10.0 * (signal / best.1.max(1e-9)).log10();
        assert!(
            snr > 30.0,
            "{channels} ch at {rate} Hz: {snr:.1} dB at a delay of {}",
            best.0
        );
    }
}

/// An AAC sound (made by the system from a tone) saved as MP3 through
/// `convert`: the system decoder reads the AAC, dm encodes it, and the
/// MP3 decodes back to the tone.
#[cfg(target_os = "macos")]
#[test]
fn an_aac_sound_is_saved_as_an_mp3_that_plays_it() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("tone.wav");
    let m4a = dir.path().join("tone.m4a");
    let mp3 = dir.path().join("tone.mp3");
    // A 440 Hz stereo tone as a WAV, then AAC by the system.
    let samples = tones(44100);
    let mut file = Vec::new();
    let data = (samples.len() * 2) as u32;
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    for v in [1u16, 2] {
        file.extend_from_slice(&v.to_le_bytes());
    }
    for v in [44100u32, 44100 * 4] {
        file.extend_from_slice(&v.to_le_bytes());
    }
    for v in [4u16, 16] {
        file.extend_from_slice(&v.to_le_bytes());
    }
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data.to_le_bytes());
    for s in &samples {
        file.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(&wav, file).unwrap();
    let made = std::process::Command::new("/usr/bin/afconvert")
        .arg(&wav)
        .arg(&m4a)
        .args(["-f", "m4af", "-d", "aac"])
        .status()
        .unwrap()
        .success();
    assert!(made, "the system would not make the AAC");
    let length = convert(&m4a, &mp3).unwrap();
    assert!(length > 40_000, "{length}");
    let pcm = decode::read(&mp3).unwrap();
    assert_eq!((pcm.channels, pcm.rate), (2, 44100));
    // The tone is there at about the level it went in.
    let seg = &pcm.samples[40000..80000];
    let (mut c, mut s) = (0.0f64, 0.0f64);
    for (i, v) in seg.chunks(2).enumerate() {
        let ph = 2.0 * PI * 440.0 * i as f64 / 44100.0;
        c += f64::from(v[0]) * ph.cos();
        s += f64::from(v[0]) * ph.sin();
    }
    let level = 2.0 * (c * c + s * s).sqrt() / (seg.len() / 2) as f64;
    assert!((10_000.0..14_000.0).contains(&level), "{level}");
}

/// A real AAC sound (the fixture) saved as MP3 through `convert`, on
/// Windows: Media Foundation's decoder reads it, dm encodes it, and the
/// file is a run of whole MPEG-1 Layer III frames as long as the sound.
#[cfg(any(windows, target_os = "linux"))]
#[test]
fn a_real_aac_sound_is_saved_as_an_mp3_as_long_as_it_is() {
    if !possible() {
        eprintln!("no AAC decoder on this Windows (an N edition): nothing to check");
        return;
    }
    let m4a = crate::fixtures().join("mp4/sound.f140.m4a");
    let dir = tempfile::tempdir().unwrap();
    let mp3 = dir.path().join("sound.mp3");
    let length = convert(&m4a, &mp3).unwrap();
    let bytes = std::fs::read(&mp3).unwrap();
    assert_eq!(bytes.len() as u64, length);
    let pcm = decode::read(&m4a).unwrap();
    let seconds = pcm.samples.len() as f64 / (pcm.channels as f64 * f64::from(pcm.rate));
    assert!(seconds > 0.5, "{seconds} s of sound");
    let bitrate: u64 = if pcm.channels == 2 { 192_000 } else { 128_000 };
    let rate = u64::from(pcm.rate);
    let (mut at, mut frames) = (0usize, 0u64);
    while at + 4 <= bytes.len() {
        assert_eq!(bytes[at], 0xFF, "no sync at {at}");
        assert_eq!(bytes[at + 1] & 0xE0, 0xE0, "no sync at {at}");
        assert_eq!((bytes[at + 1] >> 3) & 3, 3, "not MPEG-1 at {at}");
        assert_eq!((bytes[at + 1] >> 1) & 3, 1, "not Layer III at {at}");
        let padding = usize::from((bytes[at + 2] >> 1) & 1);
        at += (144 * bitrate / rate) as usize + padding;
        frames += 1;
    }
    assert_eq!(at, bytes.len(), "frames do not tile the file");
    let made = frames as f64 * FRAME as f64 / f64::from(pcm.rate);
    assert!(
        (made - seconds).abs() < 0.1,
        "{made} s of MP3 for {seconds} s of sound"
    );
}

/// Decoded by Media Foundation's own MP3 decoder, the sound is the sound
/// that went in: aligned, its signal-to-noise ratio is high (the check
/// the macOS build makes with the system's decoder).
#[cfg(windows)]
#[test]
fn the_windows_decoder_plays_back_what_went_in() {
    use windows::Win32::Media::MediaFoundation::{
        MFAudioFormat_MP3, MFMediaType_Audio, MFT_CATEGORY_AUDIO_DECODER,
    };
    if !crate::mf::decoder_present(
        MFT_CATEGORY_AUDIO_DECODER,
        MFMediaType_Audio,
        MFAudioFormat_MP3,
    ) {
        eprintln!("no MP3 decoder on this Windows: nothing to check");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for (channels, rate) in [(2usize, 44100u32), (1, 48000), (2, 32000)] {
        let input: Vec<i16> = if channels == 2 {
            tones(rate)
        } else {
            tones(rate).chunks(2).map(|p| p[0]).collect()
        };
        let mp3 = dir.path().join(format!("t{channels}{rate}.mp3"));
        std::fs::write(&mp3, encode(&input, channels, rate)).unwrap();
        let heard = decode::read(&mp3).unwrap();
        assert_eq!((heard.channels, heard.rate), (channels, rate));
        let out = heard.samples;
        // The encoder and decoder's delay: the shift that matches best.
        let left = |s: &[i16], i: usize| f64::from(s[i * channels]);
        let span = rate as usize / 2;
        let start = rate as usize / 4;
        let mut best = (0usize, f64::MAX);
        for shift in 0..3000 {
            if (start + span + shift) * channels > out.len() {
                break;
            }
            let err: f64 = (start..start + span)
                .map(|i| (left(&out, i + shift) - left(&input, i)).powi(2))
                .sum();
            if err < best.1 {
                best = (shift, err);
            }
        }
        let signal: f64 = (start..start + span).map(|i| left(&input, i).powi(2)).sum();
        let snr = 10.0 * (signal / best.1.max(1e-9)).log10();
        assert!(
            snr > 30.0,
            "{channels} ch at {rate} Hz: {snr:.1} dB at a delay of {}",
            best.0
        );
    }
}

/// A file that is not sound is refused in words, by name or without one
/// the reader knows, and never panics.
#[cfg(any(windows, target_os = "linux"))]
#[test]
fn a_file_that_is_not_sound_is_refused_in_words() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["not-sound.m4a", "not-sound.mp4", "not-sound.part"] {
        let source = dir.path().join(name);
        std::fs::write(&source, b"this is text, not a sound").unwrap();
        let target = dir.path().join("out.mp3");
        let said = convert(&source, &target).unwrap_err();
        assert!(!said.is_empty(), "{name}");
        assert!(!target.exists(), "{name} left an MP3");
    }
    assert!(convert(&dir.path().join("missing.m4a"), &dir.path().join("o.mp3")).is_err());
}

#[test]
fn encoding_on_every_core_gives_the_bytes_one_run_gives() {
    // Long enough to be cut into runs: ten seconds of the tones.
    let mut samples = tones(44100);
    samples.extend(tones(44100));
    samples.extend(tones(44100));
    samples.extend(tones(44100));
    samples.extend(tones(44100));
    assert_eq!(
        encode_all(&samples, 2, 44100, None).unwrap(),
        encode(&samples, 2, 44100)
    );
}

/// An MP3 decoded by a decoder that is not dm's (symphonia's), one frame
/// a packet: the Linux build's stand-in for the system decoders that
/// play the files back in the macOS and Windows checks, and on every
/// system the check of each bit rate.
fn heard(bytes: &[u8]) -> crate::mp3::decode::Pcm {
    use symphonia_bundle_mp3::MpaDecoder;
    use symphonia_core::codecs::audio::well_known::CODEC_ID_MP3;
    use symphonia_core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
    use symphonia_core::packet::PacketRef;
    use symphonia_core::units::{Duration, Timestamp};
    const KBPS_AT: [u64; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const RATES: [u32; 3] = [44100, 48000, 32000];
    let mut params = AudioCodecParameters::new();
    params.for_codec(CODEC_ID_MP3);
    let mut decoder = MpaDecoder::try_new(&params, &AudioDecoderOptions::default()).unwrap();
    let (mut at, mut n) = (0usize, 0i64);
    let (mut samples, mut frame, mut channels, mut rate) = (Vec::new(), Vec::new(), 0, 0);
    while at + 4 <= bytes.len() {
        assert_eq!(
            (bytes[at], bytes[at + 1] & 0xFE),
            (0xFF, 0xFA),
            "no frame at {at}"
        );
        let kbps = KBPS_AT[usize::from(bytes[at + 2] >> 4)];
        rate = RATES[usize::from((bytes[at + 2] >> 2) & 3)];
        channels = if bytes[at + 3] >> 6 == 3 { 1 } else { 2 };
        let size =
            (144_000 * kbps / u64::from(rate)) as usize + usize::from((bytes[at + 2] >> 1) & 1);
        let packet = PacketRef::new(
            0,
            Timestamp::new(n * 1152),
            Duration::new(1152),
            &bytes[at..at + size],
        );
        decoder
            .decode_ref(&packet)
            .unwrap()
            .copy_to_vec_interleaved(&mut frame);
        samples.extend_from_slice(&frame);
        at += size;
        n += 1;
    }
    crate::mp3::decode::Pcm {
        samples,
        channels,
        rate,
    }
}

/// How alike two interleaved sounds are on their first channel, at the
/// shift that matches best: signal-to-noise in dB.
fn alike(input: &[i16], out: &[i16], channels: usize, rate: u32) -> (f64, usize) {
    let left = |s: &[i16], i: usize| f64::from(s[i * channels]);
    let span = rate as usize / 2;
    let start = rate as usize / 4;
    let mut best = (0usize, f64::MAX);
    for shift in 0..3000 {
        if (start + span + shift) * channels > out.len() {
            break;
        }
        let err: f64 = (start..start + span)
            .map(|i| (left(out, i + shift) - left(input, i)).powi(2))
            .sum();
        if err < best.1 {
            best = (shift, err);
        }
    }
    let signal: f64 = (start..start + span).map(|i| left(input, i).powi(2)).sum();
    (10.0 * (signal / best.1.max(1e-9)).log10(), best.0)
}

/// Linux: dm's MP3s, decoded by another decoder, are the sound that went
/// in (the check macOS makes with afconvert and Windows with Media
/// Foundation).
#[cfg(target_os = "linux")]
#[test]
fn another_decoder_plays_back_what_went_in() {
    for (channels, rate) in [(2usize, 44100u32), (1, 48000), (2, 32000)] {
        let input: Vec<i16> = if channels == 2 {
            tones(rate)
        } else {
            tones(rate).chunks(2).map(|p| p[0]).collect()
        };
        let out = heard(&encode(&input, channels, rate));
        assert_eq!((out.channels, out.rate), (channels, rate));
        let (snr, delay) = alike(&input, &out.samples, channels, rate);
        assert!(
            snr > 30.0,
            "{channels} ch at {rate} Hz: {snr:.1} dB at a delay of {delay}"
        );
    }
}

/// Linux: `convert`, as the app calls it, turns a stereo AAC (a 440 Hz
/// tone left, a sweep right) into an MP3 that plays that sound back.
#[cfg(target_os = "linux")]
#[test]
fn an_aac_sound_is_saved_as_an_mp3_that_plays_it() {
    let m4a = crate::fixtures().join("mp4/tone.stereo.m4a");
    let dir = tempfile::tempdir().unwrap();
    let mp3 = dir.path().join("tone.mp3");
    assert!(possible());
    let length = convert(&m4a, &mp3).unwrap();
    let bytes = std::fs::read(&mp3).unwrap();
    assert_eq!(bytes.len() as u64, length);
    let aac = decode::read(&m4a).unwrap();
    let out = heard(&bytes);
    assert_eq!((out.channels, out.rate), (2, 44100));
    let (snr, delay) = alike(&aac.samples, &out.samples, 2, 44100);
    eprintln!("the MP3 against its AAC: {snr:.1} dB at a delay of {delay}");
    assert!(snr > 20.0, "{snr:.1} dB at a delay of {delay}");
    let seconds = out.samples.len() as f64 / 2.0 / 44100.0;
    assert!((seconds - 2.0).abs() < 0.1, "{seconds} s");
}

/// Each of MPEG-1 Layer III's bit rates (gaps row Y25).
mod rates;

/// Opus in a WebM as the sound an MP3 is made from (gaps row Y25).
mod opus;
