// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's Opus decoder: the range decoder against known answers worked from RFC 6716's
//! arithmetic, a packet's framing, the stream header, whole WebM files, and (by hand) the
//! RFC's own test vectors.

use super::*;
use std::path::PathBuf;

/// The range decoder reads what the RFC's arithmetic says these bytes hold: raw bits from the
/// end, low bits first; a silent frame's flag; a run of symbols of each kind; and zeros past
/// either end, never a panic.
#[test]
fn the_range_decoder_reads_known_answers() {
    let mut dec = RangeDecoder::new(&[0x00, 0xA5]);
    assert_eq!((dec.bits(4), dec.bits(4), dec.bits(8)), (5, 10, 0));
    let mut dec = RangeDecoder::new(&[0xFF, 0xFE]);
    assert_eq!((dec.tell(), dec.tell_frac()), (1, 8));
    assert!(dec.bit_logp(15));
    assert_eq!(dec.tell(), 16);
    let mut dec = RangeDecoder::new(&[0x12, 0x34, 0x56, 0x78, 0x9a]);
    assert_eq!(dec.uint(3), 0);
    assert_eq!(dec.uint(1000), 214);
    assert!(!dec.bit_logp(1));
    assert_eq!(
        dec.icdf(&[126, 124, 119, 109, 87, 41, 19, 9, 4, 2, 0], 7),
        5
    );
    assert_eq!(dec.bits(3), 6);
    assert_eq!((dec.tell(), dec.tell_frac()), (19, 145));
    assert_eq!(dec.range(), 0x07d9_c532);
    let mut dec = RangeDecoder::new(&[]);
    assert_eq!(dec.tell(), 1);
    for _ in 0..100 {
        dec.uint(10);
        dec.bits(24);
        dec.icdf(&[25, 23, 2, 0], 5);
        range::laplace(&mut dec, 72 << 7, 127 << 6);
    }
}

/// A packet's first byte: mode, frame size, channels and bandwidth (RFC 6716, table 2).
#[test]
fn a_packets_table_of_contents_is_read() {
    use packet::{toc, Mode};
    let celt = toc(0xF8);
    assert_eq!(
        (celt.mode, celt.frame_size, celt.stereo, celt.end_band),
        (Mode::Celt, 960, false, 21)
    );
    assert!(toc(0xFC).stereo);
    let wide = toc(23 << 3);
    assert_eq!(
        (wide.mode, wide.frame_size, wide.end_band),
        (Mode::Celt, 960, 17)
    );
    let short = toc(16 << 3);
    assert_eq!(
        (short.mode, short.frame_size, short.end_band),
        (Mode::Celt, 120, 13)
    );
    assert_eq!(toc(15 << 3).mode, Mode::Hybrid);
    let speech = toc(9 << 3);
    assert_eq!((speech.mode, speech.frame_size), (Mode::Silk, 960));
}

/// The four ways a packet holds its frames (RFC 6716, 3.2), and damaged ones refused.
#[test]
fn a_packets_frames_are_cut_out() {
    use packet::frames;
    assert_eq!(frames(&[0xF8, 1, 2, 3]).unwrap(), [&[1u8, 2, 3][..]]);
    assert_eq!(
        frames(&[0xF9, 1, 2, 3, 4]).unwrap(),
        [&[1u8, 2][..], &[3, 4][..]]
    );
    assert!(frames(&[0xF9, 1, 2, 3]).is_err());
    assert_eq!(
        frames(&[0xFA, 2, 1, 2, 3, 4, 5]).unwrap(),
        [&[1u8, 2][..], &[3, 4, 5][..]]
    );
    assert!(frames(&[0xFA, 9, 1]).is_err());
    assert_eq!(
        frames(&[0xFB, 0x03, 1, 2, 3, 4, 5, 6]).unwrap(),
        [&[1u8, 2][..], &[3, 4][..], &[5, 6][..]]
    );
    assert_eq!(
        frames(&[0xFB, 0x82, 1, 7, 8, 9]).unwrap(),
        [&[7u8][..], &[8, 9][..]]
    );
    assert_eq!(
        frames(&[0xFB, 0x41, 2, 7, 8, 0, 0]).unwrap(),
        [&[7u8, 8][..]]
    );
    assert!(frames(&[0xFB, 0x00]).is_err());
    // Seven frames of 20 ms are more than a packet's 120 ms.
    assert!(frames(&[0xFB, 0x07, 1, 2, 3, 4, 5, 6, 7]).is_err());
    assert!(frames(&[]).is_err());
}

