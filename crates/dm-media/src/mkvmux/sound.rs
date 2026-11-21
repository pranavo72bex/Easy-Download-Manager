// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A WebM's sound track as its codec's packets, for a decoder of dm's own (`crate::opus`,
//! gaps row Y25): the track's private data, its frames in order (unlaced) and the padding its
//! last block names.

use super::*;

const CODEC_PRIVATE: u32 = 0x63A2;
/// The most bytes one sound block may hold here: a sound frame is far smaller.
const MOST_BLOCK: u64 = 1 << 24;

/// The first sound track of a WebM, as its codec reads it.
pub(crate) struct SoundTrack {
    /// Its codec, as Matroska names it ("A_OPUS").
    pub(crate) codec: String,
    /// Its private data (Opus's `OpusHead`).
    pub(crate) private: Vec<u8>,
    /// Its frames in the order they play.
    pub(crate) packets: Vec<Vec<u8>>,
    /// How much of the last frame is padding, in nanoseconds (WebM's DiscardPadding).
    pub(crate) padding_ns: i64,
}

/// The first sound track of the WebM at `path`, its frames read.
pub(crate) fn sound_track(path: &Path) -> Result<SoundTrack, Error> {
    let input = read_input(path, 0)?;
    let track = input
        .tracks
        .iter()
        .find(|t| t.kind == 2)
        .ok_or("the file has no sound to save as MP3")?;
    let private = find(&track.rest, CODEC_PRIVATE)
        .unwrap_or_default()
        .to_vec();
    let mut blocks: Vec<&Block> = input
        .blocks
        .iter()
        .filter(|b| b.track == track.number)
        .collect();
    blocks.sort_by_key(|b| b.decode);
    let mut file =
        Positioned::open(path).map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    let mut packets = Vec::with_capacity(blocks.len());
    for block in &blocks {
        if block.data_len > MOST_BLOCK {
            return Err("a sound block is too large".into());
        }
        let mut data = vec![0u8; block.data_len as usize];
        file.read_into(block.data_at, &mut data)
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        unlace(block.flags, data, &mut packets)?;
    }
    let padding_ns = blocks
        .last()
        .and_then(|b| b.group.as_deref())
        .and_then(|group| find(group, DISCARD_PADDING))
        .map(super::codecs::sint)
        .unwrap_or(0);
    Ok(SoundTrack {
        codec: track.codec.clone(),
        private,
        packets,
        padding_ns,
    })
}

/// A block's frames: itself, or the frames laced into it (Xiph, EBML or fixed lacing).
fn unlace(flags: u8, data: Vec<u8>, out: &mut Vec<Vec<u8>>) -> Result<(), Error> {
    let lacing = flags & 0x06;
    if lacing == 0 {
        out.push(data);
        return Ok(());
    }
    let damaged = || Error::from("a laced sound block is damaged");
    let (&less_one, mut rest) = data.split_first().ok_or_else(damaged)?;
    let count = usize::from(less_one) + 1;
    let mut sizes = Vec::with_capacity(count);
    match lacing {
        0x02 => {
            for _ in 1..count {
                let mut size = 0usize;
                loop {
                    let (&byte, after) = rest.split_first().ok_or_else(damaged)?;
                    rest = after;
                    size += usize::from(byte);
                    if byte != 255 {
                        break;
                    }
                }
                sizes.push(size);
            }
        }
        0x06 => {
            let mut size = 0i64;
            for i in 1..count {
                let (value, len) = read_size(rest).ok_or_else(damaged)?;
                let value = i64::try_from(value.ok_or_else(damaged)?).map_err(|_| damaged())?;
                rest = rest.get(len..).ok_or_else(damaged)?;
                if i == 1 {
                    size = value;
                } else {
                    // A signed difference from the size before.
                    size += value - ((1i64 << (7 * len - 1)) - 1);
                }
                sizes.push(usize::try_from(size).map_err(|_| damaged())?);
            }
        }
        _ => {
            if rest.len() % count != 0 {
                return Err(damaged());
            }
            sizes.resize(count - 1, rest.len() / count);
        }
    }
    for size in sizes {
        if size > rest.len() {
            return Err(damaged());
        }
        let (frame, after) = rest.split_at(size);
        out.push(frame.to_vec());
        rest = after;
    }
    out.push(rest.to_vec());
    Ok(())
}
