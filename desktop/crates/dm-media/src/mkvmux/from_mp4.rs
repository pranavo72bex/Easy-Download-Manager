// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An MP4's tracks read as a Matroska file's, so the writer copies them as
//! it copies a WebM's: H.264, HEVC, AV1, VP9 and VP8 pictures; AAC, AC-3,
//! E-AC-3, MPEG audio and Opus sound. Each sample becomes one block, its
//! bytes unchanged: the length-prefixed units an MP4 keeps are what
//! Matroska keeps too. What the decoder is set up with goes in the track's
//! `CodecPrivate`: the `avcC`, `hvcC` or `av1C` record, the AAC
//! AudioSpecificConfig from `esds`, an `OpusHead` made from `dOps`, and
//! nothing for AC-3.
//!
//! A block's time is when its sample shows, from the sample tables: the
//! durations before it (`stts`, or a fragment's `trun`) from where its
//! fragments say it starts (`tfdt`), its composition offset (`ctts`), less
//! where the edit list starts the track (an AAC encoder's priming, a
//! picture's reordering delay). Opus keeps its delay in the block times and
//! says it as `CodecDelay`, as Matroska has it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::{element, uint_element, Block, Input, TrackIn, CODEC_ID, TRACK_TYPE};
use crate::mp4mux::{boxes, child, Track};

const CODEC_PRIVATE: u32 = 0x63A2;
const LANGUAGE: u32 = 0x22_B59C;
const FLAG_LACING: u32 = 0x9C;
const CODEC_DELAY: u32 = 0x56AA;
const SEEK_PRE_ROLL: u32 = 0x56BB;
const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const AUDIO: u32 = 0xE1;
const SAMPLING_FREQUENCY: u32 = 0xB5;
const CHANNELS: u32 = 0x9F;

const NS_PER_SECOND: i128 = 1_000_000_000;
/// How far before a seek an Opus decoder starts, as Matroska advises.
const OPUS_PRE_ROLL_NS: u64 = 80_000_000;

/// A track's sample description as Matroska names it.
#[derive(Debug, Clone, PartialEq)]
struct Described {
    codec: &'static str,
    private: Option<Vec<u8>>,
    /// A picture's width and height.
    size: Option<(u64, u64)>,
    /// A sound's sampling rate and channels.
    sound: Option<(f64, u64)>,
    /// Opus's pre-skip, in 48 kHz samples: said as `CodecDelay`, its block
    /// times not moved by the edit list that says it in an MP4.
    opus_delay: Option<u64>,
}

/// The MP4 at `path` as input `input` of a join: its picture and sound
/// tracks, each sample a block whose time is in nanoseconds.
pub(super) fn read(path: &Path, input: usize) -> Result<Input, String> {
    let tracks = crate::mp4mux::tracks_of_input(path, input)?;
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut blocks = Vec::new();
    let mut origin: Option<i64> = None;
    for track in &tracks {
        let kind = match track.hdlr.get(16..20) {
            Some(b"vide") => 1,
            Some(b"soun") => 2,
            _ => continue,
        };
        if track.samples.is_empty() || track.id == 0 {
            continue;
        }
        let (fourcc, entry) =
            first_entry(&track.stsd).ok_or("a track's sample description is unreadable")?;
        let head = track
            .samples
            .first()
            .map(|s| first_bytes(&mut file, s.offset, s.size.min(4)))
            .unwrap_or_default();
        let described = describe(&fourcc, entry, kind, track.timescale, &head)?;
        let (times, start) = block_times(track, described.opus_delay.is_some());
        if let Some(start) = start {
            origin = Some(origin.map_or(start, |o| o.min(start)));
        }
        let number = u64::from(track.id);
        for (sample, (shows, decodes)) in track.samples.iter().zip(times) {
            blocks.push(Block {
                input,
                track: number,
                ticks: shows,
                decode: decodes,
                key: sample.sync,
                // A SimpleBlock's key-frame flag; no lacing.
                flags: if sample.sync { 0x80 } else { 0 },
                data_at: sample.offset,
                data_len: u64::from(sample.size),
                group: None,
            });
        }
        out.push(TrackIn {
            number,
            kind,
            codec: described.codec.to_string(),
            rest: entry_elements(kind, &described, track.language),
        });
    }
    if out.is_empty() {
        return Err("it has no picture or sound".into());
    }
    Ok(Input {
        scale: 1,
        tracks: out,
        blocks,
        origin,
    })
}