proptest::proptest! {
    #[test]
    fn any_packet_is_read_without_a_panic(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..300)) {
        let _ = packet::frames(&bytes);
        let _ = packet::head(&bytes);
        let mut decoder = Decoder::new(2);
        let mut out = Vec::new();
        let _ = decoder.decode(&bytes, &mut out);
        proptest::prop_assert!(out.iter().all(|v| v.is_finite()));
    }
}

/// The header of a stream (RFC 7845, 5.1): its channels, pre-skip and gain; more than two
/// channels refused in words.
#[test]
fn the_opus_header_is_read() {
    let mut bytes = b"OpusHead".to_vec();
    bytes.extend_from_slice(&[1, 2, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0, 0, 0]);
    let head = packet::head(&bytes).unwrap();
    assert_eq!((head.channels, head.pre_skip, head.gain), (2, 312, 1.0));
    // +6 dB in Q7.8.
    bytes[16..18].copy_from_slice(&(6i16 * 256).to_le_bytes());
    let gain = packet::head(&bytes).unwrap().gain;
    assert!((gain - 1.995).abs() < 0.01, "{gain}");
    let mut surround = bytes.clone();
    surround[9] = 6;
    surround[18] = 1;
    surround.extend_from_slice(&[4, 2, 0, 4, 1, 2, 3, 5]);
    let said = packet::head(&surround).unwrap_err().to_string();
    assert!(said.contains("6 channels"), "{said}");
    assert!(packet::head(b"OpusTags").is_err());
}

/// Packets of silence decode to silence, a packet that is missing to its length of sound,
/// and a SILK or hybrid packet is refused in words.
#[test]
fn packets_decode_and_silk_is_refused_in_words() {
    for (channels, toc) in [(1usize, 0xF8u8), (2, 0xFC)] {
        let mut decoder = Decoder::new(channels);
        let mut out = Vec::new();
        for _ in 0..5 {
            decoder.decode(&[toc, 0xFF, 0xFE], &mut out).unwrap();
        }
        assert_eq!(out.len(), 5 * 960 * channels);
        decoder.decode(&[], &mut out).unwrap();
        assert_eq!(out.len(), 6 * 960 * channels);
        assert!(out.iter().all(|v| v.abs() < 1e-3));
    }
    let mut decoder = Decoder::new(1);
    for silk in [9u8 << 3, 15 << 3, 0] {
        let said = decoder
            .decode(&[silk, 1, 2, 3], &mut Vec::new())
            .unwrap_err();
        assert_eq!(said.as_str(), NO_SILK);
    }
}

/// An Opus packet of 20 ms of silence: CELT, full band, one frame, the silence flag set.
fn silent(channels: u8) -> Vec<u8> {
    vec![if channels == 2 { 0xFC } else { 0xF8 }, 0xFF, 0xFE]
}

