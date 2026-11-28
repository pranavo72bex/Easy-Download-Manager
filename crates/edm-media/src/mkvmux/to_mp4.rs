// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A WebM's picture or Opus track as an MP4 track whose samples are its frames, for
//! `mp4_tracks`.

use super::{find, opus_samples, opus_shown, uint, vp9_shape, Block, TrackIn};
use crate::mp4mux::{full, Sample, Track};
use crate::Error;

/// The VP9 (`vp09` with `vpcC`) or AV1 (`av01` with the `av1C` WebM keeps as its
/// `private` data) picture `track`, whose frames are `blocks` of input `input`: `ms` is a
/// block's time in milliseconds, `first` reads a block's first bytes.
pub(super) fn picture_track(
    track: &TrackIn,
    blocks: &[&Block],
    private: &[u8],
    ms: &impl Fn(&Block) -> i64,
    first: &mut impl FnMut(&Block, usize) -> Vec<u8>,
    input: usize,
) -> Result<Track, Error> {
    let video = find(&track.rest, 0xE0).unwrap_or_default();
    let width = find(video, 0xB0).map_or(0, uint) as u32;
    let height = find(video, 0xBA).map_or(0, uint) as u32;
    let entry = if track.codec == "V_VP9" {
        let key = blocks.iter().find(|b| b.key).copied();
        let (profile, depth, chroma) = key
            .and_then(|b| vp9_shape(&first(b, 64)))
            .unwrap_or((0, 8, 1));
        // Level unknown (0 says so); colour unspecified (2).
        let mut vpcc = vec![profile, 0, (depth << 4) | (chroma << 1), 2, 2, 2];
        vpcc.extend_from_slice(&0u16.to_be_bytes());
        crate::fmp4::visual(b"vp09", &full(b"vpcC", 1, 0, &vpcc), width, height)
    } else {
        if private.len() < 4 {
            return Err("its AV1 has no configuration".into());
        }
        crate::fmp4::visual(
            b"av01",
            &crate::mp4mux::boxed(b"av1C", &[private]),
            width,
            height,
        )
    };
    let mut samples = Vec::with_capacity(blocks.len());
    for (i, b) in blocks.iter().enumerate() {
        let next = blocks.get(i + 1).map(|n| ms(n));
        let duration = next.map_or(0, |n| {
            u32::try_from(n.saturating_sub(ms(b)).max(0)).unwrap_or(u32::MAX)
        });
        samples.push(Sample {
            offset: b.data_at,
            size: b.data_len as u32,
            duration,
            cts: 0,
            sync: b.key,
        });
    }
    // The last lasts as long as the one before it.
    let usual = samples
        .iter()
        .rev()
        .nth(1)
        .map_or(33, |s| s.duration.max(1));
    if let Some(last) = samples.last_mut() {
        last.duration = usual;
    }
    Ok(Track {
        input,
        id: track.number as u32,
        tkhd_tail: crate::tsmux::tkhd_tail(0, width, height),
        timescale: 1000,
        language: 0x55c4,
        hdlr: crate::tsmux::hdlr(b"vide", "VideoHandler"),
        media_header: full(b"vmhd", 0, 1, &[0u8; 8]),
        stsd: full(b"stsd", 0, 0, &[&1u32.to_be_bytes()[..], &entry].concat()),
        edits: Vec::new(),
        edits_timescale: 1000,
        samples,
        descriptions: Vec::new(),
        start: blocks.first().map(|b| ms(b).max(0) as u64),
    })
}

/// The Opus (`Opus` with `dOps`) sound `track`, as [`picture_track`] has a picture.
pub(super) fn opus_track(
    track: &TrackIn,
    blocks: &[&Block],
    private: &[u8],
    ms: &impl Fn(&Block) -> i64,
    first: &mut impl FnMut(&Block, usize) -> Vec<u8>,
    input: usize,
) -> Result<Track, Error> {
    // OpusHead, little-endian, as WebM keeps it; dOps is the same
    // fields big-endian, without the magic.
    let head = private
        .get(8..)
        .filter(|h| h.len() >= 11)
        .ok_or("its Opus has no header")?;
    let byte = |i: usize| head.get(i).copied().unwrap_or(0);
    let channels = byte(1);
    let pre_skip = u16::from_le_bytes([byte(2), byte(3)]);
    let rate = u32::from_le_bytes([byte(4), byte(5), byte(6), byte(7)]);
    let gain = i16::from_le_bytes([byte(8), byte(9)]);
    let mut dops = vec![0, channels];
    dops.extend_from_slice(&pre_skip.to_be_bytes());
    dops.extend_from_slice(&rate.to_be_bytes());
    dops.extend_from_slice(&gain.to_be_bytes());
    dops.extend_from_slice(head.get(10..).unwrap_or_default());
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 8]);
    entry.extend_from_slice(&u16::from(channels.max(1)).to_be_bytes());
    entry.extend_from_slice(&16u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 4]);
    entry.extend_from_slice(&(48_000u32 << 16).to_be_bytes());
    let opus = crate::mp4mux::boxed(b"Opus", &[&entry, &crate::mp4mux::boxed(b"dOps", &[&dops])]);
    let mut samples = Vec::with_capacity(blocks.len());
    for b in blocks {
        samples.push(Sample {
            offset: b.data_at,
            size: b.data_len as u32,
            duration: opus_samples(&first(b, 2)),
            cts: 0,
            sync: true,
        });
    }
    Ok(Track {
        input,
        id: track.number as u32,
        tkhd_tail: crate::tsmux::tkhd_tail(0x0100, 0, 0),
        timescale: 48_000,
        language: 0x55c4,
        hdlr: crate::tsmux::hdlr(b"soun", "SoundHandler"),
        media_header: full(b"smhd", 0, 0, &[0u8; 4]),
        stsd: full(b"stsd", 0, 0, &[&1u32.to_be_bytes()[..], &opus].concat()),
        // The encoder's delay skipped, as dOps says, and its
        // padding after the end, as the last block's
        // DiscardPadding says: kept, the sound ran 13 ms past
        // its end where a player honours where it ends.
        edits: vec![crate::mp4mux::Edit {
            duration: opus_shown(&samples, pre_skip, blocks.last().copied()),
            media_time: i64::from(pre_skip),
        }],
        edits_timescale: 48_000,
        samples,
        descriptions: Vec::new(),
        start: blocks
            .first()
            .map(|b| (ms(b).max(0) as u64).saturating_mul(48)),
    })
}
