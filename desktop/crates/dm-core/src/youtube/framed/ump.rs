// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The framing of YouTube's framed answers (UMP, `application/vnd.yt-ump`): a run of parts,
//! each its kind, then its size, then that many bytes. The kind and the size are written in
//! UMP's own variable-length numbers, not Protocol Buffers' varints: the leading 1 bits of the
//! first byte say how many bytes follow it (none to three, the rest of the byte holding the
//! number's low bits; four, a whole 32-bit number after a first byte of five 1 bits).

/// The largest part dm holds: a piece of a picture is megabytes, never this.
pub const MOST_PART: u64 = 128 << 20;

/// The number at the start of `data` and how many bytes it took; `None` where it is cut off.
pub fn varint(data: &[u8]) -> Option<(u64, usize)> {
    let first = *data.first()?;
    let size = match first {
        0x00..=0x7f => 1,
        0x80..=0xbf => 2,
        0xc0..=0xdf => 3,
        0xe0..=0xef => 4,
        _ => 5,
    };
    let b = data.get(..size)?;
    let byte = |i: usize| u64::from(b[i]);
    let value = match size {
        1 => u64::from(first),
        2 => u64::from(first & 0x3f) + 64 * byte(1),
        3 => u64::from(first & 0x1f) + 32 * (byte(1) + 256 * byte(2)),
        4 => u64::from(first & 0x0f) + 16 * (byte(1) + 256 * (byte(2) + 256 * byte(3))),
        _ => u64::from(u32::from_le_bytes([b[1], b[2], b[3], b[4]])),
    };
    Some((value, size))
}

/// `value` written at the end of `into` in the fewest bytes UMP's numbers take (a value past
/// 32 bits is written as the largest 32-bit one, which no part kind or size is).
pub fn put_varint(into: &mut Vec<u8>, value: u64) {
    let low = |shift: u32| ((value >> shift) & 0xff) as u8;
    if value < 0x80 {
        into.push(value as u8);
    } else if value < 1 << 14 {
        into.extend_from_slice(&[0x80 | (value & 0x3f) as u8, low(6)]);
    } else if value < 1 << 21 {
        into.extend_from_slice(&[0xc0 | (value & 0x1f) as u8, low(5), low(13)]);
    } else if value < 1 << 28 {
        into.extend_from_slice(&[0xe0 | (value & 0x0f) as u8, low(4), low(12), low(20)]);
    } else {
        let whole = u32::try_from(value).unwrap_or(u32::MAX);
        into.push(0xf0);
        into.extend_from_slice(&whole.to_le_bytes());
    }
}

/// A part written whole at the end of `into`: its kind, its size and its bytes.
pub fn put_part(into: &mut Vec<u8>, kind: u64, data: &[u8]) {
    put_varint(into, kind);
    put_varint(into, data.len() as u64);
    into.extend_from_slice(data);
}

/// The part at the start of `data`, whole: its kind, its bytes and how many bytes it took in
/// all. `Ok(None)` where it is not all there yet; an error where it says it is larger than
/// [`MOST_PART`].
pub fn part(data: &[u8]) -> Result<Option<(u64, &[u8], usize)>, String> {
    let Some((kind, at)) = varint(data) else {
        return Ok(None);
    };
    let Some((size, used)) = data.get(at..).and_then(varint) else {
        return Ok(None);
    };
    if size > MOST_PART {
        return Err(format!(
            "a part of {size} bytes, larger than any piece of a video"
        ));
    }
    let start = at + used;
    let end = start + size as usize;
    Ok(data.get(start..end).map(|bytes| (kind, bytes, end)))
}

/// Parts read from an answer as its bytes come: what is pushed is held until a part is whole.
#[derive(Debug, Default)]
pub struct Reader {
    held: Vec<u8>,
    at: usize,
}

impl Reader {
    /// A reader holding nothing.
    pub fn new() -> Reader {
        Reader::default()
    }

    /// More of the answer.
    pub fn push(&mut self, bytes: &[u8]) {
        // What was read already is let go once it is most of what is held.
        if self.at > 0 && self.at * 2 >= self.held.len() {
            self.held.drain(..self.at);
            self.at = 0;
        }
        self.held.extend_from_slice(bytes);
    }

    /// The next whole part, taken: its kind and its bytes. `Ok(None)` until one is whole.
    pub fn next_part(&mut self) -> Result<Option<(u64, Vec<u8>)>, String> {
        let rest = self.held.get(self.at..).unwrap_or_default();
        match part(rest)? {
            Some((kind, bytes, used)) => {
                let bytes = bytes.to_vec();
                self.at += used;
                Ok(Some((kind, bytes)))
            }
            None => Ok(None),
        }
    }

    /// How many bytes are held of a part not yet whole: none where the answer ended cleanly.
    pub fn left(&self) -> usize {
        self.held.len().saturating_sub(self.at)
    }
}