/// Up to `len` bytes at `at` in `file`; none where they cannot be read.
fn first_bytes(file: &mut File, at: u64, len: u32) -> Vec<u8> {
    let mut buf = vec![0u8; len as usize];
    match file
        .seek(SeekFrom::Start(at))
        .and_then(|_| file.read_exact(&mut buf))
    {
        Ok(()) => buf,
        Err(_) => Vec::new(),
    }
}

/// A whole `stsd` box's first sample entry: its type and body.
fn first_entry(stsd: &[u8]) -> Option<([u8; 4], &[u8])> {
    // The box header, then the full box's version and flags and the count.
    let entries = stsd.get(16..)?;
    boxes(entries).next().map(|(kind, body, _)| (kind, body))
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn be64(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// A sound sample entry's rate, channels and the boxes after its fields,
/// whichever of the three layouts it has (by its version at 8). A rate it
/// does not say is the track's timescale, which is the rate in practice.
fn sound_entry(entry: &[u8], timescale: u32) -> (f64, u64, &[u8]) {
    let fallback = f64::from(timescale);
    match be16(entry, 8).unwrap_or(0) {
        2 => {
            let rate = be64(entry, 32)
                .map(f64::from_bits)
                .filter(|r| r.is_finite() && *r > 0.0)
                .unwrap_or(fallback);
            let channels = be32(entry, 40).map_or(2, u64::from);
            (rate, channels, entry.get(64..).unwrap_or_default())
        }
        version => {
            let rate = be32(entry, 24)
                .map(|r| r >> 16)
                .filter(|r| *r > 0)
                .map_or(fallback, f64::from);
            let channels = be16(entry, 16).map_or(2, u64::from);
            let fields = if version == 1 { 44 } else { 28 };
            (rate, channels, entry.get(fields..).unwrap_or_default())
        }
    }
}

/// One descriptor in an `esds`: its tag, its body and what follows it.
fn descriptor(data: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *data.first()?;
    let mut at = 1;
    let mut length = 0usize;
    // The length in up to four bytes of seven bits, the top bit "more".
    for _ in 0..4 {
        let byte = *data.get(at)?;
        at += 1;
        length = (length << 7) | usize::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            break;
        }
    }
    let end = at.checked_add(length)?;
    Some((tag, data.get(at..end)?, data.get(end..)?))
}

/// An `esds` box body's object type and its decoder-specific info (for
/// AAC, the AudioSpecificConfig), where it has one.
fn esds_config(esds: &[u8]) -> Option<(u8, Option<&[u8]>)> {
    let (tag, es, _) = descriptor(esds.get(4..)?)?;
    if tag != 3 {
        return None;
    }
    // The ES descriptor's ID, then its flags and what they say follows.
    let flags = *es.get(2)?;
    let mut at = 3;
    if flags & 0x80 != 0 {
        at += 2;
    }
    if flags & 0x40 != 0 {
        at += 1 + usize::from(*es.get(at)?);
    }
    if flags & 0x20 != 0 {
        at += 2;
    }
    let mut rest = es.get(at..)?;
    while let Some((tag, body, after)) = descriptor(rest) {
        if tag == 4 {
            let object = *body.first()?;
            // The decoder config's fixed fields, then its own descriptors.
            let mut inner = body.get(13..).unwrap_or_default();
            while let Some((tag, info, after)) = descriptor(inner) {
                if tag == 5 {
                    return Some((object, Some(info)));
                }
                inner = after;
            }
            return Some((object, None));
        }
        rest = after;
    }
    None
}

/// MPEG audio's Matroska codec by the layer its first frame header says;
/// layer III where there is no header to read.
fn mpeg_audio(head: &[u8]) -> &'static str {
    match head {
        [0xFF, second, ..] if second & 0xE0 == 0xE0 => match (second >> 1) & 3 {
            3 => "A_MPEG/L1",
            2 => "A_MPEG/L2",
            _ => "A_MPEG/L3",
        },
        _ => "A_MPEG/L3",
    }
}

