// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
    let fixture = crate::fixtures().join("mp4/sound.f140.m4a");
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
                let v = (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / f64::from(rate)).sin();
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
    let fixture = crate::fixtures().join("mp4/tone.stereo.m4a");
    let pcm = read(&fixture).unwrap();
    let bytes = crate::mp3::encode_all(&pcm.samples, pcm.channels, pcm.rate, None).unwrap();
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
    let fixture = crate::fixtures().join("mp4/tone.stereo.m4a");
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
