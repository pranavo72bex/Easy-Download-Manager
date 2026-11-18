// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Small WebM files made for tests: this crate's, and dm-core's through the `testing`
//! feature.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

/// A WebM of one track of `codec` (`kind` 1 a picture, 2 a sound):
/// `frames` blocks of 20 bytes each 33 ms apart, the first a key frame.
pub fn tiny_webm(path: &Path, codec: &str, kind: u64, frames: usize) {
    tiny_webm_at(path, codec, kind, frames, 0);
}

/// [`tiny_webm`] whose one cluster starts `start_ms` in, as a live
/// stream's does.
pub fn tiny_webm_at(path: &Path, codec: &str, kind: u64, frames: usize, start_ms: u64) {
    let header = element(EBML, &element(0x4282, b"webm"));
    let info = element(INFO, &uint_element(TIMECODE_SCALE, SCALE_NS));
    let mut entry = uint_element(TRACK_NUMBER, 1);
    entry.extend(uint_element(TRACK_UID, 1));
    entry.extend(uint_element(TRACK_TYPE, kind));
    entry.extend(element(CODEC_ID, codec.as_bytes()));
    if codec == "A_VORBIS" {
        // Vorbis keeps its three headers here; their bytes do not matter.
        entry.extend(element(0x63A2, &[2, 1, 1, 0xAA, 0xBB, 0xCC]));
    }
    let tracks = element(TRACKS, &element(TRACK_ENTRY, &entry));
    let mut cluster = uint_element(TIMECODE, start_ms);
    for i in 0..frames {
        let mut block = vec![0x81];
        block.extend_from_slice(&(i as i16 * 33).to_be_bytes());
        block.push(if i == 0 || kind == 2 { 0x80 } else { 0 });
        block.extend_from_slice(&[i as u8; 20]);
        cluster.extend(element(SIMPLE_BLOCK, &block));
    }
    let segment = element(
        SEGMENT,
        &[info, tracks, element(CLUSTER, &cluster)].concat(),
    );
    std::fs::write(path, [header, segment].concat()).unwrap();
}
