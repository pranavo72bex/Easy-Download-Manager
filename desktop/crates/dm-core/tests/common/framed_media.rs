// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The media a stand-in for YouTube's framed stream serves (gaps row Y2): a fixture's first
//! boxes and its one piece, numbered and timed as the stream's pieces are, as many as asked.

#![allow(dead_code)] // each test binary uses a different part of this

/// A fixture's bytes, from `tests/fixtures/mp4`.
pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mp4")
            .join(name),
    )
    .unwrap()
}

/// A box of `kind` inside `data` (the boxes a piece nests searched): where its body starts.
fn find_box(data: &[u8], kind: &[u8; 4]) -> Option<usize> {
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize;
        if size < 8 || at + size > data.len() {
            return None;
        }
        if &data[at + 4..at + 8] == kind {
            return Some(at + 8);
        }
        if matches!(&data[at + 4..at + 8], b"moof" | b"traf") {
            if let Some(found) = find_box(&data[at + 8..at + size], kind) {
                return Some(at + 8 + found);
            }
        }
        at += size;
    }
    None
}

/// Where a top-level box of `kind` starts, and its size.
fn top_box(data: &[u8], kind: &[u8; 4]) -> (usize, usize) {
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        if &data[at + 4..at + 8] == kind {
            return (at, size);
        }
        at += size;
    }
    panic!("no {kind:?} box");
}

/// A fixture as the framed stream serves it: its first boxes, and its one piece, which
/// [`Split::piece`] numbers and times.
pub struct Split {
    /// Its first boxes (`ftyp`, `moov`).
    pub init: Vec<u8>,
    /// Its one piece (`moof` and `mdat`), as the fixture has it.
    piece: Vec<u8>,
    /// How long the piece plays, in the fixture's timescale (its `sidx`).
    duration: u32,
    /// The media bytes a piece carries.
    pub media_bytes: usize,
}

impl Split {
    /// The fixture `name`, split.
    pub fn of(name: &str) -> Split {
        let whole = fixture(name);
        let (moov_at, moov_size) = top_box(&whole, b"moov");
        let (sidx_at, sidx_size) = top_box(&whole, b"sidx");
        let (moof_at, _) = top_box(&whole, b"moof");
        let (mdat_at, mdat_size) = top_box(&whole, b"mdat");
        let sidx = &whole[sidx_at..sidx_at + sidx_size];
        let entries = if sidx[8] == 1 { 40 } else { 32 };
        let duration = u32::from_be_bytes(sidx[entries + 4..entries + 8].try_into().unwrap());
        Split {
            init: whole[..moov_at + moov_size].to_vec(),
            piece: whole[moof_at..mdat_at + mdat_size].to_vec(),
            duration,
            media_bytes: mdat_size - 8,
        }
    }

    /// Piece `n` (from 1): the fixture's piece numbered `n` and timed after `n - 1` of them.
    pub fn piece(&self, n: u32) -> Vec<u8> {
        let mut piece = self.piece.clone();
        let seq_at = find_box(&piece, b"mfhd").unwrap();
        piece[seq_at + 4..seq_at + 8].copy_from_slice(&n.to_be_bytes());
        let tfdt_at = find_box(&piece, b"tfdt").unwrap();
        let time = u64::from(n - 1) * u64::from(self.duration);
        if piece[tfdt_at] == 1 {
            piece[tfdt_at + 4..tfdt_at + 12].copy_from_slice(&time.to_be_bytes());
        } else {
            piece[tfdt_at + 4..tfdt_at + 8].copy_from_slice(&(time as u32).to_be_bytes());
        }
        piece
    }
}

/// A fixture as the framed stream serves it: its first boxes, then `pieces` copies of its one
/// piece, each numbered and timed after the one before. Returns them, and the media bytes each
/// piece carries.
pub fn pieces_of(name: &str, pieces: u32) -> (Vec<u8>, Vec<Vec<u8>>, usize) {
    let split = Split::of(name);
    let all = (1..=pieces).map(|n| split.piece(n)).collect();
    (split.init, all, split.media_bytes)
}
