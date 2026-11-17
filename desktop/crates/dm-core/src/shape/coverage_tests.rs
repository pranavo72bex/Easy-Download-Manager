// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use std::path::{Path, PathBuf};

fn media(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media/mp4mux")
        .join(name)
}

/// What an SPS says, for the builder below.
#[derive(Clone, Copy)]
struct Sps {
    profile: u64,
    /// `chroma_format_idc`, and the separate-planes flag where it is 3.
    chroma: Option<(u64, u64)>,
    scaling: bool,
    poc: u64,
    frames_only: u64,
    width_mbs: u64,
    height_units: u64,
    crop: Option<[u64; 4]>,
    vui: bool,
}

const PLAIN: Sps = Sps {
    profile: 77,
    chroma: None,
    scaling: false,
    poc: 0,
    frames_only: 1,
    width_mbs: 20,
    height_units: 10,
    crop: None,
    vui: false,
};

fn put_se(w: &mut Writer, v: i64) {
    w.put_ue(if v > 0 {
        2 * v as u64 - 1
    } else {
        (-2 * v) as u64
    });
}

/// The SPS NAL unit `s` describes, written bit by bit as H.264 lays it
/// out (7.3.2.1.1).
fn sps(s: Sps) -> Vec<u8> {
    let mut w = Writer::default();
    w.put(s.profile, 8);
    w.put(0, 8);
    w.put(30, 8);
    w.put_ue(0); // id
    if let Some((chroma, separate)) = s.chroma {
        w.put_ue(chroma);
        if chroma == 3 {
            w.put(separate, 1);
        }
        w.put_ue(0); // luma bit depth - 8
        w.put_ue(0); // chroma bit depth - 8
        w.put(0, 1); // lossless bypass
        w.put(u64::from(s.scaling), 1);
        if s.scaling {
            let lists = if chroma == 3 { 12 } else { 8 };
            for i in 0..lists {
                // Every other list present: the first of them given in
                // full (a delta of 0 sixteen or sixty-four times), the
                // rest ended at once (a delta to 0, "use the default").
                let present = i % 2 == 0;
                w.put(u64::from(present), 1);
                if present {
                    if i == 0 {
                        for _ in 0..16 {
                            put_se(&mut w, 0);
                        }
                    } else {
                        put_se(&mut w, -8);
                    }
                }
            }
        }
    }
    w.put_ue(0); // log2 max frame num - 4
    w.put_ue(s.poc);
    match s.poc {
        0 => w.put_ue(2),
        1 => {
            w.put(1, 1); // delta pic order always zero
            put_se(&mut w, -3); // offset for non-reference pictures
            put_se(&mut w, 5); // offset top to bottom
            w.put_ue(2); // two frames in the cycle
            put_se(&mut w, 1);
            put_se(&mut w, -1);
        }
        _ => {}
    }
    w.put_ue(1); // reference frames
    w.put(0, 1); // gaps
    w.put_ue(s.width_mbs - 1);
    w.put_ue(s.height_units - 1);
    w.put(s.frames_only, 1);
    if s.frames_only == 0 {
        w.put(1, 1); // adaptive frame/field
    }
    w.put(1, 1); // direct 8x8
    match s.crop {
        Some(crop) => {
            w.put(1, 1);
            for side in crop {
                w.put_ue(side);
            }
        }
        None => w.put(0, 1),
    }
    w.put(u64::from(s.vui), 1);
    if s.vui {
        w.put(1, 1); // aspect ratio info
        w.put(255, 8); // extended
        w.put(4, 16);
        w.put(3, 16);
        w.put(0, 6);
    }
    w.put(1, 1); // stop bit
    let mut nal = vec![0x67];
    nal.extend_from_slice(&escape(&w.finish()));
    nal
}

/// Shaped, the SPS shows `size`; shaped again the same way it reads
/// back whole (the crop written, and every field after it), and where
/// the size was the ratio exactly nothing changes.
fn shapes_to(s: Sps, shape: Shape, size: (u64, u64)) -> Vec<u8> {
    let (out, shown) = shaped_sps(&sps(s), shape).unwrap();
    assert_eq!(shown, size, "{}", shape.name());
    let (again, shown_again) = shaped_sps(&out, shape).unwrap();
    assert!(
        shown_again.0 <= size.0 && shown_again.1 <= size.1,
        "shaped twice it grew: {shown_again:?}"
    );
    if shown_again == size {
        assert_eq!(again, out, "shaping a shaped SPS changed it");
    }
    out
}

