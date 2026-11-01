//! Prefix codes as the standard lists them: a code word and its length for each symbol.

use super::bits::Bits;
use std::collections::HashMap;

pub(super) struct Book {
    /// (length, code) -> symbol index.
    map: HashMap<(u8, u32), u16>,
    longest: u8,
}

impl Book {
    /// `lengths[i]` bits long and `codes[i]` is symbol i's code word. A book whose arrays differ
    /// in size, or which has two symbols with one code, is refused.
    pub(super) fn new(lengths: &[u8], codes: &[u32]) -> Result<Book, String> {
        if lengths.len() != codes.len() || lengths.len() > usize::from(u16::MAX) {
            return Err("a Huffman book whose lengths and codes differ".into());
        }
        let mut map = HashMap::new();
        let mut longest = 0;
        for (index, (&len, &code)) in lengths.iter().zip(codes).enumerate() {
            if len == 0 || len > 32 {
                return Err("a Huffman book with a code of no length".into());
            }
            if map.insert((len, code), index as u16).is_some() {
                return Err("a Huffman book with one code twice".into());
            }
            longest = longest.max(len);
        }
        Ok(Book { map, longest })
    }

    /// The symbol whose code comes next in `bits`.
    pub(super) fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let mut code = 0u32;
        for len in 1..=self.longest {
            code = (code << 1) | bits.bit()?;
            if let Some(&symbol) = self.map.get(&(len, code)) {
                return Ok(symbol);
            }
        }
        Err("a code word that is in no Huffman book".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_book_decodes_its_symbols_in_a_row() {
        // 0 -> a, 10 -> b, 110 -> c, 111 -> d
        let book = Book::new(&[1, 2, 3, 3], &[0b0, 0b10, 0b110, 0b111]).unwrap();
        let mut bits = Bits::new(&[0b0101_1011, 0b1000_0000]);
        let got: Vec<u16> = (0..5).map(|_| book.decode(&mut bits).unwrap()).collect();
        assert_eq!(got, vec![0, 1, 2, 3, 0]);
    }

    #[test]
    fn a_code_in_no_book_and_a_bad_book_are_errors() {
        let book = Book::new(&[1, 2], &[0b0, 0b10]).unwrap();
        let mut bits = Bits::new(&[0b1111_1111]);
        assert!(book.decode(&mut bits).is_err());
        assert!(Book::new(&[1, 1], &[0, 0]).is_err());
        assert!(Book::new(&[1], &[0, 1]).is_err());
        assert!(Book::new(&[0], &[0]).is_err());
    }
}
