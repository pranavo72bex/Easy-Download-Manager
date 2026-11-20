// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The framed stream's readers and writers on bytes built here, and which formats are kept for
//! it (gaps row Y2).

use super::messages::{self, kind, Ask, FormatStart, Held, MediaHeader, NextAsk};
use super::wire::{self, Message, Value as Wire, Writer};
use super::*;

#[test]
fn a_varint_reads_back_as_written() {
    for value in [
        0u64,
        1,
        127,
        128,
        300,
        16_383,
        16_384,
        u32::MAX as u64,
        u64::MAX,
    ] {
        let mut bytes = Vec::new();
        wire::put_varint(&mut bytes, value);
        assert_eq!(wire::varint(&bytes), Some((value, bytes.len())), "{value}");
    }
    // The example of Protocol Buffers' own description: 150 is 96 01.
    assert_eq!(wire::varint(&[0x96, 0x01]), Some((150, 2)));
    // Cut off, or longer than ten bytes: not read.
    assert_eq!(wire::varint(&[0x96]), None);
    assert_eq!(wire::varint(&[0xff; 11]), None);
}

#[test]
fn a_message_reads_back_by_field_number() {
    let mut w = Writer::new();
    w.uint(1, 7)
        .text(2, "abc")
        .fixed32(3, 0x0102_0304)
        .fixed64(4, 9)
        .int(5, -2)
        .message(6, |inner| {
            inner.uint(1, 140);
        })
        .uint(1, 8);
    let bytes = w.finish();
    let m = Message(&bytes);
    assert!(m.whole());
    // A field written twice is its last.
    assert_eq!(m.uint(1), Some(8));
    assert_eq!(m.text(2), Some("abc"));
    assert_eq!(m.uint(3), Some(0x0102_0304));
    assert_eq!(m.uint(4), Some(9));
    assert_eq!(m.int(5), Some(-2));
    assert_eq!(m.message(6).and_then(|i| i.uint(1)), Some(140));
    assert_eq!(m.uint(9), None);
    let numbers: Vec<u32> = m.fields().map(|(n, _)| n).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5, 6, 1]);
    assert_eq!(m.fields().nth(1), Some((2, Wire::Bytes(b"abc"))));
    // Cut short in its last field: what comes before it is read, and it is said to be broken.
    let cut = Message(&bytes[..bytes.len() - 1]);
    assert!(!cut.whole());
    assert_eq!(cut.text(2), Some("abc"));
}

#[test]
fn ump_numbers_take_the_bytes_their_first_byte_says() {
    // One byte under 128; then 2, 3, 4 bytes by the leading ones; five past 28 bits.
    let cases: [(u64, &[u8]); 6] = [
        (5, &[0x05]),
        (127, &[0x7f]),
        (128, &[0x80, 0x02]),
        (20, &[0x14]),
        (16_384, &[0xc0, 0x00, 0x02]),
        (1 << 28, &[0xf0, 0x00, 0x00, 0x00, 0x10]),
    ];
    for (value, bytes) in cases {
        assert_eq!(ump::varint(bytes), Some((value, bytes.len())), "{value}");
        let mut written = Vec::new();
        ump::put_varint(&mut written, value);
        assert_eq!(written, bytes, "{value}");
    }
    for value in [
        0u64,
        63,
        64,
        16_383,
        2_097_151,
        2_097_152,
        268_435_455,
        u32::MAX as u64,
    ] {
        let mut written = Vec::new();
        ump::put_varint(&mut written, value);
        assert_eq!(
            ump::varint(&written),
            Some((value, written.len())),
            "{value}"
        );
    }
    assert_eq!(ump::varint(&[0xc0, 0x00]), None);
}

#[test]
fn parts_are_read_whole_however_the_answer_is_cut() {
    let mut answer = Vec::new();
    ump::put_part(&mut answer, kind::MEDIA_HEADER, b"head");
    ump::put_part(&mut answer, kind::MEDIA, &[7u8; 300]);
    ump::put_part(&mut answer, kind::MEDIA_END, &[0]);
    for cut in [1, 2, 3, 7, 64, answer.len()] {
        let mut reader = ump::Reader::new();
        let mut parts = Vec::new();
        for chunk in answer.chunks(cut) {
            reader.push(chunk);
            while let Some(part) = reader.next_part().unwrap() {
                parts.push(part);
            }
        }
        assert_eq!(reader.left(), 0, "cut at {cut}");
        assert_eq!(parts.len(), 3, "cut at {cut}");
        assert_eq!(parts[0], (kind::MEDIA_HEADER, b"head".to_vec()));
        assert_eq!(parts[1].1.len(), 300);
        assert_eq!(parts[2], (kind::MEDIA_END, vec![0]));
    }
    // A part that says it is larger than any piece is refused, not waited for.
    let mut huge = Vec::new();
    ump::put_varint(&mut huge, kind::MEDIA);
    ump::put_varint(&mut huge, ump::MOST_PART + 1);
    assert!(ump::part(&huge).is_err());
}

