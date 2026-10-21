//! Puts an HLS stream's MPEG transport stream (`.ts`) into an MP4, without
//! ffmpeg.
//!
//! Players open an MP4; few open a `.ts`. An HLS stream's segments are a
//! transport stream: 188-byte packets carrying H.264 or HEVC pictures and
//! AAC, MP3/MP2 or AC-3 sound
//! in PES packets, each stamped with when it plays. This reads them out —
//! the tables that say which packets are which stream, the pictures as the
//! byte stream an encoder wrote and the sound as ADTS frames — and hands
//! them to the MP4 writer (`mp4mux`) as samples: each picture with its
//! start codes turned into lengths, each sound frame without its ADTS
//! header, picture and sound started together. Nothing is decoded or
//! encoded. A stream in anything else (HEVC, MP3, AC-3) is refused, and the
//! caller keeps the `.ts` or leaves it to an ffmpeg.
//!
//! The input came from a server: nothing here indexes past what it checked.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::mp4mux::{boxed, full, Edit, Sample, Track, MATRIX};

const PACKET: usize = 188;
const TIMESCALE: u32 = 90_000;
/// PTS and DTS are 33 bits and wrap.
const WRAP: i64 = 1 << 33;

const H264: u8 = 0x1b;
const HEVC: u8 = 0x24;
const AAC: u8 = 0x0f;
const MPEG1_AUDIO: u8 = 0x03;
const MPEG2_AUDIO: u8 = 0x04;
const AC3: u8 = 0x81;
const PICTURES: [u8; 2] = [H264, HEVC];
const SOUNDS: [u8; 4] = [AAC, MPEG1_AUDIO, MPEG2_AUDIO, AC3];
/// Picture and sound kinds this does not carry: MPEG-1/2 and MPEG-4 Part 2
/// pictures, AVS; LATM AAC, E-AC-3. A stream with one is refused whole
/// rather than put into an MP4 without it.
const OTHERS: [u8; 6] = [0x01, 0x02, 0x10, 0x42, 0x11, 0x87];

/// The shape of a sound stream, from its first frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sound {
    Aac {
        profile: u8,
        rate_index: u8,
        channels: u8,
    },
    /// MPEG-1 or -2 audio (MP3, MP2): its frames go in whole.
    Mpeg {
        mpeg1: bool,
        rate: u32,
        channels: u16,
        per_frame: u32,
    },
    Ac3 {
        dac3: [u8; 3],
        rate: u32,
        channels: u16,
    },
}

impl Sound {
    fn per_frame(self) -> u32 {
        match self {
            Sound::Aac { .. } => 1024,
            Sound::Mpeg { per_frame, .. } => per_frame,
            Sound::Ac3 { .. } => 1536,
        }
    }
}

/// One elementary stream being read out.
#[derive(Default)]
struct Elementary {
    kind: u8,
    /// The PES packet being gathered.
    pes: Vec<u8>,
    /// Where its converted samples go.
    out: Option<BufWriter<File>>,
    path: PathBuf,
    written: u64,
    /// Picture: per sample, its offset, size, PTS, DTS and whether it is a
    /// key frame. Sound: per frame, its offset and size.
    samples: Vec<(u64, u32, i64, i64, bool)>,
    vps: Option<Vec<u8>>,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    /// Sound: the bytes not yet a whole frame, and the stream's shape.
    adts: Vec<u8>,
    sound: Option<Sound>,
    first_pts: Option<i64>,
    last_pts: Option<i64>,
}

/// Puts the transport stream at `input` into an MP4 at `out`, with scratch
/// files beside `out` that are gone when it returns. Returns its length.
pub fn to_mp4(input: &Path, out: &Path) -> Result<u64, String> {
    let scratch = |what: &str| {
        let mut name = out.as_os_str().to_owned();
        name.push(format!(".{what}"));
        PathBuf::from(name)
    };
    let mut streams = read_ts(input, &scratch)?;
    let result = build(&mut streams, out);
    for stream in &streams {
        let _ = std::fs::remove_file(&stream.path);
    }
    result
}

/// [`to_mp4`], off the async runtime.
pub async fn to_mp4_async(input: PathBuf, out: PathBuf) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || to_mp4(&input, &out))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

