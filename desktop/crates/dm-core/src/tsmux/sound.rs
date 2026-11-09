// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Cutting sound into frames: AAC, MPEG audio, AC-3 and E-AC-3.

use super::*;

pub(super) fn take_sound(
    stream: &mut Elementary,
    data: &[u8],
    pts: Option<i64>,
) -> Result<(), String> {
    if stream.first_pts.is_none() {
        stream.first_pts = pts;
    }
    stream.adts.extend_from_slice(data);
    let mut at = 0;
    let buffered = std::mem::take(&mut stream.adts);
    let reader = match stream.kind {
        AAC => adts_frame,
        AC3 => ac3_frame,
        EAC3 => eac3_frame,
        _ => mpeg_audio_frame,
    };
    while let Some(rest) = buffered.get(at..) {
        match reader(rest) {
            Frame::Whole {
                shape,
                skip,
                length,
            } => {
                // The first frame's shape is the stream's.
                stream.sound.get_or_insert(shape);
                let frame = rest.get(skip..length).unwrap_or_default().to_vec();
                write(stream, &frame)?;
                let offset = stream.written - frame.len() as u64;
                stream
                    .samples
                    .push((offset, frame.len() as u32, 0, 0, true));
                at += length;
            }
            Frame::NotHere => at += 1,
            Frame::Short => break,
        }
    }
    stream.adts = buffered.get(at..).unwrap_or_default().to_vec();
    Ok(())
}

/// What a sound frame reader found at the start of its bytes.
pub(crate) enum Frame {
    /// A frame of `length` bytes, its sample from `skip` on.
    Whole {
        shape: Sound,
        skip: usize,
        length: usize,
    },
    /// No frame starts here: step on a byte.
    NotHere,
    /// One starts, but its end has not arrived.
    Short,
}

/// An ADTS frame: AAC with a 7- or 9-byte header, which is not part of the
/// MP4 sample.
pub(super) fn adts_frame(rest: &[u8]) -> Frame {
    let Some(head) = rest.get(..7).and_then(|h| <[u8; 7]>::try_from(h).ok()) else {
        return Frame::Short;
    };
    if head[0] != 0xff || head[1] & 0xf0 != 0xf0 {
        return Frame::NotHere;
    }
    let header = if head[1] & 1 != 0 { 7 } else { 9 };
    let length = (usize::from(head[3] & 3) << 11)
        | (usize::from(head[4]) << 3)
        | (usize::from(head[5]) >> 5);
    if length <= header {
        return Frame::NotHere;
    }
    if rest.len() < length {
        return Frame::Short;
    }
    Frame::Whole {
        shape: Sound::Aac {
            profile: head[2] >> 6,
            rate_index: (head[2] >> 2) & 0x0f,
            channels: ((head[2] & 1) << 2) | (head[3] >> 6),
        },
        skip: header,
        length,
    }
}

/// An MPEG-1/2 audio frame (MP3, MP2), header and all.
pub(crate) fn mpeg_audio_frame(rest: &[u8]) -> Frame {
    let Some(head) = rest.get(..4).and_then(|h| <[u8; 4]>::try_from(h).ok()) else {
        return Frame::Short;
    };
    if head[0] != 0xff || head[1] & 0xe0 != 0xe0 {
        return Frame::NotHere;
    }
    let version = (head[1] >> 3) & 3; // 3: MPEG-1, 2: MPEG-2, 0: MPEG-2.5
    let layer = (head[1] >> 1) & 3; // 3: I, 2: II, 1: III
    let bitrate_index = usize::from(head[2] >> 4);
    let rate_index = usize::from((head[2] >> 2) & 3);
    let padding = u32::from((head[2] >> 1) & 1);
    if version == 1 || layer == 0 || bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return Frame::NotHere;
    }
    const V1: [[u32; 15]; 3] = [
        [
            0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
        ],
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
        ],
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ],
    ];
    const V2: [[u32; 15]; 2] = [
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
        ],
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
    ];
    let mpeg1 = version == 3;
    let row = 3 - usize::from(layer); // 0: I, 1: II, 2: III
    let kbps = if mpeg1 {
        V1.get(row).and_then(|r| r.get(bitrate_index))
    } else {
        V2.get(row.min(1)).and_then(|r| r.get(bitrate_index))
    };
    let base = [44100, 48000, 32000];
    let Some(rate) = base.get(rate_index).map(|r| match version {
        3 => *r,
        2 => r / 2,
        _ => r / 4,
    }) else {
        return Frame::NotHere;
    };
    let Some(bitrate) = kbps.map(|k| k * 1000) else {
        return Frame::NotHere;
    };
    let (length, per_frame) = match row {
        0 => ((12 * bitrate / rate + padding) * 4, 384),
        1 => (144 * bitrate / rate + padding, 1152),
        _ if mpeg1 => (144 * bitrate / rate + padding, 1152),
        _ => (72 * bitrate / rate + padding, 576),
    };
    let length = length as usize;
    if length < 4 {
        return Frame::NotHere;
    }
    if rest.len() < length {
        return Frame::Short;
    }
    Frame::Whole {
        shape: Sound::Mpeg {
            mpeg1,
            rate,
            channels: if head[3] >> 6 == 3 { 1 } else { 2 },
            per_frame,
        },
        skip: 0,
        length,
    }
}

