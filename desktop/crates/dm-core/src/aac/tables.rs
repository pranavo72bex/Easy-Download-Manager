//! The standard's tables this decoder needs.
//!
//! Only what can be set down with certainty is here. The Huffman books (one of 121 scale-factor
//! codes and eleven spectral books, several hundred code words each) and the band layouts of
//! sample rates other than 44.1 and 48 kHz are not: they are data of the standard that was not
//! to hand, and one wrong code word decodes to noise, not to an error. `COMPLETE` stays false,
//! so `super::decoder_ready` says no and dm does not offer MP3 on a computer with no system
//! decoder, until the books are supplied (`scalefactor_book` and `spectral_book` are the places
//! to fill in; the rest of the decoder is written against them).

use super::huff::Book;

/// True when every table the decoder needs is present and has been checked against the standard.
pub(super) const COMPLETE: bool = false;

/// The sample rates an AudioSpecificConfig names by index.
pub(super) const RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

/// The scale-factor book (121 symbols), when supplied.
pub(super) fn scalefactor_book() -> Option<Book> {
    None
}

/// Spectral book `number` (1..=11), when supplied.
pub(super) fn spectral_book(number: u8) -> Option<Book> {
    let _ = number;
    None
}

const LONG_44_48: [u16; 50] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 48, 56, 64, 72, 80, 88, 96, 108, 120, 132, 144, 160,
    176, 196, 216, 240, 264, 292, 320, 352, 384, 416, 448, 480, 512, 544, 576, 608, 640, 672, 704,
    736, 768, 800, 832, 864, 896, 928, 1024,
];
const SHORT_44_48: [u16; 15] = [0, 4, 8, 12, 16, 20, 28, 36, 44, 56, 68, 80, 96, 112, 128];

/// Band edges for the long (1024 lines) or short (128 lines) window at a sample-rate index.
pub(super) fn band_edges(rate_index: usize, short: bool) -> Option<&'static [u16]> {
    match (rate_index, short) {
        (3 | 4, false) => Some(&LONG_44_48),
        (3 | 4, true) => Some(&SHORT_44_48),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_edges_rise_and_end_at_the_frame() {
        for (edges, end) in [(&LONG_44_48[..], 1024), (&SHORT_44_48[..], 128)] {
            assert!(edges.windows(2).all(|w| w[0] < w[1]));
            assert_eq!(edges.last().copied(), Some(end));
            assert_eq!(edges.first().copied(), Some(0));
        }
    }

    #[test]
    fn the_decoder_stays_off_while_a_book_is_missing() {
        const { assert!(!COMPLETE) };
        assert!(scalefactor_book().is_none());
        assert!(spectral_book(1).is_none());
    }
}
