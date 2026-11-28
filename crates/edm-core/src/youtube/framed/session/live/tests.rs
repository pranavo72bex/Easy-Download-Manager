// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! When a live recording reads its video again (gaps row Y2): the broadcast standing still a few
//! pieces' length, the head moving on counted as news, and half a minute between readings.

use super::*;

fn head(sequence: u64) -> Option<Head> {
    Some(Head {
        sequence,
        time_ms: Some(sequence * 500),
    })
}

#[test]
fn a_broadcast_standing_still_a_few_pieces_is_read_again() {
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    let mut watch = Watch::new(start);
    // Pieces of half a second: four of them still before it is read again.
    watch.heard(head(10), Some(500), 1, at(0));
    assert_eq!(watch.piece(), Duration::from_millis(500));
    assert!(!watch.still(at(1_999)));
    assert!(watch.still(at(2_000)));
    // The head moving on is news, though no piece came.
    watch.heard(head(11), None, 0, at(1_500));
    assert!(!watch.still(at(3_000)));
    assert!(watch.still(at(3_500)));
    // The same head said again, or one behind it, is not; nor does it go back.
    watch.heard(head(11), None, 0, at(3_400));
    watch.heard(head(9), None, 0, at(3_450));
    assert!(watch.still(at(3_500)));
    assert_eq!(watch.head.map(|h| h.sequence), Some(11));
    // Read again: not again for half a minute, however long it stands still.
    watch.read_at = Some(at(3_500));
    assert!(!watch.still(at(10_000)));
    assert!(!watch.still(at(33_499)));
    assert!(watch.still(at(33_500)));
}

#[test]
fn a_piece_s_length_is_guessed_until_the_stream_names_one() {
    let start = Instant::now();
    let mut watch = Watch::new(start);
    assert_eq!(watch.piece(), PIECE_GUESS);
    // A header that names none, or nought, leaves the guess.
    watch.heard(None, Some(0), 0, start);
    assert_eq!(watch.piece(), PIECE_GUESS);
    assert!(!watch.still(start + PIECE_GUESS * STILL_PIECES - Duration::from_millis(1)));
    assert!(watch.still(start + PIECE_GUESS * STILL_PIECES));
    // The longest named is kept.
    watch.heard(None, Some(1_000), 1, start);
    watch.heard(None, Some(200), 1, start);
    assert_eq!(watch.piece(), Duration::from_secs(1));
}
