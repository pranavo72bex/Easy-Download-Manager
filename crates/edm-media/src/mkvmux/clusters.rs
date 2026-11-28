// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The clusters of the file `write` makes: the blocks in their order, copied from their
//! inputs.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use super::{
    id_bytes, size8, uint_element, Placed, Positioned, BLOCK, BLOCK_GROUP, CLUSTER, CLUSTER_MS,
    SIMPLE_BLOCK, TIMECODE,
};
use crate::Error;

/// A cue: a cluster's time, the track of its first block, and its position in the segment.
type Cue = (i64, u64, u64);

/// Writes the clusters to `w`, the file at `out`, from `at`, where it stands in the segment:
/// a new one at a picture's key frame a second or more in, or before a block's offset would
/// outgrow its 16 bits. Where they end, and the cues to them (each one's time, track and
/// position).
pub(super) fn write_clusters(
    w: &mut BufWriter<File>,
    out: &Path,
    placed: &[Placed],
    files: &mut [Positioned],
    has_video: bool,
    mut at: u64,
) -> Result<(u64, Vec<Cue>), Error> {
    let wrote = |e: std::io::Error| format!("{} could not be written: {e}", out.display());
    let mut cues = Vec::new();
    let mut i = 0;
    let mut buffer = vec![0u8; 1 << 20];
    while let Some(first) = placed.get(i) {
        let start = first.ms.max(0);
        let mut end = i + 1;
        while let Some(p) = placed.get(end) {
            let span = p.ms - start;
            let cut = if has_video {
                p.video && p.block.key && span >= 1000
            } else {
                span >= 5000
            };
            if cut || span >= CLUSTER_MS {
                break;
            }
            end += 1;
        }
        let blocks = placed.get(i..end).unwrap_or_default();
        if (has_video && first.video && first.block.key) || !has_video {
            cues.push((start, first.track, at));
        }
        // The cluster's length, then it, block by block.
        let block_len = |p: &Placed| -> u64 {
            let simple = 4 + p.block.data_len;
            match &p.block.group {
                None => 1 + 8 + simple,
                Some(extra) => 1 + 8 + (1 + 8 + simple) + extra.len() as u64,
            }
        };
        let timecode = uint_element(TIMECODE, start as u64);
        let body: u64 = timecode.len() as u64 + blocks.iter().map(block_len).sum::<u64>();
        w.write_all(&id_bytes(CLUSTER)).map_err(wrote)?;
        w.write_all(&size8(body)).map_err(wrote)?;
        w.write_all(&timecode).map_err(wrote)?;
        for p in blocks {
            let offset = i16::try_from(p.ms - start).map_err(|_| "a block out of order")?;
            let mut head = vec![0x80 | p.track as u8];
            head.extend_from_slice(&offset.to_be_bytes());
            head.push(p.block.flags);
            match &p.block.group {
                None => {
                    w.write_all(&[SIMPLE_BLOCK as u8]).map_err(wrote)?;
                    w.write_all(&size8(4 + p.block.data_len)).map_err(wrote)?;
                }
                Some(extra) => {
                    w.write_all(&[BLOCK_GROUP as u8]).map_err(wrote)?;
                    w.write_all(&size8(1 + 8 + 4 + p.block.data_len + extra.len() as u64))
                        .map_err(wrote)?;
                    w.write_all(&[BLOCK as u8]).map_err(wrote)?;
                    w.write_all(&size8(4 + p.block.data_len)).map_err(wrote)?;
                }
            }
            w.write_all(&head).map_err(wrote)?;
            let file = files
                .get_mut(p.block.input)
                .ok_or("an input went missing")?;
            let mut left = p.block.data_len;
            let mut from = p.block.data_at;
            while left > 0 {
                let take = left.min(buffer.len() as u64) as usize;
                let part = buffer.get_mut(..take).ok_or("a read went wrong")?;
                file.read_into(from, part)
                    .map_err(|e| format!("an input could not be read: {e}"))?;
                from += take as u64;
                w.write_all(part).map_err(wrote)?;
                left -= take as u64;
            }
            // A group's other elements after its block, which comes first.
            if let Some(extra) = &p.block.group {
                w.write_all(extra).map_err(wrote)?;
            }
        }
        at += 4 + 8 + body;
        i = end;
    }
    Ok((at, cues))
}
