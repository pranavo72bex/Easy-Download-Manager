//! Puts an HLS stream's MPEG transport stream (`.ts`) into an MP4, without
//! ffmpeg.
//!
//! Players open an MP4; few open a `.ts`. An HLS stream's segments are a
//! transport stream: 188-byte packets carrying H.264 pictures and AAC sound
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
const AAC: u8 = 0x0f;
/// Picture and sound kinds this does not carry: MPEG-1/2 and MPEG-4 Part 2
/// pictures, HEVC; MPEG audio, LATM AAC, AC-3, E-AC-3. A stream with one is
/// refused whole rather than put into an MP4 without it.
const OTHERS: [u8; 10] = [0x01, 0x02, 0x10, 0x24, 0x03, 0x04, 0x11, 0x81, 0x87, 0x42];

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
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    /// Sound: the ADTS bytes not yet a whole frame, and the stream's shape.
    adts: Vec<u8>,
    audio_config: Option<(u8, u8, u8)>,
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
                if !streams.iter().any(|(p, _)| *p == es_pid) && (kind == H264 || kind == AAC) {
                    let has_kind = streams.iter().any(|(_, s)| s.kind == kind);
                    // One picture and one sound: the first of each.
                    if !has_kind {
                        let path = scratch(if kind == H264 { "video" } else { "audio" });
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
    match stream.kind {
        H264 => take_picture(stream, data, pts, dts),
        _ => take_sound(stream, data, pts),
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
    for unit in nal_units(data) {
        match unit.first().map(|h| h & 0x1f) {
            Some(7) => {
                stream.sps.get_or_insert_with(|| unit.to_vec());
            }
            Some(8) => {
                stream.pps.get_or_insert_with(|| unit.to_vec());
            }
            // Access unit delimiters and filler: not part of an MP4 sample.
            Some(9) | Some(12) => {}
            Some(kind) => {
                if kind == 5 {
                    key = true;
                }
                sample.extend_from_slice(&(unit.len() as u32).to_be_bytes());
                sample.extend_from_slice(unit);
            }
            None => {}
        }
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
    while let Some(head) = buffered
        .get(at..at + 7)
        .and_then(|h| <[u8; 7]>::try_from(h).ok())
    {
        if head[0] != 0xff || head[1] & 0xf0 != 0xf0 {
            // Not on a frame: step to the next sync word.
            at += 1;
            continue;
        }
        let protection_absent = head[1] & 1 != 0;
        let profile = head[2] >> 6;
        let rate = (head[2] >> 2) & 0x0f;
        let channels = ((head[2] & 1) << 2) | (head[3] >> 6);
        let length = (usize::from(head[3] & 3) << 11)
            | (usize::from(head[4]) << 3)
            | (usize::from(head[5]) >> 5);
        let header = if protection_absent { 7 } else { 9 };
        if length <= header {
            at += 1;
            continue;
        }
        let Some(frame) = buffered.get(at + header..at + length) else {
            break;
        };
        stream.audio_config.get_or_insert((profile, rate, channels));
        let frame = frame.to_vec();
        write(stream, &frame)?;
        let offset = stream.written - frame.len() as u64;
        stream
            .samples
            .push((offset, frame.len() as u32, 0, 0, true));
        at += length;
    }
    stream.adts = buffered.get(at..).unwrap_or_default().to_vec();
    Ok(())
}

// ---- the MP4 tracks -----------------------------------------------------

/// Reads an H.264 sequence parameter set's picture size.
fn picture_size(sps: &[u8]) -> Option<(u32, u32)> {
    // Emulation prevention bytes out: 00 00 03 is 00 00.
    let mut rbsp = Vec::with_capacity(sps.len());
    let mut zeros = 0;
    for b in sps.iter().skip(1) {
        if zeros >= 2 && *b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if *b == 0 { zeros + 1 } else { 0 };
        rbsp.push(*b);
    }
    let mut bits = Bits { data: &rbsp, at: 0 };
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

fn video_track(stream: &Elementary, input: usize, lead: i64) -> Result<Track, String> {
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
    let avc1 = boxed(b"avc1", &[&entry, &boxed(b"avcC", &[&avcc])]);
    let stsd = full(b"stsd", 0, 0, &[&1u32.to_be_bytes()[..], &avc1].concat());

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
    let (profile, rate_index, channels) = stream.audio_config.ok_or("its sound has no frames")?;
    let rate = *RATES
        .get(usize::from(rate_index))
        .ok_or("its sound has an unknown sample rate")?;
    let config =
        (u16::from(profile + 1) << 11) | (u16::from(rate_index) << 7) | (u16::from(channels) << 3);
    let mut decoder = vec![0x40, 0x15, 0, 0, 0];
    decoder.extend_from_slice(&0u32.to_be_bytes());
    decoder.extend_from_slice(&0u32.to_be_bytes());
    decoder.extend(descriptor(5, &config.to_be_bytes()));
    let mut es = vec![0, 0, 0];
    es.extend(descriptor(4, &decoder));
    es.extend(descriptor(6, &[2]));
    let esds = full(b"esds", 0, 0, &descriptor(3, &es));
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 8]);
    entry.extend_from_slice(&u16::from(channels.max(1)).to_be_bytes());
    entry.extend_from_slice(&16u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 4]);
    entry.extend_from_slice(&(rate << 16).to_be_bytes());
    let mp4a = boxed(b"mp4a", &[&entry, &esds]);
    let stsd = full(b"stsd", 0, 0, &[&1u32.to_be_bytes()[..], &mp4a].concat());
    let samples = stream
        .samples
        .iter()
        .map(|s| Sample {
            offset: s.0,
            size: s.1,
            duration: 1024,
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
        .position(|s| s.kind == H264 && !s.samples.is_empty());
    let audio = streams
        .iter()
        .position(|s| s.kind == AAC && !s.samples.is_empty());
    if video.is_none() && audio.is_none() {
        return Err("it holds no H.264 picture or AAC sound dm can put into an MP4".into());
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
