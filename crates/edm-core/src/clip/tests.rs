// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A `sidx` box naming `pieces` of (size, duration), version 0.
pub(crate) fn sidx(timescale: u32, first_offset: u32, pieces: &[(u32, u32)]) -> Vec<u8> {
    let mut body = vec![0u8, 0, 0, 0]; // version 0, flags
    body.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes()); // earliest_presentation_time
    body.extend_from_slice(&first_offset.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&(pieces.len() as u16).to_be_bytes());
    for (size, duration) in pieces {
        body.extend_from_slice(&size.to_be_bytes());
        body.extend_from_slice(&duration.to_be_bytes());
        body.extend_from_slice(&0x9000_0000u32.to_be_bytes());
    }
    let mut whole = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    whole.extend_from_slice(b"sidx");
    whole.extend_from_slice(&body);
    whole
}

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut whole = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    whole.extend_from_slice(kind);
    whole.extend_from_slice(body);
    whole
}

#[test]
fn seconds_are_read_from_every_way_a_person_writes_them() {
    assert_eq!(Span::seconds("90"), Some(90.0));
    assert_eq!(Span::seconds("1:30"), Some(90.0));
    assert_eq!(Span::seconds("1:02:03"), Some(3723.0));
    assert_eq!(Span::seconds("1m30s"), Some(90.0));
    assert_eq!(Span::seconds("90s"), Some(90.0));
    assert_eq!(Span::seconds("2h"), Some(7200.0));
    assert_eq!(Span::seconds("1h2m3s"), Some(3723.0));
    assert_eq!(Span::seconds(" 4.5 "), Some(4.5));
    assert_eq!(Span::seconds("1:30.5"), Some(90.5));
    assert_eq!(Span::seconds(""), None);
    assert_eq!(Span::seconds("-5"), None);
    assert_eq!(Span::seconds("1:2:3:4"), None);
    assert_eq!(Span::seconds("soon"), None);
    assert_eq!(Span::parse("1:00", "0:30"), None, "an end before its start");
    assert_eq!(
        Span::parse("0:30", "1:00"),
        Some(Span {
            from: 30.0,
            to: 60.0
        })
    );
    assert_eq!(
        Span {
            from: 250.0,
            to: 285.0
        }
        .words(),
        "4m10s-4m45s"
    );
    assert_eq!(
        Span {
            from: 3723.0,
            to: 3800.0
        }
        .words(),
        "1h02m03s-1h03m20s"
    );
    assert_eq!(Span::clock(250.0), "4:10");
    assert_eq!(Span::clock(3723.4), "1:02:03");
    assert_eq!(
        Span {
            from: 1.26,
            to: 2.0
        }
        .millis(),
        (1260, 2000)
    );
}

#[test]
fn an_index_names_each_piece_with_where_it_is_and_when_it_plays() {
    // The index sits at 700 in its file and is 44 bytes long; its first
    // piece begins 10 bytes after it.
    let whole = sidx(1000, 10, &[(5000, 5000), (6000, 5000), (1000, 2500)]);
    assert_eq!(whole.len(), 32 + 36);
    let index = parse_sidx(&whole, 700).unwrap();
    assert_eq!(index.timescale, 1000);
    assert_eq!(
        index.pieces,
        [
            Piece {
                offset: 778,
                size: 5000,
                start: 0,
                duration: 5000
            },
            Piece {
                offset: 5778,
                size: 6000,
                start: 5000,
                duration: 5000
            },
            Piece {
                offset: 11778,
                size: 1000,
                start: 10000,
                duration: 2500
            },
        ]
    );
    assert_eq!(covering(&index, Span::new(0.0, 1.0).unwrap()), Some((0, 0)));
    assert_eq!(covering(&index, Span::new(4.0, 6.0).unwrap()), Some((0, 1)));
    assert_eq!(
        covering(&index, Span::new(5.0, 10.0).unwrap()),
        Some((1, 1))
    );
    assert_eq!(
        covering(&index, Span::new(7.0, 60.0).unwrap()),
        Some((1, 2))
    );
    assert_eq!(
        covering(&index, Span::new(12.5, 60.0).unwrap()),
        None,
        "past the end"
    );
    // A piece with no bytes, or no time, is never taken.
    let odd = parse_sidx(&sidx(1000, 0, &[(0, 5000), (5000, 0), (6000, 5000)]), 0).unwrap();
    assert_eq!(covering(&odd, Span::new(0.0, 60.0).unwrap()), Some((2, 2)));
    // Not an index: another box, one cut short, one naming indexes.
    assert_eq!(parse_sidx(&boxed(b"moov", &[0; 40]), 0), None);
    assert_eq!(parse_sidx(&whole[..whole.len() - 1], 0), None);
    let mut nested = sidx(1000, 0, &[(5000, 5000)]);
    nested[32] |= 0x80;
    assert_eq!(parse_sidx(&nested, 0), None);
}

#[test]
fn the_index_is_found_in_a_streams_first_bytes() {
    let ftyp = boxed(b"ftyp", b"isom\0\0\0\0isomiso2");
    let moov = boxed(b"moov", &[1; 100]);
    let index = sidx(1000, 0, &[(50, 1000)]);
    let moof = boxed(b"moof", &[2; 20]);
    let head = [ftyp.clone(), moov.clone(), index.clone(), moof].concat();
    let init_end = (ftyp.len() + moov.len()) as u64;
    assert_eq!(
        locate(&head),
        Some((init_end, (init_end, init_end + index.len() as u64 - 1)))
    );
    // Cut before the index is whole, or with no index at all.
    assert_eq!(locate(&head[..head.len() - 30]), None);
    assert_eq!(locate(&[ftyp.clone(), moov.clone()].concat()), None);
    let plain = [ftyp, moov, boxed(b"mdat", &[0; 10])].concat();
    assert_eq!(locate(&plain), None);
}