/// An E-AC-3 sync frame, whole: one independent substream (a stream with
/// dependent ones, for more than 5.1, is not read).
pub(super) fn eac3_frame(rest: &[u8]) -> Frame {
    let Some(head) = rest.get(..8) else {
        return Frame::Short;
    };
    if head.get(..2) != Some(&[0x0b, 0x77]) {
        return Frame::NotHere;
    }
    let mut bits = Bits {
        data: head.get(2..).unwrap_or_default(),
        at: 0,
    };
    let (Some(kind), Some(_substream), Some(size), Some(fscod)) =
        (bits.take(2), bits.take(3), bits.take(11), bits.take(2))
    else {
        return Frame::Short;
    };
    let (rate, blocks) = if fscod == 3 {
        let Some(rate) = bits
            .take(2)
            .and_then(|code| [24000u32, 22050, 16000].get(code as usize).copied())
        else {
            return Frame::NotHere;
        };
        (rate, 6)
    } else {
        let Some(code) = bits.take(2) else {
            return Frame::Short;
        };
        let rate = [48000u32, 44100, 32000]
            .get(fscod as usize)
            .copied()
            .unwrap_or(48000);
        (
            rate,
            [1u32, 2, 3, 6].get(code as usize).copied().unwrap_or(6),
        )
    };
    let (Some(acmod), Some(lfe), Some(bsid)) = (bits.take(3), bits.take(1), bits.take(5)) else {
        return Frame::Short;
    };
    // Type 0 is an independent substream; E-AC-3 says bsid 11 to 16.
    if kind != 0 || !(11..=16).contains(&bsid) {
        return Frame::NotHere;
    }
    let length = (size as usize + 1) * 2;
    if rest.len() < length {
        return Frame::Short;
    }
    let per_frame = blocks * 256;
    let kbps = length as u64 * 8 * u64::from(rate) / u64::from(per_frame) / 1000;
    // dec3: the data rate and one independent substream (the count is one
    // less), then that substream's shape.
    let rate_bits = (kbps.min(8191) as u16) << 3;
    let sub: u32 = (fscod.min(3) << 22) | (bsid << 17) | (acmod << 9) | (lfe << 8);
    let [_, a, b, c] = sub.to_be_bytes();
    let [high, low] = rate_bits.to_be_bytes();
    let channels = [2u16, 1, 2, 3, 3, 4, 4, 5]
        .get(acmod as usize)
        .copied()
        .unwrap_or(2)
        + lfe as u16;
    Frame::Whole {
        shape: Sound::Eac3 {
            dec3: [high, low, a, b, c],
            rate,
            channels,
            per_frame,
        },
        skip: 0,
        length,
    }
}

/// An AC-3 sync frame, whole.
pub(super) fn ac3_frame(rest: &[u8]) -> Frame {
    let Some(head) = rest.get(..8) else {
        return Frame::Short;
    };
    if head.get(..2) != Some(&[0x0b, 0x77]) {
        return Frame::NotHere;
    }
    let (Some(b4), Some(b5)) = (head.get(4), head.get(5)) else {
        return Frame::Short;
    };
    let fscod = b4 >> 6;
    let frmsizecod = usize::from(b4 & 0x3f);
    let bsid = b5 >> 3;
    let bsmod = b5 & 7;
    if fscod == 3 || frmsizecod > 37 || bsid > 10 {
        return Frame::NotHere;
    }
    const WORDS: [[u16; 19]; 3] = [
        [
            64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024,
            1152, 1280,
        ],
        [
            69, 87, 104, 121, 139, 174, 208, 243, 278, 348, 417, 487, 557, 696, 835, 975, 1114,
            1253, 1393,
        ],
        [
            96, 120, 144, 168, 192, 240, 288, 336, 384, 480, 576, 672, 768, 960, 1152, 1344, 1536,
            1728, 1920,
        ],
    ];
    let Some(words) = WORDS
        .get(usize::from(fscod))
        .and_then(|row| row.get(frmsizecod / 2))
    else {
        return Frame::NotHere;
    };
    // At 44.1 kHz an odd code is a frame one word longer.
    let words = usize::from(*words) + usize::from(fscod == 1 && frmsizecod % 2 == 1);
    let length = words * 2;
    let mut bits = Bits {
        data: head.get(6..).unwrap_or_default(),
        at: 0,
    };
    let Some(acmod) = bits.take(3) else {
        return Frame::Short;
    };
    if acmod & 1 != 0 && acmod != 1 {
        bits.take(2);
    }
    if acmod & 4 != 0 {
        bits.take(2);
    }
    if acmod == 2 {
        bits.take(2);
    }
    let lfe = bits.take(1).unwrap_or(0);
    if rest.len() < length {
        return Frame::Short;
    }
    let rate = [48000, 44100, 32000]
        .get(usize::from(fscod))
        .copied()
        .unwrap_or(48000);
    let bit_rate_code = (frmsizecod / 2) as u32;
    let packed: u32 = (u32::from(fscod) << 22)
        | (u32::from(bsid) << 17)
        | (u32::from(bsmod) << 14)
        | (acmod << 11)
        | (lfe << 10)
        | (bit_rate_code << 5);
    let [_, a, b, c] = packed.to_be_bytes();
    let channels = [2u16, 1, 2, 3, 3, 4, 4, 5]
        .get(acmod as usize)
        .copied()
        .unwrap_or(2)
        + lfe as u16;
    Frame::Whole {
        shape: Sound::Ac3 {
            dac3: [a, b, c],
            rate,
            channels,
        },
        skip: 0,
        length,
    }
}

// ---- the MP4 tracks -----------------------------------------------------

#[cfg(test)]
mod tests {
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
        let (shape, _, length) =
            whole(mpeg_audio_frame(&frame([0xFF, 0xFF, 0x10, 0], 32))).unwrap();
        assert_eq!((shape.per_frame(), length), (384, 32));
        let (shape, _, length) =
            whole(mpeg_audio_frame(&frame([0xFF, 0xFD, 0x10, 0], 104))).unwrap();
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
}
