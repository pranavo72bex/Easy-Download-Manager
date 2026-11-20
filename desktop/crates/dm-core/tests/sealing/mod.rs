// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the HLS shape tests make their streams with (gaps row Y16): a server of files by path,
//! MP4 boxes, a fragmented MP4 cut as a packager cuts it, and samples scrambled as SAMPLE-AES
//! (`ts`, a transport stream's) and Common Encryption's `cbcs` scramble them, with the keys they
//! are scrambled by.

#![allow(dead_code)] // each test binary uses a different part of this

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::Response;
use cbc::cipher::{block_padding::NoPadding, BlockModeEncrypt, InnerIvInit, KeyInit};
use dm_media::fmp4::{self, Spec};
use dm_media::mp4mux::testing::ReadBack;

use crate::common;

mod ts;
pub use ts::*;

pub type Files = Arc<HashMap<String, Vec<u8>>>;

pub async fn answer(State(files): State<Files>, uri: Uri) -> Response {
    match files.get(uri.path()) {
        Some(body) => Response::new(Body::from(body.clone())),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap(),
    }
}

/// Serves `files` by path; the address of `/v/show.m3u8` on it.
pub async fn serve(files: Vec<(String, Vec<u8>)>) -> String {
    let files: Files = Arc::new(files.into_iter().collect());
    let addr = common::serve(axum::Router::new().fallback(answer).with_state(files)).await;
    format!("http://{addr}/v/show.m3u8")
}

pub fn boxed(kind: &[u8; 4], parts: &[&[u8]]) -> Vec<u8> {
    let len: usize = parts.iter().map(|p| p.len()).sum::<usize>() + 8;
    let mut out = (len as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

pub fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let head = ((u32::from(version) << 24) | flags).to_be_bytes();
    boxed(kind, &[&head, body])
}

/// A fragmented MP4 cut where an HLS packager cuts it: its initialisation segment, and one
/// segment for each fragment.
pub fn cut(file: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut at = 0;
    let mut init = Vec::new();
    let mut segments: Vec<Vec<u8>> = Vec::new();
    while at + 8 <= file.len() {
        let size = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        let kind = &file[at + 4..at + 8];
        let whole = &file[at..at + size];
        match kind {
            b"moof" => segments.push(whole.to_vec()),
            b"mdat" => segments.last_mut().unwrap().extend_from_slice(whole),
            _ => init.extend_from_slice(whole),
        }
        at += size;
    }
    (init, segments)
}

pub fn picture_spec(avcc: &[u8], width: u32, height: u32, timescale: u32) -> Spec {
    Spec {
        entry: fmp4::avc1(avcc, width, height),
        timescale,
        video: true,
        width,
        height,
    }
}

/// When sample `index` of a track starts, in seconds.
pub fn starts_at(track: &ReadBack, index: usize) -> f64 {
    let ticks: u64 = track.durations[..index].iter().map(|d| u64::from(*d)).sum();
    (track.start.unwrap_or(0) + ticks) as f64 / f64::from(track.timescale)
}

pub fn length_of(track: &ReadBack) -> f64 {
    starts_at(track, track.samples.len())
}

pub const KEY_ONE: [u8; 16] = [0x3c; 16];
pub const KEY_TWO: [u8; 16] = [
    0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe, 0x0f,
];

/// `blocks` (whole 16-byte blocks) scrambled in one chain from `iv`.
pub fn seal_blocks(key: &[u8; 16], iv: &[u8; 16], blocks: &mut [u8]) {
    let len = blocks.len();
    let cipher = aes::Aes128::new_from_slice(key).unwrap();
    cbc::Encryptor::<aes::Aes128>::inner_iv_slice_init(cipher, iv)
        .unwrap()
        .encrypt_padded::<NoPadding>(blocks, len)
        .unwrap();
}

/// Scrambles the blocks of `data` at `starts` (each 16 bytes) as one chain from `iv`.
pub fn seal_at(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8], starts: &[usize]) {
    let mut gathered: Vec<u8> = starts
        .iter()
        .flat_map(|&at| data[at..at + 16].to_vec())
        .collect();
    if gathered.is_empty() {
        return;
    }
    seal_blocks(key, iv, &mut gathered);
    for (n, &at) in starts.iter().enumerate() {
        data[at..at + 16].copy_from_slice(&gathered[n * 16..n * 16 + 16]);
    }
}

pub const CONSTANT_IV: [u8; 16] = [0x5a; 16];

/// A sample entry made `encv` or `enca`: scrambled by `cbcs` with `pattern` (crypt and skip, in
/// four bits each) and a constant vector.
pub fn sealed_entry(entry: &[u8], kind: &[u8; 4], pattern: u8) -> Vec<u8> {
    let original: [u8; 4] = entry[4..8].try_into().unwrap();
    let mut tenc = vec![0, pattern, 1, 0];
    tenc.extend_from_slice(&[0x77; 16]);
    tenc.push(16);
    tenc.extend_from_slice(&CONSTANT_IV);
    let sinf = boxed(
        b"sinf",
        &[
            &boxed(b"frma", &[&original]),
            &full(
                b"schm",
                0,
                0,
                &[b"cbcs".as_slice(), &0x0001_0000u32.to_be_bytes()].concat(),
            ),
            &boxed(b"schi", &[&full(b"tenc", 1, 0, &tenc)]),
        ],
    );
    boxed(kind, &[&entry[8..], &sinf])
}

/// A sample's protected bytes scrambled as `cbcs` has them, from `from`: `crypt` blocks then
/// `skip` in the clear, over and over (every block where both are nought), one chain from the
/// constant vector.
pub fn seal_cbcs(key: &[u8; 16], sample: &mut [u8], from: usize, crypt: usize, skip: usize) {
    let mut starts = Vec::new();
    let mut at = from;
    while at + 16 <= sample.len() {
        if crypt == 0 {
            starts.push(at);
            at += 16;
            continue;
        }
        for _ in 0..crypt {
            if at + 16 <= sample.len() {
                starts.push(at);
            }
            at += 16;
        }
        at += skip * 16;
    }
    seal_at(key, &CONSTANT_IV, sample, &starts);
}