fn read_ts(input: &Path, scratch: &dyn Fn(&str) -> PathBuf) -> Result<Vec<Elementary>, String> {
    let file = File::open(input).map_err(|e| format!("the stream could not be read: {e}"))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut packet = [0u8; PACKET];
    let mut pmt_pids: Vec<u16> = Vec::new();
    // PID → stream, in the order the tables named them.
    let mut streams: Vec<(u16, Elementary)> = Vec::new();
    let mut lost_sync = 0u32;
    loop {
        match reader.read_exact(&mut packet) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("the stream could not be read: {e}")),
        }
        if packet[0] != 0x47 {
            // Out of step: find the next sync byte and carry on from it.
            lost_sync += 1;
            if lost_sync > 1000 {
                return Err("it is not a transport stream".into());
            }
            let Some(at) = packet.iter().skip(1).position(|b| *b == 0x47) else {
                continue;
            };
            let mut rest = vec![0u8; at + 1];
            reader
                .read_exact(&mut rest)
                .map_err(|_| "the stream ends mid-packet".to_string())?;
            let mut shifted = packet.get(at + 1..).unwrap_or_default().to_vec();
            shifted.extend_from_slice(&rest);
            packet.copy_from_slice(&shifted);
            if packet[0] != 0x47 {
                continue;
            }
        }
        let start = packet[1] & 0x40 != 0;
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let control = (packet[3] >> 4) & 3;
        let mut at = 4usize;
        if control & 2 != 0 {
            at += 1 + usize::from(packet[4]);
        }
        if control & 1 == 0 {
            continue;
        }
        let Some(payload) = packet.get(at..).filter(|p| !p.is_empty()) else {
            continue;
        };
        if pid == 0 && start {
            pmt_pids = read_pat(payload);
        } else if pmt_pids.contains(&pid) && start {
            for (kind, es_pid) in read_pmt(payload) {
                if OTHERS.contains(&kind) {
                    for (_, stream) in &streams {
                        let _ = std::fs::remove_file(&stream.path);
                    }
                    return Err(format!(
                        "it carries a kind of picture or sound (stream type {kind:#04x}) dm does \
                         not put into an MP4"
                    ));
                }
                let picture = PICTURES.contains(&kind);
                if !streams.iter().any(|(p, _)| *p == es_pid) && (picture || SOUNDS.contains(&kind))
                {
                    let has_kind = streams
                        .iter()
                        .any(|(_, s)| PICTURES.contains(&s.kind) == picture);
                    // One picture and one sound: the first of each.
                    if !has_kind {
                        let path = scratch(if picture { "video" } else { "audio" });
                        let out = File::create(&path)
                            .map_err(|e| format!("a scratch file could not be made: {e}"))?;
                        streams.push((
                            es_pid,
                            Elementary {
                                kind,
                                out: Some(BufWriter::with_capacity(1 << 20, out)),
                                path,
                                ..Elementary::default()
                            },
                        ));
                    }
                }
            }
        } else if let Some((_, stream)) = streams.iter_mut().find(|(p, _)| *p == pid) {
            if start && !stream.pes.is_empty() {
                let pes = std::mem::take(&mut stream.pes);
                take_pes(stream, &pes)?;
            }
            if start || !stream.pes.is_empty() {
                stream.pes.extend_from_slice(payload);
            }
        }
    }
    let mut out = Vec::new();
    for (_, mut stream) in streams {
        if !stream.pes.is_empty() {
            let pes = std::mem::take(&mut stream.pes);
            take_pes(&mut stream, &pes)?;
        }
        if let Some(mut writer) = stream.out.take() {
            writer
                .flush()
                .map_err(|e| format!("a scratch file could not be written: {e}"))?;
        }
        out.push(stream);
    }
    Ok(out)
}

/// The section a PSI packet's payload carries, past its pointer field.
fn section(payload: &[u8]) -> Option<&[u8]> {
    let pointer = usize::from(*payload.first()?);
    let section = payload.get(1 + pointer..)?;
    let length = usize::from(u16::from_be_bytes([*section.get(1)?, *section.get(2)?]) & 0x0fff);
    section.get(..3 + length)
}