/// Every chroma layout and both kinds of picture order count give the
/// size they should, the crop in that layout's own units.
#[test]
fn every_chroma_layout_and_order_count_is_shaped_in_its_own_units() {
    // Monochrome: crop units of one pixel. 320 by 160 made 9:16 is 90 wide.
    let mono = Sps {
        profile: 100,
        chroma: Some((0, 0)),
        ..PLAIN
    };
    shapes_to(mono, Shape::Vertical, (90, 160));
    // 4:2:2, interlaced (each height unit two macroblock rows): 160 by
    // 128, square by cutting its sides in units of two pixels.
    let field = Sps {
        profile: 122,
        chroma: Some((2, 0)),
        frames_only: 0,
        width_mbs: 10,
        height_units: 4,
        vui: true,
        ..PLAIN
    };
    shapes_to(field, Shape::Square, (128, 128));
    // 4:4:4 coded as separate planes reads as monochrome: 256 by 144
    // made 9:16 is 81 wide, an odd number allowed in one-pixel units.
    let planes = Sps {
        profile: 244,
        chroma: Some((3, 1)),
        width_mbs: 16,
        height_units: 9,
        ..PLAIN
    };
    shapes_to(planes, Shape::Vertical, (81, 144));
    // 4:4:4 with its twelve scaling lists, some given, some defaulted.
    let scaled = Sps {
        profile: 244,
        chroma: Some((3, 0)),
        scaling: true,
        width_mbs: 16,
        height_units: 9,
        ..PLAIN
    };
    shapes_to(scaled, Shape::Square, (144, 144));
    // 4:2:0 High with eight scaling lists, and the order count of
    // type 1 (its cycle of offsets) and of type 2 (nothing more).
    let high = Sps {
        profile: 100,
        chroma: Some((1, 0)),
        scaling: true,
        poc: 1,
        vui: true,
        ..PLAIN
    };
    shapes_to(high, Shape::Vertical, (90, 160));
    shapes_to(Sps { poc: 2, ..PLAIN }, Shape::Square, (160, 160));
    // Interlaced 4:2:0: a height unit of four rows. 320 by 320 is made
    // 9:16 by cutting its sides: 180 wide.
    shapes_to(
        Sps {
            frames_only: 0,
            height_units: 10,
            ..PLAIN
        },
        Shape::Vertical,
        (180, 320),
    );
    // A picture already taller than 9:16 loses rows, in whole units.
    shapes_to(
        Sps {
            width_mbs: 4,
            height_units: 20,
            ..PLAIN
        },
        Shape::Vertical,
        (64, 112),
    );
}

/// The crop already in the SPS is kept and added to.
#[test]
fn a_crop_already_there_is_counted_before_the_new_one() {
    // 320 by 160 cropped to 300 by 150 (units of two pixels).
    let cropped = Sps {
        crop: Some([5, 5, 0, 5]),
        ..PLAIN
    };
    shapes_to(cropped, Shape::Square, (150, 150));
    let none_left = Sps {
        width_mbs: 1,
        height_units: 1,
        crop: Some([4, 4, 0, 0]),
        ..PLAIN
    };
    assert_eq!(
        shaped_sps(&sps(none_left), Shape::Square).unwrap_err(),
        "the video's picture has no size"
    );
}

/// An SPS that is empty, cut short anywhere, or with an Exp-Golomb
/// number longer than any is refused, never read past its end.
#[test]
fn an_sps_cut_short_is_refused_wherever_it_ends() {
    assert_eq!(shaped_sps(&[], Shape::Square).unwrap_err(), "an empty SPS");
    for s in [
        PLAIN,
        Sps {
            profile: 100,
            chroma: Some((3, 0)),
            scaling: true,
            poc: 1,
            frames_only: 0,
            crop: Some([1, 1, 1, 1]),
            ..PLAIN
        },
    ] {
        // With no VUI after the crop, the last two bytes hold the crop's
        // end: cut anywhere before them, the SPS is cut short.
        let whole = sps(s);
        for len in 1..whole.len() - 2 {
            let said = shaped_sps(&whole[..len], Shape::Vertical).unwrap_err();
            assert_eq!(said, "the SPS is cut short", "cut to {len} bytes");
        }
    }
    // Forty zero bits where a number should begin.
    let mut long = vec![0x67, 77, 0, 30];
    long.extend_from_slice(&[0; 6]);
    assert_eq!(
        shaped_sps(&long, Shape::Vertical).unwrap_err(),
        "the SPS is cut short"
    );
}

