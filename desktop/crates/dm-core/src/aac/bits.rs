//! A big-endian bit reader over a byte slice; reading past the end is an error, never a panic.

pub(super) struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Bits { data, pos: 0 }
    }

    #[allow(
        dead_code,
        reason = "used once the decoder's tables are in (gaps row 53)"
    )]
    pub(super) fn position(&self) -> usize {
        self.pos
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

    pub(super) fn skip(&mut self, count: usize) -> Result<(), String> {
        if count > self.left() {
            return Err("the AAC data ended inside a frame".into());
        }
        self.pos += count;
        Ok(())
    }

    pub(super) fn align(&mut self) {
        self.pos = self.pos.div_ceil(8) * 8;
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
    fn reading_past_the_end_fails_and_align_rounds_up() {
        let mut r = Bits::new(&[0x80]);
        assert!(r.bits(9).is_err());
        r.skip(3).unwrap();
        r.align();
        assert_eq!(r.position(), 8);
        assert!(r.skip(1).is_err());
    }
}