/// A WebM of Opus decodes as long as it is, less the pre-skip its header names, on every
/// system; a real YouTube track that uses SILK is refused in words.
#[test]
fn a_webms_opus_is_read_as_long_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    for channels in [1u8, 2] {
        let webm = dir.path().join(format!("sound{channels}.webm"));
        crate::mkvmux::testing::opus_webm(&webm, channels, 312, 20, &vec![silent(channels); 100]);
        let pcm = read(&webm).unwrap();
        assert_eq!((pcm.channels, pcm.rate), (usize::from(channels), 48_000));
        assert_eq!(pcm.samples.len(), (96_000 - 312) * usize::from(channels));
        assert!(pcm.samples.iter().all(|s| s.unsigned_abs() < 2));
    }
    let vorbis = dir.path().join("vorbis.webm");
    crate::mkvmux::testing::tiny_webm(&vorbis, "A_VORBIS", 2, 4);
    let said = read(&vorbis).unwrap_err().to_string();
    assert!(said.contains("VORBIS"), "{said}");
    let youtube = crate::fixtures().join("media/mp4mux/av1-sound.f251.webm");
    assert_eq!(read(&youtube).unwrap_err().as_str(), NO_SILK);
    assert!(!decodable(&youtube));
    assert!(decodable(&dir.path().join("sound2.webm")));
    assert!(!decodable(&vorbis));
    // Where dm's decoder is the one used, a WebM it cannot read is not made an MP3 at all;
    // asked for anyway, it is refused in words.
    if !crate::mp3::decode::opus_decodable(&youtube) {
        assert!(!crate::mp3::takes(&youtube));
    }
    let said = crate::mp3::convert_bytes(&youtube, None);
    if let Err(said) = said {
        assert_eq!(said.as_str(), NO_SILK);
    }
}

/// RFC 6716's test vectors (as RFC 8251 renewed them), which are not kept here: fetched by
/// hand from opus-codec.org (`opus_testvectors-rfc8251.tar.gz`) into the folder that
/// `OPUS_VECTORS` names. Every CELT packet in them is read to the encoder's final range (each
/// symbol read as it was written), and the vectors of CELT alone sound as the reference
/// decoder's output within 50 dB.
#[test]
#[ignore = "needs RFC 8251's test vectors in the folder OPUS_VECTORS names"]
fn the_rfc_test_vectors_decode_to_the_encoders_final_range() {
    let Some(dir) = std::env::var_os("OPUS_VECTORS").map(PathBuf::from) else {
        eprintln!("OPUS_VECTORS names no folder: nothing to check");
        return;
    };
    for v in 1..=12 {
        let bits = std::fs::read(dir.join(format!("testvector{v:02}.bit"))).unwrap();
        let mut decoder = Decoder::new(2);
        let mut sound: Vec<f32> = Vec::new();
        let (mut matched, mut refused) = (0, 0);
        let mut at = 0;
        while at + 8 <= bits.len() {
            let len = u32::from_be_bytes(bits[at..at + 4].try_into().unwrap()) as usize;
            let range = u32::from_be_bytes(bits[at + 4..at + 8].try_into().unwrap());
            let packet = &bits[at + 8..at + 8 + len];
            at += 8 + len;
            let mut out = Vec::new();
            match decoder.decode(packet, &mut out) {
                Ok(()) => {
                    if !packet.is_empty() {
                        assert_eq!(
                            decoder.range, range,
                            "vector {v}, the packet ending at {at}"
                        );
                        matched += 1;
                    }
                    sound.extend(out);
                }
                Err(e) => {
                    assert_eq!(e.as_str(), NO_SILK);
                    refused += 1;
                }
            }
        }
        eprintln!("vector {v}: {matched} packets at the final range, {refused} SILK or hybrid");
        if refused > 0 {
            continue;
        }
        let reference = std::fs::read(dir.join(format!("testvector{v:02}.dec"))).unwrap();
        let (mut signal, mut noise) = (0f64, 0f64);
        for (made, want) in sound.iter().zip(reference.as_chunks::<2>().0) {
            let want = f64::from(i16::from_le_bytes(*want));
            let made = f64::from(made.round().clamp(-32768.0, 32767.0));
            signal += want * want;
            noise += (made - want) * (made - want);
        }
        let snr = 10.0 * (signal / noise.max(1e-9)).log10();
        eprintln!("vector {v}: {snr:.1} dB against the reference decoder's sound");
        assert!(snr > 50.0, "vector {v}: {snr:.1} dB");
    }
}
