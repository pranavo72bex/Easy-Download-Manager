// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The frames of an MP3 (after its ID3 header, where it has one), each checked to be an
/// MPEG-1 Layer III frame at `rate` Hz whose header's size is where the next starts, the last
/// ending with the file: each one's bit rate, kbit/s.
pub(super) fn frame_rates(bytes: &[u8], rate: u32) -> Vec<u32> {
    let mut at = 0;
    if bytes.starts_with(b"ID3") {
        at = 10
            + bytes[6..10]
                .iter()
                .fold(0usize, |n, &b| (n << 7) | usize::from(b));
    }
    let mut rates = Vec::new();
    while at + 4 <= bytes.len() {
        assert_eq!(
            (bytes[at], bytes[at + 1] & 0xFE),
            (0xFF, 0xFA),
            "no MPEG-1 Layer III frame at {at}"
        );
        let index = usize::from(bytes[at + 2] >> 4);
        assert!((1..15).contains(&index), "bit rate index {index} at {at}");
        let at_rate = [44100, 48000, 32000][usize::from((bytes[at + 2] >> 2) & 3)];
        assert_eq!(at_rate, rate, "sample rate at {at}");
        let kbps = BITRATES[index];
        at += (144_000 * kbps / rate) as usize + usize::from((bytes[at + 2] >> 1) & 1);
        rates.push(kbps);
    }
    assert_eq!(at, bytes.len(), "frames do not tile the file");
    rates
}

/// At every bit rate MPEG-1 Layer III has, each frame's header names that rate, the file is as
/// long as the rate and the sound's length make it, and another decoder (symphonia's) plays it
/// back as the sound that went in, as long as it was.
#[test]
fn each_bit_rate_is_in_every_frame_and_plays_back_as_long_as_it_was() {
    for (channels, rate, kbps_list) in [
        (2usize, 44100u32, &KBPS[..]),
        (1, 48000, &[32, 128, 320][..]),
        (2, 32000, &[32, 192, 320][..]),
    ] {
        // One second of the tones.
        let stereo = &tones(rate)[..rate as usize * 2];
        let input: Vec<i16> = if channels == 2 {
            stereo.to_vec()
        } else {
            stereo.chunks(2).map(|p| p[0]).collect()
        };
        let per_channel = input.len() / channels;
        // Whole frames for the sound, and the filterbank's delay.
        let frames = per_channel.div_ceil(FRAME) + 1;
        for &kbps in kbps_list {
            let mp3 = encode_all(&input, channels, rate, Some(kbps)).unwrap();
            let rates = frame_rates(&mp3, rate);
            assert_eq!(rates.len(), frames, "{kbps} kbit/s");
            assert!(rates.iter().all(|r| *r == kbps), "{kbps} kbit/s: {rates:?}");
            // The padding keeps the average exact: the length is the rate's.
            assert_eq!(
                mp3.len() as u64,
                frames as u64 * 144_000 * u64::from(kbps) / u64::from(rate),
                "{kbps} kbit/s"
            );
            let out = heard(&mp3);
            assert_eq!((out.channels, out.rate), (channels, rate), "{kbps} kbit/s");
            let seconds = out.samples.len() as f64 / (channels as f64 * f64::from(rate));
            let frame = FRAME as f64 / f64::from(rate);
            assert!(
                seconds >= 1.0 && seconds <= 1.0 + 2.0 * frame,
                "{kbps} kbit/s: {seconds} s"
            );
            let (snr, delay) = alike(&input, &out.samples, channels, rate);
            eprintln!("{channels} ch {rate} Hz {kbps} kbit/s: {snr:.1} dB at {delay}");
            // Two tones need few bits: even the lowest rate keeps them well above the noise
            // (measured: 44.6 dB at 32 kbit/s, over 74 dB from 128 kbit/s up).
            let floor = if kbps >= 128 { 60.0 } else { 30.0 };
            assert!(
                snr > floor,
                "{channels} ch at {rate} Hz, {kbps} kbit/s: {snr:.1} dB at a delay of {delay}"
            );
        }
    }
}

/// With no rate asked for, the bytes are the ones dm always made: 192 kbit/s for two channels,
/// 128 for one.
#[test]
fn no_rate_asked_for_is_192_for_stereo_and_128_for_mono_as_before() {
    let stereo = &tones(44100)[..44100];
    let mono: Vec<i16> = stereo.chunks(2).map(|p| p[0]).collect();
    assert_eq!(default_kbps(2), 192);
    assert_eq!(default_kbps(1), 128);
    assert_eq!(
        encode_all(stereo, 2, 44100, None).unwrap(),
        encode_all(stereo, 2, 44100, Some(192)).unwrap()
    );
    assert_eq!(
        encode_all(&mono, 1, 44100, None).unwrap(),
        encode_all(&mono, 1, 44100, Some(128)).unwrap()
    );
    assert_eq!(
        encode(stereo, 2, 44100),
        encode_all(stereo, 2, 44100, None).unwrap()
    );
    assert!(frame_rates(&encode(stereo, 2, 44100), 44100)
        .iter()
        .all(|r| *r == 192));
}

/// A rate MPEG-1 Layer III does not have is refused in words, before any sound is read.
#[test]
fn a_rate_it_does_not_have_is_refused_in_words() {
    for kbps in [0, 8, 100, 193, 384, 448] {
        let said = Encoder::at(2, 44100, Some(kbps)).err().unwrap();
        assert!(
            said.contains("kbit/s") && said.contains("320"),
            "{kbps}: {said}"
        );
        assert!(encode_all(&[0; 4608], 2, 44100, Some(kbps)).is_err());
    }
    // Refused before the source is opened: a file that is not there says the rate.
    let said = convert_bytes(std::path::Path::new("no such file.m4a"), Some(100)).unwrap_err();
    assert!(said.contains("kbit/s"), "{said}");
}

/// The rate reaches the file through `convert_bytes` as through the encoder: the fixture's AAC
/// at 320 kbit/s on every system that reads AAC.
#[test]
fn convert_bytes_writes_the_rate_asked_for() {
    if !possible() {
        eprintln!("no AAC decoder here (a Windows N edition): nothing to check");
        return;
    }
    let m4a = crate::fixtures().join("mp4/tone.stereo.m4a");
    let pcm = decode::read(&m4a).unwrap();
    let bytes = convert_bytes(&m4a, Some(320)).unwrap();
    let rates = frame_rates(&bytes, pcm.rate);
    assert!(rates.iter().all(|r| *r == 320), "{rates:?}");
    let made = rates.len() as f64 * FRAME as f64 / f64::from(pcm.rate);
    let seconds = pcm.samples.len() as f64 / (pcm.channels as f64 * f64::from(pcm.rate));
    assert!(
        (made - seconds).abs() < 0.1,
        "{made} s of MP3 for {seconds} s"
    );
}
