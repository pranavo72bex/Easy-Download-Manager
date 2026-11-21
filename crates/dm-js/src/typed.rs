// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An `ArrayBuffer`'s bytes and the typed arrays over them: each kind's element read and
//! written in little-endian order, as the machines dm runs on store them.

use std::cell::RefCell;
use std::rc::Rc;

/// Bytes, shared by the buffer and every view of it, and how a view reads them.
pub struct Bytes {
    /// The buffer's bytes.
    pub buffer: Rc<RefCell<Vec<u8>>>,
    /// Where the view starts, in bytes.
    pub offset: usize,
    /// How many elements it has (bytes, for the buffer itself).
    pub length: usize,
    /// 0 the buffer itself, else the typed array's kind (see [`KINDS`]).
    pub kind: u8,
}

/// Each kind's name and element size, by its number (1 to 9).
pub const KINDS: [(&str, usize); 10] = [
    ("ArrayBuffer", 1),
    ("Int8Array", 1),
    ("Uint8Array", 1),
    ("Uint8ClampedArray", 1),
    ("Int16Array", 2),
    ("Uint16Array", 2),
    ("Int32Array", 4),
    ("Uint32Array", 4),
    ("Float32Array", 4),
    ("Float64Array", 8),
];

impl Bytes {
    /// The element size.
    pub fn size(&self) -> usize {
        KINDS[self.kind as usize].1
    }

    /// Element `i`, where there is one.
    pub fn get(&self, i: usize) -> Option<f64> {
        if i >= self.length {
            return None;
        }
        let b = self.buffer.borrow();
        let at = self.offset + i * self.size();
        let s = b.get(at..at + self.size())?;
        Some(match self.kind {
            1 => s[0] as i8 as f64,
            2 | 3 => s[0] as f64,
            4 => i16::from_le_bytes([s[0], s[1]]) as f64,
            5 => u16::from_le_bytes([s[0], s[1]]) as f64,
            6 => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64,
            7 => u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64,
            8 => f32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64,
            9 => f64::from_le_bytes(s.try_into().unwrap_or([0; 8])),
            _ => s[0] as f64,
        })
    }

    /// Sets element `i` to `n` (out of range: nothing).
    pub fn set(&self, i: usize, n: f64) {
        if i >= self.length {
            return;
        }
        let size = self.size();
        let at = self.offset + i * size;
        let mut b = self.buffer.borrow_mut();
        let Some(s) = b.get_mut(at..at + size) else {
            return;
        };
        let int = crate::value::to_int32(n);
        match self.kind {
            1 | 2 => s[0] = int as u8,
            3 => {
                s[0] = if n.is_nan() {
                    0
                } else {
                    n.round().clamp(0.0, 255.0) as u8
                }
            }
            4 | 5 => s.copy_from_slice(&(int as u16).to_le_bytes()),
            6 | 7 => s.copy_from_slice(&crate::value::to_uint32(n).to_le_bytes()),
            8 => s.copy_from_slice(&(n as f32).to_le_bytes()),
            9 => s.copy_from_slice(&n.to_le_bytes()),
            _ => s[0] = int as u8,
        }
    }
}