fn read_pat(payload: &[u8]) -> Vec<u16> {
    let Some(section) = section(payload) else {
        return Vec::new();
    };
    // Programs from byte 8 to the CRC.
    let end = section.len().saturating_sub(4);
    section
        .get(8..end)
        .unwrap_or_default()
        .as_chunks::<4>()
        .0
        .iter()
        // Program 0 names the network table, not a program's.
        .filter(|[a, b, _, _]| *a != 0 || *b != 0)
        .map(|[_, _, high, low]| (u16::from(high & 0x1f) << 8) | u16::from(*low))
        .collect()
}

fn read_pmt(payload: &[u8]) -> Vec<(u8, u16)> {
    let Some(section) = section(payload) else {
        return Vec::new();
    };
    let Some(info) = section
        .get(10..12)
        .and_then(|b| b.try_into().ok())
        .map(|b: [u8; 2]| usize::from(u16::from_be_bytes(b) & 0x0fff))
    else {
        return Vec::new();
    };
    let end = section.len().saturating_sub(4);
    let mut at = 12 + info;
    let mut found = Vec::new();
    while at + 5 <= end {
        let (Some(kind), Some(p1), Some(p2), Some(l1), Some(l2)) = (
            section.get(at),
            section.get(at + 1),
            section.get(at + 2),
            section.get(at + 3),
            section.get(at + 4),
        ) else {
            break;
        };
        found.push((*kind, (u16::from(p1 & 0x1f) << 8) | u16::from(*p2)));
        at += 5 + usize::from((u16::from(l1 & 0x0f) << 8) | u16::from(*l2));
    }
    found
}

fn timestamp(b: &[u8]) -> Option<i64> {
    let b: &[u8; 5] = b.get(..5)?.try_into().ok()?;
    Some(
        (i64::from(b[0] >> 1) & 7) << 30
            | i64::from(b[1]) << 22
            | i64::from(b[2] >> 1) << 15
            | i64::from(b[3]) << 7
            | i64::from(b[4] >> 1),
    )
}

/// A timestamp carried past a wrap, so it keeps rising.
fn unwrap(value: i64, last: Option<i64>) -> i64 {
    let Some(last) = last else {
        return value;
    };
    let mut v = value + (last / WRAP) * WRAP;
    if v < last - WRAP / 2 {
        v += WRAP;
    } else if v > last + WRAP / 2 {
        v -= WRAP;
    }
    v
}

/// One whole PES packet: its timestamps, and its payload as samples.
fn take_pes(stream: &mut Elementary, pes: &[u8]) -> Result<(), String> {
    if pes.get(..3) != Some(&[0, 0, 1]) {
        return Ok(());
    }
    let flags = pes.get(7).copied().unwrap_or(0) >> 6;
    let header = usize::from(pes.get(8).copied().unwrap_or(0));
    let pts = if flags & 2 != 0 {
        pes.get(9..).and_then(timestamp)
    } else {
        None
    };
    let dts = if flags == 3 {
        pes.get(14..).and_then(timestamp)
    } else {
        None
    };
    let Some(data) = pes.get(9 + header..) else {
        return Ok(());
    };
    let pts = pts.map(|p| unwrap(p, stream.last_pts));
    let dts = dts.map(|d| unwrap(d, pts.or(stream.last_pts)));
    if let Some(p) = pts {
        stream.last_pts = Some(p);
    }
    if PICTURES.contains(&stream.kind) {
        take_picture(stream, data, pts, dts)
    } else {
        take_sound(stream, data, pts)
    }
}

/// The NAL units in an Annex B byte stream.
fn nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data.get(i..i + 3) == Some(&[0, 0, 1]) {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut units = Vec::new();
    for (n, start) in starts.iter().enumerate() {
        let mut end = starts.get(n + 1).map_or(data.len(), |next| next - 3);
        // A four-byte start code's leading zero, and trailing zeros.
        while end > *start && data.get(end - 1) == Some(&0) {
            end -= 1;
        }
        if let Some(unit) = data.get(*start..end).filter(|u| !u.is_empty()) {
            units.push(unit);
        }
    }
    units
}

