// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a codec's frames say: a VP9 key frame's shape, an Opus track's samples and what of it
//! shows, and a WebM's tracks for an MP4.

use super::*;

/// A VP9 key frame's profile, bit depth and chroma subsampling, from its
/// uncompressed header, for the `vpcC` an MP4 names them in.
pub(super) fn vp9_shape(frame: &[u8]) -> Option<(u8, u8, u8)> {
    let mut bits = BitReader { data: frame, at: 0 };
    if bits.take(2)? != 2 {
        return None;
    }
    let low = bits.take(1)?;
    let high = bits.take(1)?;
    let profile = (high << 1 | low) as u8;
    if profile == 3 {
        bits.take(1)?;
    }
    if bits.take(1)? == 1 {
        return None; // shows an earlier frame: no header here
    }
    if bits.take(1)? != 0 {
        return None; // not a key frame
    }
    bits.take(2)?; // show_frame, error_resilient_mode
    if bits.take(24)? != 0x49_8342 {
        return None;
    }
    let depth = if profile >= 2 {
        if bits.take(1)? == 1 {
            12
        } else {
            10
        }
    } else {
        8
    };
    let space = bits.take(3)?;
    // 1 is 4:2:0 with chroma beside the first luma sample, what VP9 uses.
    let mut chroma = 1;
    if space != 7 {
        bits.take(1)?; // colour range
        if profile == 1 || profile == 3 {
            let x = bits.take(1)?;
            let y = bits.take(1)?;
            chroma = match (x, y) {
                (1, 0) => 2,
                (0, 0) => 3,
                _ => 1,
            };
        }
    } else {
        chroma = 3;
    }
    Some((profile, depth, chroma))
}

struct BitReader<'a> {
    data: &'a [u8],
    at: usize,
}

impl BitReader<'_> {
    fn take(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.data.get(self.at / 8)?;
            v = (v << 1) | u32::from((byte >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Some(v)
    }
}

/// How long an Opus track plays, in 48 kHz samples: all of them, less the
/// encoder's delay before and its padding after (WebM's DiscardPadding on
/// the last block, a signed count of nanoseconds). 0, which an edit reads
/// as "to the end", where nothing is to be cut there.
pub(super) fn opus_shown(
    samples: &[crate::mp4mux::Sample],
    pre_skip: u16,
    last: Option<&Block>,
) -> u64 {
    let padding_ns = last
        .and_then(|b| b.group.as_deref())
        .and_then(|group| find(group, DISCARD_PADDING))
        .map(sint)
        .unwrap_or(0);
    if padding_ns <= 0 {
        return 0;
    }
    let padding = u64::try_from(padding_ns)
        .unwrap_or(0)
        .saturating_mul(48_000)
        / 1_000_000_000;
    let all: u64 = samples.iter().map(|s| u64::from(s.duration)).sum();
    all.saturating_sub(u64::from(pre_skip))
        .saturating_sub(padding)
        .max(1)
}

/// A signed integer element's value.
pub(super) fn sint(body: &[u8]) -> i64 {
    let len = body.len().min(8);
    if len == 0 {
        return 0;
    }
    let value = uint(body);
    let shift = 64 - 8 * len as u32;
    ((value << shift) as i64) >> shift
}

/// An Opus packet's length in 48 kHz samples, from its table of contents.
pub(super) fn opus_samples(packet: &[u8]) -> u32 {
    let Some(&toc) = packet.first() else {
        return 960;
    };
    let config = toc >> 3;
    // In tenths of a millisecond.
    let tenths: u32 = match config {
        0..=11 => [100, 200, 400, 600]
            .get(usize::from(config % 4))
            .copied()
            .unwrap_or(200),
        12..=15 => [100, 200]
            .get(usize::from(config % 2))
            .copied()
            .unwrap_or(200),
        _ => [25, 50, 100, 200]
            .get(usize::from(config % 4))
            .copied()
            .unwrap_or(200),
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => u32::from(packet.get(1).copied().unwrap_or(1) & 0x3f).max(1),
    };
    tenths * 48 / 10 * frames
}

/// The tracks of the WebM at `path` as MP4 tracks whose samples are its
/// frames, where they are: VP9 (`vp09` with `vpcC`), AV1 (`av01` with the
/// `av1C` WebM keeps as its private data) and Opus (`Opus` with `dOps`).
/// For a video whose tallest picture comes only as WebM, joined into an
/// MP4. Another codec, or frames laced together, is refused.
pub(crate) fn mp4_tracks(path: &Path, input: usize) -> Result<Vec<crate::mp4mux::Track>, Error> {
    let read = read_input(path, input)?;
    let mut tracks = Vec::new();
    for track in &read.tracks {
        let blocks: Vec<&Block> = read
            .blocks
            .iter()
            .filter(|b| b.track == track.number)
            .collect();
        if blocks.is_empty() {
            continue;
        }
        if blocks.iter().any(|b| b.flags & 0x06 != 0) {
            return Err("its frames are laced together".into());
        }
        let private = find(&track.rest, 0x63A2).unwrap_or_default();
        let ms = |b: &Block| (i128::from(b.ticks) * i128::from(read.scale) / 1_000_000) as i64;
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let mut first = |b: &Block, n: usize| -> Vec<u8> {
            let mut buf = vec![0u8; (b.data_len as usize).min(n)];
            let ok = file
                .seek(SeekFrom::Start(b.data_at))
                .and_then(|_| file.read_exact(&mut buf));
            if ok.is_ok() {
                buf
            } else {
                Vec::new()
            }
        };
        match track.codec.as_str() {
            "V_VP9" | "V_AV1" => tracks.push(to_mp4::picture_track(
                track, &blocks, private, &ms, &mut first, input,
            )?),
            "A_OPUS" => tracks.push(to_mp4::opus_track(
                track, &blocks, private, &ms, &mut first, input,
            )?),
            other => return Err(format!("dm does not put {other} into an MP4").into()),
        }
    }
    Ok(tracks)
}