/// An `avcC` with this SPS, and one picture parameter set.
fn avcc(sps_nal: &[u8]) -> Vec<u8> {
    let mut out = vec![1, 77, 0, 30, 0xff, 0xe1];
    out.extend_from_slice(&(sps_nal.len() as u16).to_be_bytes());
    out.extend_from_slice(sps_nal);
    out.extend_from_slice(&[1, 0, 4, 0x68, 0xee, 0x3c, 0x80]);
    out
}

/// A visual sample entry of `kind`, `width` by `height`, with `children`.
fn entry(kind: &[u8; 4], width: u16, height: u16, children: &[u8]) -> Vec<u8> {
    let mut fields = vec![0u8; 78];
    fields[6..8].copy_from_slice(&1u16.to_be_bytes()); // data reference
    fields[24..26].copy_from_slice(&width.to_be_bytes());
    fields[26..28].copy_from_slice(&height.to_be_bytes());
    fields.extend_from_slice(children);
    boxed(kind, &fields)
}

/// A whole `stsd` box holding `entries`.
fn stsd(entries: &[&[u8]]) -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0];
    body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for e in entries {
        body.extend_from_slice(e);
    }
    boxed(b"stsd", &body)
}

/// The width and height an `stsd`'s first entry states.
fn stated(stsd: &[u8]) -> (u16, u16) {
    (
        u16::from_be_bytes([stsd[48], stsd[49]]),
        u16::from_be_bytes([stsd[50], stsd[51]]),
    )
}

/// The sample description is rewritten whole: its stated size, its
/// `avcC`'s SPS, the boxes beside it and any later entry as they were;
/// the SPS's picture parameter set after it, untouched.
#[test]
fn a_sample_description_is_shaped_and_keeps_everything_else() {
    let pasp = boxed(b"pasp", &[0, 0, 0, 1, 0, 0, 0, 1]);
    let first_sps = sps(PLAIN);
    let children = [pasp.clone(), boxed(b"avcC", &avcc(&first_sps))].concat();
    let first = entry(b"avc3", 320, 160, &children);
    let second = entry(b"avc1", 1, 1, &[]);
    let before = stsd(&[&first, &second]);
    let (after, size) = shaped_stsd(&before, Shape::Vertical).unwrap();
    assert_eq!(size, (90, 160));
    assert_eq!(stated(&after), (90, 160));
    assert_eq!(u32_at(&after, 0).unwrap() as usize, after.len());
    assert_eq!(&after[4..16], &before[4..16], "version and entry count");
    assert_eq!(&after[20..24], b"avc3", "the entry's kind");
    assert!(after.ends_with(&second), "the second entry was lost");
    let at = after.windows(4).position(|w| w == b"pasp").unwrap();
    assert_eq!(&after[at - 4..at - 4 + pasp.len()], &pasp[..]);
    let (shaped_sps_nal, _) = shaped_sps(&first_sps, Shape::Vertical).unwrap();
    let expected_avcc = avcc(&shaped_sps_nal);
    assert!(
        after
            .windows(expected_avcc.len())
            .any(|w| w == expected_avcc),
        "the avcC is not the shaped SPS and the PPS as it was"
    );
}