/// What the sample entry `fourcc` with body `entry` is, as Matroska names
/// it, with what its decoder is set up with. `head` is the first sample's
/// first bytes (MPEG audio says its layer there).
fn describe(
    fourcc: &[u8; 4],
    entry: &[u8],
    kind: u64,
    timescale: u32,
    head: &[u8],
) -> Result<Described, String> {
    let named = String::from_utf8_lossy(fourcc).into_owned();
    if kind == 1 {
        // A visual sample entry's fields are 78 bytes, its boxes after.
        let boxes_of = entry.get(78..).unwrap_or_default();
        let config = |kind: &[u8; 4]| {
            child(boxes_of, kind)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| format!("its {named} picture has no configuration"))
        };
        let (codec, private) = match fourcc {
            b"avc1" | b"avc3" => ("V_MPEG4/ISO/AVC", Some(config(b"avcC")?)),
            b"hvc1" | b"hev1" => ("V_MPEGH/ISO/HEVC", Some(config(b"hvcC")?)),
            b"av01" => ("V_AV1", Some(config(b"av1C")?)),
            b"vp09" => ("V_VP9", None),
            b"vp08" => ("V_VP8", None),
            _ => return Err(format!("dm does not put {named} into an MKV")),
        };
        let width = be16(entry, 24).unwrap_or(0);
        let height = be16(entry, 26).unwrap_or(0);
        return Ok(Described {
            codec,
            private,
            size: Some((u64::from(width), u64::from(height))),
            sound: None,
            opus_delay: None,
        });
    }
    let (rate, channels, boxes_of) = sound_entry(entry, timescale);
    let sound = |codec, private| Described {
        codec,
        private,
        size: None,
        sound: Some((rate, channels)),
        opus_delay: None,
    };
    match fourcc {
        b"mp4a" => {
            let (object, info) = child(boxes_of, b"esds")
                .and_then(esds_config)
                .ok_or("its sound has no decoder configuration")?;
            match object {
                // MPEG-4 AAC, and MPEG-2's three AAC profiles.
                0x40 | 0x66..=0x68 => {
                    let info = info
                        .filter(|i| !i.is_empty())
                        .ok_or("its AAC has no AudioSpecificConfig")?;
                    Ok(sound("A_AAC", Some(info.to_vec())))
                }
                0x69 | 0x6B => Ok(sound(mpeg_audio(head), None)),
                other => Err(format!(
                    "dm does not put sound of object type {other:#04x} into an MKV"
                )),
            }
        }
        b".mp3" => Ok(sound("A_MPEG/L3", None)),
        b"ac-3" => Ok(sound("A_AC3", None)),
        b"ec-3" => Ok(sound("A_EAC3", None)),
        b"Opus" => {
            // dOps: version, channels, pre-skip, input rate, gain (all
            // big-endian), then the mapping family and its table. OpusHead
            // is the same fields little-endian after its magic.
            let dops = child(boxes_of, b"dOps")
                .filter(|d| d.len() >= 11)
                .ok_or("its Opus has no configuration")?;
            let byte = |i: usize| dops.get(i).copied().unwrap_or(0);
            let opus_channels = byte(1);
            let pre_skip = u16::from_be_bytes([byte(2), byte(3)]);
            let input_rate = u32::from_be_bytes([byte(4), byte(5), byte(6), byte(7)]);
            let gain = i16::from_be_bytes([byte(8), byte(9)]);
            let mut head = b"OpusHead".to_vec();
            head.push(1);
            head.push(opus_channels);
            head.extend_from_slice(&pre_skip.to_le_bytes());
            head.extend_from_slice(&input_rate.to_le_bytes());
            head.extend_from_slice(&gain.to_le_bytes());
            head.extend_from_slice(dops.get(10..).unwrap_or_default());
            Ok(Described {
                codec: "A_OPUS",
                private: Some(head),
                size: None,
                // Opus always decodes at 48 kHz.
                sound: Some((48_000.0, u64::from(opus_channels.max(1)))),
                opus_delay: Some(u64::from(pre_skip)),
            })
        }
        _ => Err(format!("dm does not put {named} into an MKV")),
    }
}

