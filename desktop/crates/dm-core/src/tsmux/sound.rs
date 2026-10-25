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