/// Each way a sample description can be broken is refused in words.
#[test]
fn a_broken_sample_description_is_refused_in_words() {
    let good_avcc = boxed(b"avcC", &avcc(&sps(PLAIN)));
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (vec![0; 10], "the sample description is cut short"),
        (
            {
                let mut s = stsd(&[&entry(b"avc1", 1, 1, &good_avcc)]);
                s.truncate(s.len() - 1);
                s
            },
            "the sample description is cut short",
        ),
        (
            stsd(&[&boxed(b"avc1", &[0; 20])]),
            "the video's description is cut short",
        ),
        (
            stsd(&[&[0, 0, 0, 6, b'a', b'v']]),
            "the sample description is cut short",
        ),
        (
            stsd(&[&entry(b"avc1", 1, 1, &[0, 0, 0, 4, 0, 0, 0, 0])]),
            "a box is cut short",
        ),
        (
            stsd(&[&entry(
                b"avc1",
                1,
                1,
                &[0, 0, 0, 40, b'f', b'r', b'e', b'e'],
            )]),
            "a box is cut short",
        ),
        (
            stsd(&[&entry(b"avc1", 1, 1, &boxed(b"pasp", &[0; 8]))]),
            "the video has no H.264 settings (avcC)",
        ),
    ];
    for (bytes, said) in cases {
        assert_eq!(shaped_stsd(&bytes, Shape::Square).unwrap_err(), said);
    }
    let hevc = stsd(&[&entry(b"hvc1", 1, 1, &[])]);
    assert_eq!(
        shaped_stsd(&hevc, Shape::Square).unwrap_err(),
        "only an H.264 video can be made square; this one is hvc1"
    );
}

/// An `avcC` cut short, or with no SPS, is refused in words.
#[test]
fn a_broken_avcc_is_refused_in_words() {
    let whole = avcc(&sps(PLAIN));
    assert_eq!(
        shaped_avcc(&whole[..4], Shape::Square).unwrap_err(),
        "the H.264 settings are cut short"
    );
    assert_eq!(
        shaped_avcc(&whole[..5], Shape::Square).unwrap_err(),
        "the H.264 settings are cut short"
    );
    assert_eq!(
        shaped_avcc(&whole[..7], Shape::Square).unwrap_err(),
        "an SPS is cut short"
    );
    assert_eq!(
        shaped_avcc(&whole[..12], Shape::Square).unwrap_err(),
        "an SPS is cut short"
    );
    let mut none = whole.clone();
    none[5] = 0xe0;
    assert_eq!(
        shaped_avcc(&none, Shape::Square).unwrap_err(),
        "the video has no SPS"
    );
    // Two SPSs: both shaped, the size the first shows.
    let mut two = vec![1, 77, 0, 30, 0xff, 0xe2];
    for s in [
        PLAIN,
        Sps {
            width_mbs: 40,
            ..PLAIN
        },
    ] {
        let nal = sps(s);
        two.extend_from_slice(&(nal.len() as u16).to_be_bytes());
        two.extend_from_slice(&nal);
    }
    let (_, size) = shaped_avcc(&two, Shape::Square).unwrap();
    assert_eq!(size, (160, 160));
}

/// `apply` shapes a picture's description and its header's size, and
/// leaves a sound alone and a short header as it is.
#[test]
fn apply_shapes_a_picture_and_leaves_a_sound_alone() {
    let hdlr = |kind: &[u8; 4]| {
        let mut h = vec![0u8; 32];
        h[16..20].copy_from_slice(kind);
        h
    };
    let description = stsd(&[&entry(
        b"avc1",
        320,
        160,
        &boxed(b"avcC", &avcc(&sps(PLAIN))),
    )]);
    let mut picture = Track {
        hdlr: hdlr(b"vide"),
        stsd: description.clone(),
        tkhd_tail: vec![7; 60],
        ..Track::default()
    };
    apply(&mut picture, Shape::Vertical).unwrap();
    assert_eq!(stated(&picture.stsd), (90, 160));
    assert_eq!(&picture.tkhd_tail[..52], &[7; 52][..]);
    assert_eq!(u32_at(&picture.tkhd_tail, 52), Some(90 << 16));
    assert_eq!(u32_at(&picture.tkhd_tail, 56), Some(160 << 16));

    let mut short = Track {
        hdlr: hdlr(b"vide"),
        stsd: description.clone(),
        tkhd_tail: vec![7; 20],
        ..Track::default()
    };
    apply(&mut short, Shape::Square).unwrap();
    assert_eq!(stated(&short.stsd), (160, 160));
    assert_eq!(short.tkhd_tail, vec![7; 20]);

    let mut sound = Track {
        hdlr: hdlr(b"soun"),
        stsd: vec![1, 2, 3],
        tkhd_tail: vec![7; 60],
        ..Track::default()
    };
    apply(&mut sound, Shape::Vertical).unwrap();
    assert_eq!(sound.stsd, [1, 2, 3]);
    assert_eq!(sound.tkhd_tail, vec![7; 60]);
}