/// An MP4's packed language code ("und" is 0x55c4) as its three letters.
fn language(packed: u16) -> Option<[u8; 3]> {
    let letter = |shift: u16| ((packed >> shift) & 0x1f) as u8 + 0x60;
    let code = [letter(10), letter(5), letter(0)];
    code.iter().all(u8::is_ascii_lowercase).then_some(code)
}

/// The track entry's elements but its number and UID.
fn entry_elements(kind: u64, described: &Described, packed_language: u16) -> Vec<u8> {
    let mut rest = uint_element(TRACK_TYPE, kind);
    rest.extend(element(CODEC_ID, described.codec.as_bytes()));
    if let Some(private) = &described.private {
        rest.extend(element(CODEC_PRIVATE, private));
    }
    if let Some(code) = language(packed_language).filter(|c| c != b"und") {
        rest.extend(element(LANGUAGE, &code));
    }
    rest.extend(uint_element(FLAG_LACING, 0));
    if let Some(pre_skip) = described.opus_delay {
        rest.extend(uint_element(
            CODEC_DELAY,
            pre_skip.saturating_mul(1_000_000_000) / 48_000,
        ));
        rest.extend(uint_element(SEEK_PRE_ROLL, OPUS_PRE_ROLL_NS));
    }
    if let Some((width, height)) = described.size {
        let video = [
            uint_element(PIXEL_WIDTH, width),
            uint_element(PIXEL_HEIGHT, height),
        ]
        .concat();
        rest.extend(element(VIDEO, &video));
    }
    if let Some((rate, channels)) = described.sound {
        let audio = [
            element(SAMPLING_FREQUENCY, &rate.to_be_bytes()),
            uint_element(CHANNELS, channels),
        ]
        .concat();
        rest.extend(element(AUDIO, &audio));
    }
    rest
}

/// Each sample's show and decode time in nanoseconds, from the track's
/// tables, its fragments' start and its edit list; and where its
/// fragments say it starts, in nanoseconds, where they do. `opus` keeps
/// the edit list's start in the times (`CodecDelay` says it instead).
fn block_times(track: &Track, opus: bool) -> (Vec<(i64, i64)>, Option<i64>) {
    let scale = i128::from(track.timescale.max(1));
    let ns = |t: i128| -> i64 {
        i64::try_from(t.saturating_mul(NS_PER_SECOND) / scale).unwrap_or(if t < 0 {
            i64::MIN
        } else {
            i64::MAX
        })
    };
    // Empty edits first wait (in the movie's timescale); the first edit
    // that is not empty says where the track's media starts showing.
    let mut wait_ns = 0i128;
    let mut shift = 0i128;
    for edit in &track.edits {
        if edit.media_time < 0 {
            wait_ns = wait_ns.saturating_add(
                i128::from(edit.duration).saturating_mul(NS_PER_SECOND)
                    / i128::from(track.edits_timescale.max(1)),
            );
            continue;
        }
        if !opus {
            shift = i128::from(edit.media_time);
        }
        break;
    }
    let wait = i64::try_from(wait_ns).unwrap_or(i64::MAX);
    let start = track.start.map(|s| ns(i128::from(s)));
    let mut decode = i128::from(track.start.unwrap_or(0));
    let mut times = Vec::with_capacity(track.samples.len());
    for sample in &track.samples {
        let shows = decode + i128::from(sample.cts) - shift;
        times.push((
            ns(shows).saturating_add(wait),
            ns(decode - shift).saturating_add(wait),
        ));
        decode += i128::from(sample.duration);
    }
    (times, start)
}

#[cfg(test)]
mod tests;