fn take_picture(
    stream: &mut Elementary,
    data: &[u8],
    pts: Option<i64>,
    dts: Option<i64>,
) -> Result<(), String> {
    let mut sample = Vec::with_capacity(data.len() + 16);
    let mut key = false;
    let hevc = stream.kind == HEVC;
    for unit in nal_units(data) {
        let Some(head) = unit.first() else {
            continue;
        };
        /* What each unit is: a parameter set (kept for the sample
         * description, not the samples), a delimiter or filler (dropped),
         * or part of the picture, a key frame's or not. */
        let (parameter, dropped, keyframe) = if hevc {
            let kind = (head >> 1) & 0x3f;
            let parameter = match kind {
                32 => Some(&mut stream.vps),
                33 => Some(&mut stream.sps),
                34 => Some(&mut stream.pps),
                _ => None,
            };
            (
                parameter,
                matches!(kind, 35 | 38),
                (16..=21).contains(&kind),
            )
        } else {
            let kind = head & 0x1f;
            let parameter = match kind {
                7 => Some(&mut stream.sps),
                8 => Some(&mut stream.pps),
                _ => None,
            };
            (parameter, matches!(kind, 9 | 12), kind == 5)
        };
        if let Some(slot) = parameter {
            slot.get_or_insert_with(|| unit.to_vec());
            continue;
        }
        if dropped {
            continue;
        }
        key |= keyframe;
        sample.extend_from_slice(&(unit.len() as u32).to_be_bytes());
        sample.extend_from_slice(unit);
    }
    if sample.is_empty() {
        return Ok(());
    }
    let Some(pts) = pts else {
        // A picture with no time is the rest of the one before it.
        return append_to_last(stream, &sample, key);
    };
    write(stream, &sample)?;
    let offset = stream.written - sample.len() as u64;
    stream.first_pts = Some(stream.first_pts.map_or(pts, |f| f.min(pts)));
    stream
        .samples
        .push((offset, sample.len() as u32, pts, dts.unwrap_or(pts), key));
    Ok(())
}

fn append_to_last(stream: &mut Elementary, sample: &[u8], key: bool) -> Result<(), String> {
    if stream.samples.is_empty() {
        return Ok(());
    }
    write(stream, sample)?;
    if let Some(last) = stream.samples.last_mut() {
        last.1 += sample.len() as u32;
        last.4 |= key;
    }
    Ok(())
}

fn write(stream: &mut Elementary, bytes: &[u8]) -> Result<(), String> {
    let out = stream.out.as_mut().ok_or("a scratch file went missing")?;
    out.write_all(bytes)
        .map_err(|e| format!("a scratch file could not be written: {e}"))?;
    stream.written += bytes.len() as u64;
    Ok(())
}

