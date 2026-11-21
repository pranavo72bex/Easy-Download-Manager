// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The shape and length of a whole frame, or `None` for any other answer.
fn whole(frame: Frame) -> Option<(Sound, usize, usize)> {
    match frame {
        Frame::Whole {
            shape,
            skip,
            length,
        } => Some((shape, skip, length)),
        _ => None,
    }
}

fn is_short(frame: Frame) -> bool {
    matches!(frame, Frame::Short)
}

fn not_here(frame: Frame) -> bool {
    matches!(frame, Frame::NotHere)
}

/// `fields` (value, bits) written most significant bit first.
fn bits(fields: &[(u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0u32;
    for &(value, width) in fields {
        for i in (0..width).rev() {
            if at.is_multiple_of(8) {
                out.push(0);
            }
            if (value >> i) & 1 == 1 {
                *out.last_mut().unwrap() |= 0x80 >> (at % 8);
            }
            at += 1;
        }
    }
    out
}

#[test]
fn mpeg_audio_frames_of_every_version_and_layer_are_measured() {
    let frame = |head: [u8; 4], len: usize| {
        let mut f = head.to_vec();
        f.resize(len, 0);
        f
    };
    // MPEG-2 layer III, 64 kbit/s at 22.05 kHz, mono: 576 samples a frame.
    let got = whole(mpeg_audio_frame(&frame([0xFF, 0xF3, 0x80, 0xC0], 208))).unwrap();
    assert_eq!(
        got,
        (
            Sound::Mpeg {
                mpeg1: false,
                rate: 22050,
                channels: 1,
                per_frame: 576
            },
            0,
            208
        )
    );
    // MPEG-2.5: a quarter of the rate.
    let (shape, _, _) = whole(mpeg_audio_frame(&frame([0xFF, 0xE3, 0x80, 0], 500))).unwrap();
    assert!(matches!(
        shape,
        Sound::Mpeg {
            rate: 11025,
            channels: 2,
            ..
        }
    ));
    // MPEG-1 layer I and layer II at 32 kbit/s, 44.1 kHz.
    let (shape, _, length) = whole(mpeg_audio_frame(&frame([0xFF, 0xFF, 0x10, 0], 32))).unwrap();
    assert_eq!((shape.per_frame(), length), (384, 32));
    let (shape, _, length) = whole(mpeg_audio_frame(&frame([0xFF, 0xFD, 0x10, 0], 104))).unwrap();
    assert_eq!((shape.per_frame(), length), (1152, 104));
    // Cut short, too short to have a header, or a reserved version.
    assert!(is_short(mpeg_audio_frame(&frame(
        [0xFF, 0xF3, 0x80, 0],
        100
    ))));
    assert!(is_short(mpeg_audio_frame(&[0xFF, 0xF3])));
    assert!(not_here(mpeg_audio_frame(&frame(
        [0xFF, 0xEB, 0x80, 0],
        400
    ))));
    assert!(not_here(mpeg_audio_frame(&frame(
        [0xFF, 0xF3, 0xF0, 0],
        400
    ))));
}

#[test]
fn adts_frames_with_and_without_a_checksum_are_measured() {
    // AAC-LC, 44.1 kHz (index 4), two channels, 20 bytes.
    let head = |protection_absent: bool, length: usize| {
        let mut f = vec![
            0xFF,
            0xF0 | u8::from(protection_absent),
            (1 << 6) | (4 << 2),
            (2 << 6) | ((length >> 11) as u8 & 3),
            (length >> 3) as u8,
            ((length & 7) << 5) as u8 | 0x1F,
            0xFC,
        ];
        f.resize(length, 0);
        f
    };
    let shape = Sound::Aac {
        profile: 1,
        rate_index: 4,
        channels: 2,
    };
    assert_eq!(whole(adts_frame(&head(true, 20))), Some((shape, 7, 20)));
    assert_eq!(whole(adts_frame(&head(false, 20))), Some((shape, 9, 20)));
    assert!(not_here(adts_frame(&head(false, 9))));
    assert!(is_short(adts_frame(&head(true, 20)[..15])));
    assert!(is_short(adts_frame(&[0xFF, 0xF1])));
    assert!(not_here(adts_frame(&[0u8; 20])));
}

#[test]
fn ac3_frames_are_measured_and_their_shape_read() {
    // 44.1 kHz, an odd size code (one word longer), bsid 8, 3/2 with LFE.
    let mut frame = vec![0x0B, 0x77, 0, 0, (1 << 6) | 1, 8 << 3];
    frame.extend(bits(&[(7, 3), (0, 2), (0, 2), (1, 1)]));
    frame.resize(140, 0);
    let (shape, skip, length) = whole(ac3_frame(&frame)).unwrap();
    assert_eq!((skip, length), (0, 140));
    assert!(matches!(
        shape,
        Sound::Ac3 {
            rate: 44100,
            channels: 6,
            ..
        }
    ));
    // Stereo at 48 kHz, no LFE.
    let mut stereo = vec![0x0B, 0x77, 0, 0, 0, 8 << 3];
    stereo.extend(bits(&[(2, 3), (0, 2), (0, 1)]));
    stereo.resize(128, 0);
    let (shape, _, length) = whole(ac3_frame(&stereo)).unwrap();
    assert_eq!(length, 128);
    assert!(matches!(
        shape,
        Sound::Ac3 {
            rate: 48000,
            channels: 2,
            ..
        }
    ));
    assert!(is_short(ac3_frame(&stereo[..100])));
    assert!(is_short(ac3_frame(&stereo[..6])));
    // A reserved rate, a size code past the table, an E-AC-3 bsid.
    let mut odd = stereo.clone();
    odd[4] = 3 << 6;
    assert!(not_here(ac3_frame(&odd)));
    odd[4] = 40;
    assert!(not_here(ac3_frame(&odd)));
    let mut later = stereo.clone();
    later[5] = 16 << 3;
    assert!(not_here(ac3_frame(&later)));
    assert!(not_here(ac3_frame(&[0u8; 10])));
}

#[test]
fn eac3_frames_are_measured_and_their_shape_read() {
    let frame = |fscod: u32, code: u32, bsid: u32, length: usize| {
        let mut f = vec![0x0B, 0x77];
        f.extend(bits(&[
            (0, 2),
            (0, 3),
            (length as u32 / 2 - 1, 11),
            (fscod, 2),
            (code, 2),
            (2, 3),
            (1, 1),
            (bsid, 5),
        ]));
        f.resize(length, 0);
        f
    };
    // 48 kHz, six blocks, stereo with LFE.
    let (shape, _, length) = whole(eac3_frame(&frame(0, 3, 16, 100))).unwrap();
    assert_eq!(length, 100);
    assert!(matches!(
        shape,
        Sound::Eac3 {
            rate: 48000,
            channels: 3,
            per_frame: 1536,
            ..
        }
    ));
    // Two blocks a frame.
    let (shape, _, _) = whole(eac3_frame(&frame(1, 1, 16, 100))).unwrap();
    assert!(matches!(
        shape,
        Sound::Eac3 {
            rate: 44100,
            per_frame: 512,
            ..
        }
    ));
    // A reduced rate (fscod 3), and one past its table.
    let (shape, _, _) = whole(eac3_frame(&frame(3, 1, 16, 100))).unwrap();
    assert!(matches!(
        shape,
        Sound::Eac3 {
            rate: 22050,
            per_frame: 1536,
            ..
        }
    ));
    assert!(not_here(eac3_frame(&frame(3, 3, 16, 100))));
    // An AC-3 bsid, a frame cut short, too short for a header.
    assert!(not_here(eac3_frame(&frame(0, 3, 8, 100))));
    assert!(is_short(eac3_frame(&frame(0, 3, 16, 100)[..50])));
    assert!(is_short(eac3_frame(&[0x0B, 0x77, 0])));
    assert!(not_here(eac3_frame(&[0u8; 10])));
}
