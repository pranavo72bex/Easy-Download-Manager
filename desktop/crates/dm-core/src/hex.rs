//! Bytes as lower-case hexadecimal text, and back.

/// `bytes` as lower-case hexadecimal, two digits to a byte.
pub fn encode(bytes: impl AsRef<[u8]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 15)] as char);
    }
    out
}

/// The bytes `text` spells, or `None` when it has an odd length or a
/// character that is not a hexadecimal digit (either case is read).
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let digits = text.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    let value = |d: u8| char::from(d).to_digit(16).map(|v| v as u8);
    digits
        .chunks(2)
        .map(|pair| Some(value(pair[0])? << 4 | value(pair[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip_through_text() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)), Some(all));
        assert_eq!(encode([0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(encode(b""), "");
    }

    #[test]
    fn either_case_is_read_and_bad_text_is_refused() {
        assert_eq!(decode("A5fF"), Some(vec![0xa5, 0xff]));
        assert_eq!(decode("abc"), None);
        assert_eq!(decode("zz"), None);
        assert_eq!(decode("a b"), None);
        assert_eq!(decode(""), Some(Vec::new()));
    }
}
