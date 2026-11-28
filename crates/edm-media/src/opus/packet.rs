// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An Opus packet's framing (RFC 6716, section 3): its table-of-contents byte (the mode,
//! bandwidth, frame size and channels) and its one to 48 frames, packed by one of four codes;
//! and the identification header of an Opus stream (RFC 7845, section 5.1).

use crate::Error;

/// Which coder a packet's frames are in (RFC 6716, 3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// SILK alone (speech, up to 8 kHz of bandwidth).
    Silk,
    /// SILK below 8 kHz and CELT above.
    Hybrid,
    /// CELT alone.
    Celt,
}

/// What a packet's first byte says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Toc {
    pub(crate) mode: Mode,
    /// Each frame's length in samples at 48 kHz.
    pub(crate) frame_size: usize,
    /// Whether the frames code two channels.
    pub(crate) stereo: bool,
    /// The CELT band after the last the bandwidth reaches (13 narrowband, 17 medium and wide,
    /// 19 super-wide, 21 full).
    pub(crate) end_band: usize,
}

/// A packet's table of contents (RFC 6716, table 2).
pub(crate) fn toc(byte: u8) -> Toc {
    let config = usize::from(byte >> 3);
    let (mode, frame_size, end_band) = match config {
        0..=11 => (
            Mode::Silk,
            [480, 960, 1920, 2880][config % 4],
            [13, 17, 17][config / 4],
        ),
        12..=15 => (
            Mode::Hybrid,
            [480, 960][config % 2],
            [19, 21][(config - 12) / 2],
        ),
        _ => (
            Mode::Celt,
            [120, 240, 480, 960][config % 4],
            [13, 17, 19, 21][(config - 16) / 4],
        ),
    };
    Toc {
        mode,
        frame_size,
        stereo: byte & 0x04 != 0,
        end_band,
    }
}

/// The most bytes a frame may have.
const MAX_FRAME: usize = 1275;
/// The most a packet may hold, in samples at 48 kHz (120 ms).
const MAX_SAMPLES: usize = 5760;

/// A frame's length as a packet codes it: one byte under 252, else two. Returns the length
/// and the bytes it took.
fn frame_length(data: &[u8]) -> Option<(usize, usize)> {
    let first = usize::from(*data.first()?);
    if first < 252 {
        return Some((first, 1));
    }
    let second = usize::from(*data.get(1)?);
    Some((4 * second + first, 2))
}

/// A damaged packet, in words.
fn damaged() -> Error {
    "an Opus packet is damaged".into()
}

/// The frames of `packet` (RFC 6716, 3.2), each as its bytes; a frame of no bytes is one
/// to conceal.
pub(crate) fn frames(packet: &[u8]) -> Result<Vec<&[u8]>, Error> {
    let (&first, data) = packet.split_first().ok_or_else(damaged)?;
    let frame_size = toc(first).frame_size;
    let frames: Vec<&[u8]> = match first & 3 {
        0 => vec![data],
        1 => {
            if data.len() % 2 != 0 {
                return Err(damaged());
            }
            let (a, b) = data.split_at(data.len() / 2);
            vec![a, b]
        }
        2 => {
            let (len, took) = frame_length(data).ok_or_else(damaged)?;
            let rest = data.get(took..).ok_or_else(damaged)?;
            if len > rest.len() {
                return Err(damaged());
            }
            let (a, b) = rest.split_at(len);
            vec![a, b]
        }
        _ => code3(data)?,
    };
    if frames.len() * frame_size > MAX_SAMPLES || frames.iter().any(|f| f.len() > MAX_FRAME) {
        return Err(damaged());
    }
    Ok(frames)
}

/// A packet of any number of frames (code 3): a count byte, the padding's length, and each
/// frame's length but the last where they differ.
fn code3(data: &[u8]) -> Result<Vec<&[u8]>, Error> {
    let (&count_byte, mut rest) = data.split_first().ok_or_else(damaged)?;
    let count = usize::from(count_byte & 0x3F);
    if count == 0 {
        return Err(damaged());
    }
    if count_byte & 0x40 != 0 {
        let mut padding = 0usize;
        loop {
            let (&p, after) = rest.split_first().ok_or_else(damaged)?;
            rest = after;
            padding += if p == 255 { 254 } else { usize::from(p) };
            if p != 255 {
                break;
            }
        }
        let keep = rest.len().checked_sub(padding).ok_or_else(damaged)?;
        rest = &rest[..keep];
    }
    let mut frames = Vec::with_capacity(count);
    if count_byte & 0x80 != 0 {
        let mut lengths = Vec::with_capacity(count);
        for _ in 1..count {
            let (len, took) = frame_length(rest).ok_or_else(damaged)?;
            rest = rest.get(took..).ok_or_else(damaged)?;
            lengths.push(len);
        }
        for len in lengths {
            if len > rest.len() {
                return Err(damaged());
            }
            let (frame, after) = rest.split_at(len);
            frames.push(frame);
            rest = after;
        }
        frames.push(rest);
    } else {
        if rest.len() % count != 0 {
            return Err(damaged());
        }
        let each = rest.len() / count;
        if each == 0 {
            frames.resize(count, &[][..]);
        } else {
            frames.extend(rest.chunks(each));
        }
    }
    Ok(frames)
}

/// An Opus stream's identification header (RFC 7845, 5.1), as a WebM keeps it in its track's
/// private data.
#[derive(Debug, PartialEq)]
pub(crate) struct Head {
    /// Channels of output: 1 or 2.
    pub(crate) channels: usize,
    /// Samples at 48 kHz the decoder's output starts with that are not the sound's.
    pub(crate) pre_skip: usize,
    /// The gain to apply to the output, as a factor.
    pub(crate) gain: f32,
}

/// The `OpusHead` in `bytes`: one stream of one or two channels (mapping family 0, or 1 with
/// one stream). More channels are refused in words.
pub(crate) fn head(bytes: &[u8]) -> Result<Head, Error> {
    let bad = || Error::from("the Opus sound's header is damaged");
    if bytes.get(..8) != Some(&b"OpusHead"[..]) {
        return Err(bad());
    }
    let version = *bytes.get(8).ok_or_else(bad)?;
    if version & 0xF0 != 0 {
        return Err(format!("Opus header version {version} is not one EDM reads").into());
    }
    let channels = usize::from(*bytes.get(9).ok_or_else(bad)?);
    let pre_skip = u16::from_le_bytes([
        *bytes.get(10).ok_or_else(bad)?,
        *bytes.get(11).ok_or_else(bad)?,
    ]);
    let gain = i16::from_le_bytes([
        *bytes.get(16).ok_or_else(bad)?,
        *bytes.get(17).ok_or_else(bad)?,
    ]);
    let family = *bytes.get(18).ok_or_else(bad)?;
    if family != 0 {
        // A channel mapping: EDM takes it only where it is one stream, as family 0 is.
        let streams = *bytes.get(19).ok_or_else(bad)?;
        if streams != 1 {
            return Err(format!(
                "an Opus sound of {channels} channels in {streams} streams is not one EDM reads"
            )
            .into());
        }
    }
    if !(1..=2).contains(&channels) {
        return Err(format!("EDM makes an MP3 of one or two channels, not {channels}").into());
    }
    Ok(Head {
        channels,
        pre_skip: usize::from(pre_skip),
        gain: 10f32.powf(f32::from(gain) / (20.0 * 256.0)),
    })
}