#[test]
fn the_stream_s_messages_are_read_by_their_numbers() {
    let mut w = Writer::new();
    w.uint(1, 3)
        .uint(3, 999)
        .flag(8, false)
        .uint(9, 4)
        .uint(11, 10_000)
        .uint(12, 5_000)
        .message(13, |f| {
            f.uint(1, 137).uint(2, 1_712_000_000_000_000).text(3, "CgQ");
        });
    let header = MediaHeader::read(&w.finish()).unwrap();
    // The format inside it is taken over the plain itag.
    assert_eq!(header.itag, Some(137));
    assert_eq!(header.lmt, Some(1_712_000_000_000_000));
    assert_eq!((header.header_id, header.sequence), (3, Some(4)));
    assert_eq!(
        (header.start_ms, header.duration_ms),
        (Some(10_000), Some(5_000))
    );
    assert!(!header.init && !header.compressed);
    assert_eq!(messages::media_of(&[3, 1, 2]), Some((3, &[1u8, 2][..])));

    let mut w = Writer::new();
    w.message(2, |f| {
        f.uint(1, 140);
    })
    .uint(3, 60_000)
    .uint(4, 12);
    let start = FormatStart::read(&w.finish());
    assert_eq!(
        (start.itag, start.end_ms, start.end_segment),
        (Some(140), Some(60_000), Some(12))
    );

    let mut w = Writer::new();
    w.uint(4, 250).bytes(7, b"cookie");
    let next = NextAsk::read(&w.finish());
    assert_eq!(
        (next.backoff_ms, next.cookie.as_deref()),
        (Some(250), Some(&b"cookie"[..]))
    );

    let mut w = Writer::new();
    w.text(1, "sabr.malformed_config").uint(2, 3);
    assert_eq!(messages::error_of(&w.finish()), "sabr.malformed_config (3)");
    let mut w = Writer::new();
    w.uint(1, 3);
    assert_eq!(messages::protection_of(&w.finish()), Some(3));
}

#[test]
fn a_request_names_the_formats_what_is_held_and_the_config() {
    let video = Pick {
        itag: 137,
        lmt: Some(11),
        xtags: None,
        video: true,
        height: 1080,
    };
    let audio = Pick {
        itag: 140,
        lmt: Some(22),
        xtags: Some("CgQ".into()),
        video: false,
        height: 0,
    };
    let held = Held {
        pick: &video,
        duration_ms: 10_000,
        first: 1,
        last: 2,
    };
    let ask = Ask {
        config: b"config",
        client_name: 7,
        client_version: "5.1",
        player_time_ms: 10_000,
        height: 1080,
        audio: vec![&audio],
        video: vec![&video],
        started: vec![&video],
        held: vec![held],
        cookie: Some(&b"c"[..]),
    };
    let body = ask.body();
    let m = Message(&body);
    assert!(m.whole());
    assert_eq!(m.bytes(5), Some(&b"config"[..]));
    assert_eq!(m.uint(4), Some(10_000));
    let state = m.message(1).unwrap();
    assert_eq!(
        (state.uint(16), state.uint(28), state.uint(40)),
        (Some(1080), Some(10_000), Some(0))
    );
    assert_eq!(m.message(16).and_then(|f| f.uint(1)), Some(140));
    assert_eq!(m.message(16).and_then(|f| f.text(3)), Some("CgQ"));
    assert_eq!(m.message(17).and_then(|f| f.uint(2)), Some(11));
    assert_eq!(m.message(2).and_then(|f| f.uint(1)), Some(137));
    let range = m.message(3).unwrap();
    assert_eq!(range.message(1).and_then(|f| f.uint(1)), Some(137));
    assert_eq!(
        (range.uint(3), range.uint(4), range.uint(5)),
        (Some(10_000), Some(1), Some(2))
    );
    let context = m.message(19).unwrap();
    assert_eq!(context.message(1).and_then(|c| c.uint(16)), Some(7));
    assert_eq!(context.message(1).and_then(|c| c.text(17)), Some("5.1"));
    assert_eq!(context.bytes(3), Some(&b"c"[..]));
    // No proof-of-origin token.
    assert_eq!(context.bytes(2), None);
}

#[test]
fn xtags_written_as_the_stream_writes_them_are_read() {
    use base64::Engine as _;
    let mut w = Writer::new();
    w.message(1, |p| {
        p.text(1, "acont").text(2, "original");
    })
    .message(1, |p| {
        p.text(1, "lang").text(2, "es");
    });
    let written = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(w.finish());
    let pairs = messages::xtag_pairs(&written);
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(pairs, [pair("acont", "original"), pair("lang", "es")]);
    assert!(messages::xtag_pairs("not base64 at all!").is_empty());
}

#[test]
fn framed_formats_are_kept_only_where_nothing_with_an_address_is_the_same() {
    let read = |json: Value| super::super::Format::of(&json).unwrap();
    let mp4 = "video/mp4; codecs=\"avc1.640028\"";
    let plain = read(
        json!({"itag": 136, "mimeType": mp4, "width": 1280, "height": 720, "url": "https://r.example/v?itag=136"}),
    );
    let same = read(
        json!({"itag": 136, "mimeType": mp4, "width": 1280, "height": 720, "lastModified": "5"}),
    );
    let taller = read(
        json!({"itag": 137, "mimeType": mp4, "width": 1920, "height": 1080, "lastModified": "6"}),
    );
    assert!(same.framed && taller.framed && !plain.framed);
    assert_eq!(taller.lmt, Some(6));
    let mut formats = vec![plain.clone(), same.clone(), taller.clone()];
    keep_framed(&mut formats, true);
    let itags: Vec<u64> = formats.iter().map(|f| f.itag).collect();
    assert_eq!(itags, [136, 137]);
    assert!(!formats[0].framed);
    // With no framed stream named, none of them is kept.
    let mut formats = vec![plain, same, taller];
    keep_framed(&mut formats, false);
    assert_eq!(formats.len(), 1);
}
