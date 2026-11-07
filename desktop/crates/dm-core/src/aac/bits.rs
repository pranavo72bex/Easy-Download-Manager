// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A big-endian bit reader over a byte slice; reading past the end is an error, never a panic.

pub(super) struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Bits { data, pos: 0 }
    }

    pub(super) fn left(&self) -> usize {
        self.data.len().saturating_mul(8).saturating_sub(self.pos)
    }

    pub(super) fn bit(&mut self) -> Result<u32, String> {
        let byte = self
            .data
            .get(self.pos / 8)
            .ok_or("the AAC data ended inside a frame")?;
        let bit = (byte >> (7 - (self.pos % 8))) & 1;
        self.pos += 1;
        Ok(u32::from(bit))
    }

    /// `count` bits (at most 32), most significant first.
    pub(super) fn bits(&mut self, count: usize) -> Result<u32, String> {
        if count > 32 || count > self.left() {
            return Err("the AAC data ended inside a frame".into());
        }
        let mut value = 0u64;
        for _ in 0..count {
            value = (value << 1) | u64::from(self.bit()?);
        }
        Ok(value as u32)
    }

    pub(super) fn flag(&mut self) -> Result<bool, String> {
        Ok(self.bit()? == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_read_most_significant_first() {
        let mut r = Bits::new(&[0b1010_0101, 0xff]);
        assert_eq!(r.bits(3).unwrap(), 0b101);
        assert_eq!(r.bits(5).unwrap(), 0b00101);
        assert_eq!(r.bits(8).unwrap(), 0xff);
        assert!(r.bit().is_err());
    }

    #[test]
    fn reading_past_the_end_fails() {
        let mut r = Bits::new(&[0x80]);
        assert!(r.bits(9).is_err());
        assert_eq!(r.bits(8).unwrap(), 0x80);
        assert!(r.flag().is_err());
    }
}
