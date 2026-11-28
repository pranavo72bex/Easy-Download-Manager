// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! HLS's SAMPLE-AES with a clear key: the samples of a stream, not its segments, scrambled
//! with AES-128-CBC (gaps row Y16).
//!
//! A transport stream's H.264 pictures and ADTS AAC frames are scrambled as Apple's "MPEG-2
//! Stream Encryption Format for HTTP Live Streaming" describes (`tsmux`); a fragmented MP4's
//! samples by Common Encryption's `cbcs` scheme (`mp4mux::clear_cbcs`). Either way only whole
//! 16-byte blocks are scrambled, so a sample keeps its length, and each scrambled run starts its
//! chain afresh from the vector. The key is the playlist's own, handed to any player that asks:
//! a key kept by a rights system is refused before anything gets here.

use aes::cipher::{BlockModeDecrypt, InnerIvInit, KeyInit};

/// One segment's key and vector, from where it starts in the file its segments were joined
/// into; `None` for a segment in the clear.
pub type Lock = Option<([u8; 16], [u8; 16])>;

/// The keys of a stream's segments, by where each segment starts in the joined file, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Locks(pub Vec<(u64, Lock)>);

impl Locks {
    /// The lock of the segment the byte at `offset` belongs to.
    pub fn at(&self, offset: u64) -> Lock {
        let after = self.0.partition_point(|(start, _)| *start <= offset);
        after
            .checked_sub(1)
            .and_then(|i| self.0.get(i))
            .and_then(|(_, lock)| *lock)
    }

    /// Whether any segment is scrambled.
    pub fn any(&self) -> bool {
        self.0.iter().any(|(_, lock)| lock.is_some())
    }
}

/// A run of scrambled blocks, chained from `iv`: `blocks` is decrypted in place, every whole
/// 16-byte block of it (anything past the last whole block is left).
pub(crate) struct Chain {
    mode: cbc::Decryptor<aes::Aes128>,
}

impl Chain {
    pub(crate) fn new(key: &[u8; 16], iv: &[u8; 16]) -> Option<Chain> {
        let cipher = aes::Aes128::new_from_slice(key).ok()?;
        let mode = cbc::Decryptor::<aes::Aes128>::inner_iv_slice_init(cipher, iv).ok()?;
        Some(Chain { mode })
    }

    /// The next whole blocks of the chain, decrypted in place.
    pub(crate) fn blocks(&mut self, data: &mut [u8]) {
        let whole = data.len() / 16 * 16;
        let Some(data) = data.get_mut(..whole) else {
            return;
        };
        let (blocks, _) = aes::cipher::Block::<aes::Aes128>::slice_as_chunks_mut(data);
        self.mode.decrypt_blocks(blocks);
    }
}

/// One H.264 NAL unit of a scrambled transport stream (its start code gone, its emulation
/// prevention in place), in the clear: a coded slice (types 1 and 5) longer than 48 bytes has
/// its emulation prevention taken off, then, after the type byte and 31 bytes in the clear, one
/// scrambled block in every ten (a block, then up to 144 bytes in the clear), one chain from the
/// vector. What comes back is the unit as the encoder wrote it. Any other unit is as it was.
pub(crate) fn clear_nal(unit: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Option<Vec<u8>> {
    let kind = unit.first()? & 0x1f;
    if !(kind == 1 || kind == 5) || unit.len() <= 48 {
        return None;
    }
    let mut raw = unescape(unit);
    let mut chain = Chain::new(key, iv)?;
    let mut at = 32usize;
    while at < raw.len() {
        let left = raw.len() - at;
        if left > 16 {
            if let Some(block) = raw.get_mut(at..at + 16) {
                chain.blocks(block);
            }
            at += 16;
        }
        at += (raw.len() - at).min(144);
    }
    Some(raw)
}

/// One ADTS frame's payload (after its 7- or 9-byte header) in the clear: 16 bytes in the clear,
/// then every whole block scrambled in one chain from the vector, and 0 to 15 bytes in the clear.
pub(crate) fn clear_adts(payload: &mut [u8], key: &[u8; 16], iv: &[u8; 16]) {
    if let (Some(rest), Some(mut chain)) = (payload.get_mut(16..), Chain::new(key, iv)) {
        chain.blocks(rest);
    }
}

/// A NAL unit without its emulation prevention: each `00 00 03` loses its `03`.
pub(crate) fn unescape(unit: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(unit.len());
    let mut zeros = 0;
    for &byte in unit {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        out.push(byte);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segment_a_byte_is_in_names_its_lock() {
        let one = Some(([1; 16], [2; 16]));
        let locks = Locks(vec![(0, None), (100, one), (250, None)]);
        assert_eq!(locks.at(0), None);
        assert_eq!(locks.at(99), None);
        assert_eq!(locks.at(100), one);
        assert_eq!(locks.at(249), one);
        assert_eq!(locks.at(250), None);
        assert!(locks.any());
        assert!(!Locks::default().any());
    }

    #[test]
    fn emulation_prevention_comes_off() {
        assert_eq!(unescape(&[0, 0, 3, 1, 0, 0, 3, 3]), [0, 0, 1, 0, 0, 3]);
        assert_eq!(unescape(&[5, 0, 3, 0, 0]), [5, 0, 3, 0, 0]);
    }

    #[test]
    fn short_units_and_others_are_left_alone() {
        let key = [7; 16];
        assert!(clear_nal(&[0x65; 48], &key, &key).is_none());
        assert!(clear_nal(&[0x67; 200], &key, &key).is_none());
        assert!(clear_nal(&[0x65; 49], &key, &key).is_some());
    }
}