fn take_sound(stream: &mut Elementary, data: &[u8], pts: Option<i64>) -> Result<(), String> {
    if stream.first_pts.is_none() {
        stream.first_pts = pts;
    }
    stream.adts.extend_from_slice(data);
    let mut at = 0;
    let buffered = std::mem::take(&mut stream.adts);
    let reader = match stream.kind {
        AAC => adts_frame,
        AC3 => ac3_frame,
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
enum Frame {
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
fn adts_frame(rest: &[u8]) -> Frame {
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
fn mpeg_audio_frame(rest: &[u8]) -> Frame {
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

/// An AC-3 sync frame, whole.
fn ac3_frame(rest: &[u8]) -> Frame {
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

/// Reads an H.264 sequence parameter set's picture size.
fn picture_size(sps: &[u8]) -> Option<(u32, u32)> {
    // Past the one-byte NAL header, emulation prevention bytes out.
    let data = rbsp(sps.get(1..)?);
    let mut bits = Bits { data: &data, at: 0 };
    let profile = bits.take(8)?;
    bits.take(16)?; // constraints, level
    bits.ue()?; // seq_parameter_set_id
    let mut chroma = 1;
    let mut separate = false;
    if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].contains(&profile) {
        chroma = bits.ue()?;
        if chroma == 3 {
            separate = bits.take(1)? == 1;
        }
        bits.ue()?;
        bits.ue()?;
        bits.take(1)?;
        if bits.take(1)? == 1 {
            for i in 0..if chroma == 3 { 12 } else { 8 } {
                if bits.take(1)? == 1 {
                    let size = if i < 6 { 16 } else { 64 };
                    let (mut last, mut next) = (8i64, 8i64);
                    for _ in 0..size {
                        if next != 0 {
                            next = (last + bits.se()? + 256) % 256;
                        }
                        if next != 0 {
                            last = next;
                        }
                    }
                }
            }
        }
    }
    bits.ue()?; // log2_max_frame_num
    match bits.ue()? {
        0 => {
            bits.ue()?;
        }
        1 => {
            bits.take(1)?;
            bits.se()?;
            bits.se()?;
            for _ in 0..bits.ue()?.min(256) {
                bits.se()?;
            }
        }
        _ => {}
    }
    bits.ue()?; // max_num_ref_frames
    bits.take(1)?;
    let width_mbs = bits.ue()? + 1;
    let height_units = bits.ue()? + 1;
    let frame_only = bits.take(1)?;
    if frame_only == 0 {
        bits.take(1)?;
    }
    bits.take(1)?;
    let (mut left, mut right, mut top, mut bottom) = (0, 0, 0, 0);
    if bits.take(1)? == 1 {
        left = bits.ue()?;
        right = bits.ue()?;
        top = bits.ue()?;
        bottom = bits.ue()?;
    }
    let array_type = if separate { 0 } else { chroma };
    let (sub_w, sub_h) = match array_type {
        1 => (2, 2),
        2 => (2, 1),
        _ => (1, 1),
    };
    let (unit_x, unit_y) = if array_type == 0 {
        (1, 2 - frame_only)
    } else {
        (sub_w, sub_h * (2 - frame_only))
    };
    let width = (width_mbs * 16).checked_sub((left + right) * unit_x)?;
    let height = ((2 - frame_only) * height_units * 16).checked_sub((top + bottom) * unit_y)?;
    Some((width, height))
}

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn take(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.data.get(self.at / 8)?;
            v = (v << 1) | u32::from((byte >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Some(v)
    }

    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.take(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1u32 << zeros) - 1 + self.take(zeros)?)
    }

    fn se(&mut self) -> Option<i64> {
        let v = i64::from(self.ue()?);
        Some(if v % 2 == 1 { (v + 1) / 2 } else { -(v / 2) })
    }
}

fn hdlr(kind: &[u8; 4], name: &str) -> Vec<u8> {
    let mut body = vec![0u8; 4];
    body.extend_from_slice(kind);
    body.extend_from_slice(&[0u8; 12]);
    body.extend_from_slice(name.as_bytes());
    body.push(0);
    full(b"hdlr", 0, 0, &body)
}

fn tkhd_tail(volume: u16, width: u32, height: u32) -> Vec<u8> {
    let mut tail = vec![0u8; 8];
    tail.extend_from_slice(&0u16.to_be_bytes());
    tail.extend_from_slice(&0u16.to_be_bytes());
    tail.extend_from_slice(&volume.to_be_bytes());
    tail.extend_from_slice(&0u16.to_be_bytes());
    for m in MATRIX {
        tail.extend_from_slice(&m.to_be_bytes());
    }
    tail.extend_from_slice(&(width << 16).to_be_bytes());
    tail.extend_from_slice(&(height << 16).to_be_bytes());
    tail
}

/// An RBSP: a NAL unit's payload with its emulation prevention bytes out.
fn rbsp(unit: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(unit.len());
    let mut zeros = 0;
    for b in unit {
        if zeros >= 2 && *b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if *b == 0 { zeros + 1 } else { 0 };
        out.push(*b);
    }
    out
}

/// What an HEVC sequence parameter set says that its `hvcC` repeats: the
/// profile, tier and level bytes as they are, the sub-layers, the chroma
/// format and bit depths, and the picture's size.
struct HevcSps {
    profile: Vec<u8>,
    sub_layers: u32,
    nested: bool,
    chroma: u32,
    luma_bits: u32,
    chroma_bits: u32,
    width: u32,
    height: u32,
}

fn hevc_sps(sps: &[u8]) -> Option<HevcSps> {
    // Past the two-byte NAL header.
    let data = rbsp(sps.get(2..)?);
    let mut bits = Bits { data: &data, at: 0 };
    bits.take(4)?;
    let max_sub_layers = bits.take(3)?;
    let nested = bits.take(1)? == 1;
    // general_profile_space … general_level_idc: twelve whole bytes.
    let profile = data.get(1..13)?.to_vec();
    bits.at = 13 * 8;
    let mut present = Vec::new();
    for _ in 0..max_sub_layers {
        present.push((bits.take(1)?, bits.take(1)?));
    }
    if max_sub_layers > 0 {
        for _ in max_sub_layers..8 {
            bits.take(2)?;
        }
    }
    for (profile_present, level_present) in present {
        if profile_present == 1 {
            bits.at += 88;
        }
        if level_present == 1 {
            bits.at += 8;
        }
    }
    bits.ue()?;
    let chroma = bits.ue()?;
    if chroma == 3 {
        bits.take(1)?;
    }
    let mut width = bits.ue()?;
    let mut height = bits.ue()?;
    if bits.take(1)? == 1 {
        let (sub_w, sub_h) = match chroma {
            1 => (2, 2),
            2 => (2, 1),
            _ => (1, 1),
        };
        let (left, right, top, bottom) = (bits.ue()?, bits.ue()?, bits.ue()?, bits.ue()?);
        width = width.checked_sub((left + right) * sub_w)?;
        height = height.checked_sub((top + bottom) * sub_h)?;
    }
    let luma_bits = bits.ue()? + 8;
    let chroma_bits = bits.ue()? + 8;
    Some(HevcSps {
        profile,
        sub_layers: max_sub_layers + 1,
        nested,
        chroma,
        luma_bits,
        chroma_bits,
        width,
        height,
    })
}

/// An HEVC picture's sample entry: `hvc1`, its parameter sets in `hvcC`
/// (and none in the samples), and its size.
fn hevc_entry(stream: &Elementary) -> Result<(Vec<u8>, u32, u32), String> {
    let (Some(vps), Some(sps), Some(pps)) = (&stream.vps, &stream.sps, &stream.pps) else {
        return Err("its pictures have no parameter sets".into());
    };
    let info = hevc_sps(sps).ok_or("its sequence parameter set could not be read")?;
    let mut hvcc = vec![1];
    hvcc.extend_from_slice(&info.profile);
    hvcc.extend_from_slice(&[0xf0, 0x00, 0xfc]);
    hvcc.push(0xfc | (info.chroma & 3) as u8);
    hvcc.push(0xf8 | (info.luma_bits.saturating_sub(8) & 7) as u8);
    hvcc.push(0xf8 | (info.chroma_bits.saturating_sub(8) & 7) as u8);
    hvcc.extend_from_slice(&[0, 0]);
    hvcc.push((((info.sub_layers & 7) << 3) as u8) | (u8::from(info.nested) << 2) | 3);
    hvcc.push(3);
    for (kind, unit) in [(32u8, vps), (33, sps), (34, pps)] {
        hvcc.push(0x80 | kind);
        hvcc.extend_from_slice(&1u16.to_be_bytes());
        hvcc.extend_from_slice(&(unit.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(unit);
    }
    Ok((boxed(b"hvcC", &[&hvcc]), info.width, info.height))
}

fn video_track(stream: &Elementary, input: usize, lead: i64) -> Result<Track, String> {
    let (config, codec, width, height) = if stream.kind == HEVC {
        let (hvcc, width, height) = hevc_entry(stream)?;
        (hvcc, *b"hvc1", width, height)
    } else {
        let (avcc, width, height) = avc_entry(stream)?;
        (avcc, *b"avc1", width, height)
    };
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 16]);
    entry.extend_from_slice(&(width as u16).to_be_bytes());
    entry.extend_from_slice(&(height as u16).to_be_bytes());
    entry.extend_from_slice(&0x0048_0000u32.to_be_bytes());
    entry.extend_from_slice(&0x0048_0000u32.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 32]);
    entry.extend_from_slice(&0x0018u16.to_be_bytes());
    entry.extend_from_slice(&0xffffu16.to_be_bytes());
    let sample_entry = boxed(&codec, &[&entry, &config]);
    let stsd = full(
        b"stsd",
        0,
        0,
        &[&1u32.to_be_bytes()[..], &sample_entry].concat(),
    );
    picture_track(stream, input, lead, stsd, width, height)
}

/// An H.264 picture's `avcC` and size.
fn avc_entry(stream: &Elementary) -> Result<(Vec<u8>, u32, u32), String> {
    let (Some(sps), Some(pps)) = (&stream.sps, &stream.pps) else {
        return Err("its pictures have no parameter sets".into());
    };
    let (width, height) = picture_size(sps).unwrap_or((0, 0));
    let mut avcc = vec![
        1,
        sps.get(1).copied().unwrap_or(0),
        sps.get(2).copied().unwrap_or(0),
        sps.get(3).copied().unwrap_or(0),
        0xff,
        0xe1,
    ];
    avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    avcc.extend_from_slice(sps);
    avcc.push(1);
    avcc.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    avcc.extend_from_slice(pps);
    Ok((boxed(b"avcC", &[&avcc]), width, height))
}

fn picture_track(
    stream: &Elementary,
    input: usize,
    lead: i64,
    stsd: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<Track, String> {
    let first_dts = stream.samples.first().map_or(0, |s| s.3);
    let mut durations: Vec<u32> = stream
        .samples
        .windows(2)
        .map(|w| match w {
            [a, b] => u32::try_from(b.3 - a.3).unwrap_or(0),
            _ => 0,
        })
        .collect();
    // A picture's length where its timestamps do not say (the last, or a
    // stamp out of step): the most common one.
    let usual = {
        let mut sorted = durations.clone();
        sorted.sort_unstable();
        sorted
            .get(sorted.len() / 2)
            .copied()
            .filter(|d| *d > 0)
            .unwrap_or(3000)
    };
    for d in &mut durations {
        if *d == 0 || *d > usual * 10 {
            *d = usual;
        }
    }
    durations.push(usual);
    let samples = stream
        .samples
        .iter()
        .zip(durations)
        .map(|(s, duration)| Sample {
            offset: s.0,
            size: s.1,
            duration,
            cts: i32::try_from(s.2 - s.3).unwrap_or(0),
            sync: s.4,
        })
        .collect();
    let shown = stream.first_pts.unwrap_or(first_dts) - first_dts;
    let mut edits = Vec::new();
    if lead > 0 {
        edits.push(Edit {
            duration: lead as u64,
            media_time: -1,
        });
    }
    edits.push(Edit {
        duration: 0,
        media_time: shown.max(0),
    });
    Ok(Track {
        input,
        id: 1,
        tkhd_tail: tkhd_tail(0, width, height),
        timescale: TIMESCALE,
        language: 0x55c4,
        hdlr: hdlr(b"vide", "VideoHandler"),
        media_header: full(b"vmhd", 0, 1, &[0u8; 8]),
        stsd,
        edits,
        edits_timescale: TIMESCALE,
        samples,
    })
}

const RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag, body.len() as u8];
    out.extend_from_slice(body);
    out
}

fn audio_track(stream: &Elementary, input: usize, lead: i64) -> Result<Track, String> {
    let shape = stream.sound.ok_or("its sound has no frames")?;
    let es = |object: u8, specific: Option<&[u8]>| {
        let mut decoder = vec![object, 0x15, 0, 0, 0];
        decoder.extend_from_slice(&0u32.to_be_bytes());
        decoder.extend_from_slice(&0u32.to_be_bytes());
        if let Some(specific) = specific {
            decoder.extend(descriptor(5, specific));
        }
        let mut es = vec![0, 0, 0];
        es.extend(descriptor(4, &decoder));
        es.extend(descriptor(6, &[2]));
        full(b"esds", 0, 0, &descriptor(3, &es))
    };
    let (codec, config, rate, channels) = match shape {
        Sound::Aac {
            profile,
            rate_index,
            channels,
        } => {
            let rate = *RATES
                .get(usize::from(rate_index))
                .ok_or("its sound has an unknown sample rate")?;
            let specific = (u16::from(profile + 1) << 11)
                | (u16::from(rate_index) << 7)
                | (u16::from(channels) << 3);
            (
                *b"mp4a",
                es(0x40, Some(&specific.to_be_bytes())),
                rate,
                u16::from(channels),
            )
        }
        // MPEG-1 audio is object type 0x6B; MPEG-2's (and 2.5's) 0x69.
        Sound::Mpeg {
            mpeg1,
            rate,
            channels,
            ..
        } => (
            *b"mp4a",
            es(if mpeg1 { 0x6b } else { 0x69 }, None),
            rate,
            channels,
        ),
        Sound::Ac3 {
            dac3,
            rate,
            channels,
        } => (*b"ac-3", boxed(b"dac3", &[&dac3]), rate, channels),
    };
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 8]);
    entry.extend_from_slice(&channels.clamp(1, 2).to_be_bytes());
    entry.extend_from_slice(&16u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 4]);
    entry.extend_from_slice(&(rate << 16).to_be_bytes());
    let sample_entry = boxed(&codec, &[&entry, &config]);
    let stsd = full(
        b"stsd",
        0,
        0,
        &[&1u32.to_be_bytes()[..], &sample_entry].concat(),
    );
    let per_frame = shape.per_frame();
    let samples = stream
        .samples
        .iter()
        .map(|s| Sample {
            offset: s.0,
            size: s.1,
            duration: per_frame,
            cts: 0,
            sync: true,
        })
        .collect();
    let mut edits = Vec::new();
    if lead > 0 {
        edits.push(Edit {
            duration: lead as u64,
            media_time: -1,
        });
    }
    edits.push(Edit {
        duration: 0,
        media_time: 0,
    });
    Ok(Track {
        input,
        id: 2,
        tkhd_tail: tkhd_tail(0x0100, 0, 0),
        timescale: rate,
        language: 0x55c4,
        hdlr: hdlr(b"soun", "SoundHandler"),
        media_header: full(b"smhd", 0, 0, &[0u8; 4]),
        stsd,
        edits,
        edits_timescale: TIMESCALE,
        samples,
    })
}