/// A real H.264 picture joined with its sound, made vertical and
/// square: the picture states its new size in its description and its
/// header, the sound is the same as in a plain join.
#[test]
fn a_joined_video_is_shaped_and_its_sound_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let inputs = [media("picture.mp4"), media("sound.m4a")];
    let plain = dir.path().join("plain.mp4");
    crate::mp4mux::join(&inputs, &plain).unwrap();
    let plain = crate::mp4mux::tracks_in_file(&plain).unwrap();
    let sound_of = |tracks: &[Track]| {
        tracks
            .iter()
            .find(|t| t.hdlr.get(16..20) == Some(b"soun"))
            .map(|t| (t.stsd.clone(), t.tkhd_tail.clone()))
            .unwrap()
    };
    assert_eq!(
        stated(
            &plain
                .iter()
                .find(|t| t.hdlr[16..20] == *b"vide")
                .unwrap()
                .stsd
        ),
        (160, 90)
    );
    for (shape, size) in [(Shape::Vertical, (50, 90)), (Shape::Square, (90, 90))] {
        let out = dir.path().join(format!("{}.mp4", shape.name()));
        crate::mp4mux::join_cut(&inputs, &[], &out, None, Some(shape)).unwrap();
        let tracks = crate::mp4mux::tracks_in_file(&out).unwrap();
        let video = tracks
            .iter()
            .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
            .unwrap();
        assert_eq!(stated(&video.stsd), size, "{}", shape.name());
        let tail = &video.tkhd_tail;
        assert_eq!(u32_at(tail, 52), Some(u32::from(size.0) << 16));
        assert_eq!(u32_at(tail, 56), Some(u32::from(size.1) << 16));
        assert_eq!(sound_of(&tracks), sound_of(&plain), "the sound changed");
    }
}

/// VP9 and AV1 pictures are refused in words, naming the shape and
/// the kind of picture.
#[test]
fn a_vp9_or_av1_picture_is_refused_in_words() {
    let dir = tempfile::tempdir().unwrap();
    for (name, kind) in [("vp9.f313.webm", "vp09"), ("av1.f399.webm", "av01")] {
        let out = dir.path().join("out.mp4");
        let said = crate::mp4mux::join_cut(&[media(name)], &[], &out, None, Some(Shape::Vertical))
            .unwrap_err();
        assert_eq!(
            said,
            format!("only an H.264 video can be made vertical; this one is {kind}")
        );
        assert!(!out.exists(), "a file was written for a refused shape");
    }
}

/// A shape is stored as its lowercase word and read back from it.
#[test]
fn a_shape_is_stored_as_its_word() {
    assert_eq!(
        serde_json::to_string(&Shape::Vertical).unwrap(),
        "\"vertical\""
    );
    assert_eq!(
        serde_json::from_str::<Shape>("\"square\"").unwrap(),
        Shape::Square
    );
    assert_eq!(Shape::named(" Vertical "), Some(Shape::Vertical));
    assert_eq!(Shape::named("SQUARE"), Some(Shape::Square));
}

/// Escaping puts a 3 before every 0, 1, 2 or 3 that follows two zeros,
/// and unescaping takes exactly those out again.
#[test]
fn escaping_guards_every_byte_a_start_code_could_begin_with() {
    for last in 0..=3u8 {
        let raw = [0, 0, last, 9];
        let escaped = escape(&raw);
        assert_eq!(escaped, [0, 0, 3, last, 9], "{last}");
        assert_eq!(unescape(&escaped), raw);
    }
    assert_eq!(escape(&[0, 0, 4]), [0, 0, 4]);
    // No stop bit to be found: nothing is left before it.
    let zeros = [0u8; 2];
    let bits = Bits::new(&zeros);
    assert_eq!(bits.remaining_before_stop(), 0);
}