fn build(streams: &mut [Elementary], out: &Path) -> Result<u64, String> {
    let video = streams
        .iter()
        .position(|s| PICTURES.contains(&s.kind) && !s.samples.is_empty());
    let audio = streams
        .iter()
        .position(|s| SOUNDS.contains(&s.kind) && !s.samples.is_empty());
    if video.is_none() && audio.is_none() {
        return Err("it holds no picture or sound dm can put into an MP4".into());
    }
    // Picture and sound start together: whichever starts later waits.
    let start = |i: Option<usize>| i.and_then(|i| streams.get(i)).and_then(|s| s.first_pts);
    let earliest = [start(video), start(audio)]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(0);
    let mut tracks = Vec::new();
    let mut files = Vec::new();
    for (index, make) in [
        (
            video,
            video_track as fn(&Elementary, usize, i64) -> Result<Track, String>,
        ),
        (audio, audio_track),
    ] {
        let Some(stream) = index.and_then(|i| streams.get(i)) else {
            continue;
        };
        let lead = stream.first_pts.unwrap_or(earliest) - earliest;
        tracks.push(make(stream, files.len(), lead)?);
        files.push(
            File::open(&stream.path)
                .map_err(|e| format!("a scratch file could not be read: {e}"))?,
        );
    }
    crate::mp4mux::write_output(&tracks, &mut files, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_keep_rising_across_a_wrap() {
        assert_eq!(unwrap(100, None), 100);
        assert_eq!(unwrap(10, Some(WRAP - 10)), WRAP + 10);
        assert_eq!(unwrap(WRAP - 10, Some(WRAP + 5)), WRAP - 10);
    }

    #[test]
    fn a_byte_stream_splits_into_its_units() {
        let data = [
            0, 0, 0, 1, 9, 0xf0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x65, 7, 7, 0,
        ];
        let units = nal_units(&data);
        assert_eq!(units, [&[9, 0xf0][..], &[0x67, 1, 2], &[0x65, 7, 7]]);
    }

    #[test]
    fn what_is_not_a_transport_stream_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("page.ts");
        std::fs::write(&input, vec![b'x'; 188 * 1200]).unwrap();
        assert!(to_mp4(&input, &dir.path().join("out.mp4")).is_err());
        let empty = dir.path().join("empty.ts");
        std::fs::write(&empty, b"").unwrap();
        assert!(to_mp4(&empty, &dir.path().join("out.mp4")).is_err());
        // Nothing left behind.
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert_eq!(left.len(), 2);
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..2000)) {
            let _ = picture_size(&data);
            let _ = read_pat(&data);
            let _ = read_pmt(&data);
            let _ = nal_units(&data);
            let mut stream = Elementary { kind: AAC, ..Elementary::default() };
            let _ = take_pes(&mut stream, &data);
            let mut stream = Elementary { kind: H264, ..Elementary::default() };
            let _ = take_pes(&mut stream, &data);
        }

        #[test]
        fn any_packets_are_read_without_a_panic(seed in proptest::collection::vec(proptest::num::u8::ANY, 0..188 * 6)) {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("in.ts");
            let mut data = seed.clone();
            for chunk in data.chunks_mut(188) {
                chunk[0] = 0x47;
            }
            std::fs::write(&input, &data).unwrap();
            let _ = to_mp4(&input, &dir.path().join("out.mp4"));
        }
    }
}
