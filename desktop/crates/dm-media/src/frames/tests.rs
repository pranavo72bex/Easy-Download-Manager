// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The mean red and blue of an RGB picture.
fn red_blue(rgb: &[u8]) -> (u32, u32) {
    let n = (rgb.len() / 3).max(1) as u32;
    let (r, b) = rgb.chunks(3).fold((0u32, 0u32), |(r, b), px| {
        (r + u32::from(px[0]), b + u32::from(px[2]))
    });
    (r / n, b / n)
}

/// An AV1 MP4 (one second red, one blue, made by an AV1 encoder) read by
/// dm's AV1 reader, the Linux route to a GIF (gaps row 54): twelve
/// pictures a second, its size kept, red then blue.
#[test]
fn an_av1_video_is_read_to_pictures_red_then_blue() {
    let fixture = crate::fixtures().join("mp4/red-blue.av1.mp4");
    let (w, h, frames) = av1::read(&fixture, 12, 480).unwrap();
    assert_eq!((w, h), (320, 180));
    assert!(
        (22..=25).contains(&frames.len()),
        "{} pictures",
        frames.len()
    );
    for frame in &frames {
        assert_eq!(frame.rgb.len(), 320 * 180 * 3);
    }
    let (r, b) = red_blue(&frames[1].rgb);
    assert!(
        r > 150 && b < 90,
        "the first second is not red: r {r}, b {b}"
    );
    let (r, b) = red_blue(&frames[frames.len() - 2].rgb);
    assert!(
        b > 150 && r < 90,
        "the second second is not blue: r {r}, b {b}"
    );
    // Smaller where asked: at most 160 across, its shape kept.
    let (w, h, _) = av1::read(&fixture, 12, 160).unwrap();
    assert_eq!((w, h), (160, 90));
    // And written as a GIF.
    let gif = crate::gif::encode(320, 180, &frames).unwrap();
    assert_eq!(&gif[..6], b"GIF89a");
}

/// A part cut by dm's own cutter (an edit list over the samples) gives
/// the pictures of its span alone: from half a second, a second long, so
/// red for its first half and blue after.
#[test]
fn a_cut_av1_part_gives_the_pictures_of_its_span_only() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = crate::fixtures().join("mp4/red-blue.av1.mp4");
    let part = dir.path().join("part.mp4");
    crate::mp4mux::join_cut(&[fixture], &[], &part, Some((500, 1500)), None).unwrap();
    let (_, _, frames) = av1::read(&part, 12, 480).unwrap();
    assert!(
        (10..=13).contains(&frames.len()),
        "{} pictures",
        frames.len()
    );
    let (r, _) = red_blue(&frames[0].rgb);
    assert!(r > 150, "the part does not start red: r {r}");
    let (_, b) = red_blue(&frames[frames.len() - 1].rgb);
    assert!(b > 150, "the part does not end blue: b {b}");
}

/// A picture that is not AV1 is refused in words, not decoded as noise.
#[test]
fn a_part_that_is_not_av1_is_refused_in_words() {
    let h264 = crate::fixtures().join("mp4/picture.f137.mp4");
    let Err(said) = av1::read(&h264, 12, 480) else {
        panic!("an H.264 part was decoded as AV1");
    };
    assert!(said.contains("AV1"), "{said}");
    assert!(av1::read(Path::new("/nowhere/at/all.mp4"), 12, 480).is_err());
}

#[test]
fn a_picture_shrinks_by_averaging_and_keeps_its_crop() {
    // 4x2 BGRA: left half red, right half blue; stride with padding.
    let stride = 4 * 4 + 8;
    let mut bgra = vec![0u8; stride * 2];
    for y in 0..2 {
        for x in 0..4 {
            let at = y * stride + x * 4;
            let px = if x < 2 {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            };
            bgra[at..at + 4].copy_from_slice(&px);
        }
    }
    assert_eq!(
        shrink(&bgra, stride, (0, 0, 4, 2), 2, 1),
        [255, 0, 0, 0, 0, 255]
    );
    // Only the right half (a crop): all blue.
    assert_eq!(shrink(&bgra, stride, (2, 0, 2, 2), 1, 1), [0, 0, 255]);
}

/// The picture fixture (a real H.264 track) made into an MP4 the way a
/// download is, read by Media Foundation and written as a GIF: the
/// pictures have the size the GIF says, the same read gives the same
/// GIF, and the file has its header and its end.
#[cfg(windows)]
#[test]
fn a_picture_is_read_by_media_foundation_and_made_a_gif() {
    if !crate::gif::possible() {
        eprintln!("no H.264 decoder on this Windows (an N edition): nothing to check");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let fixtures = crate::fixtures().join("mp4");
    let mp4 = dir.path().join("part.mp4");
    crate::mp4mux::join(&[fixtures.join("picture.f137.mp4")], &mp4).unwrap();
    let (w, h, frames) = read(&mp4, 12, 480).unwrap();
    assert!(w > 0 && h > 0 && w <= 480, "{w} x {h}");
    assert!(!frames.is_empty(), "no picture was decoded");
    for frame in &frames {
        assert_eq!(frame.rgb.len(), usize::from(w) * usize::from(h) * 3);
        assert!(frame.delay >= 2);
    }
    let bytes = crate::gif::from_video_bytes(&mp4).unwrap();
    assert_eq!(&bytes[..6], b"GIF89a");
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), w);
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), h);
    assert_eq!(bytes.last(), Some(&0x3B), "no GIF trailer");
    assert_eq!(
        bytes,
        crate::gif::encode(w, h, &frames).unwrap(),
        "reading the same part twice gave different pictures"
    );
    // The picture's shape is the stream's: its size from the sample
    // description (`avc1`: width and height after twelve reserved
    // bytes), shrunk to at most 480 across.
    let tracks = crate::mp4mux::tracks_in_file(&mp4).unwrap();
    let video = tracks
        .iter()
        .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
        .unwrap();
    let (sw, sh) = (
        u32::from(u16::from_be_bytes([video.stsd[48], video.stsd[49]])),
        u32::from(u16::from_be_bytes([video.stsd[50], video.stsd[51]])),
    );
    let (gw, gh) = (u32::from(w), u32::from(h));
    assert!(
        (gw * sh).abs_diff(gh * sw) <= sw.max(sh) * 2,
        "{gw} x {gh} for a stream of {sw} x {sh}"
    );
}

/// A part cut from the picture (the edit list `join_cut` writes) gives
/// the pictures of its span only, whether Media Foundation plays the
/// edit list itself or gives the media's own times.
#[cfg(windows)]
#[test]
fn a_cut_part_gives_the_pictures_of_its_span_only() {
    if !crate::gif::possible() {
        eprintln!("no H.264 decoder on this Windows (an N edition): nothing to check");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let picture = crate::fixtures().join("mp4/picture.f137.mp4");
    let whole = dir.path().join("whole.mp4");
    crate::mp4mux::join(std::slice::from_ref(&picture), &whole).unwrap();
    let all = read(&whole, 12, 480).unwrap().2.len();
    let part = dir.path().join("part.mp4");
    crate::mp4mux::join_cut(&[picture], &[], &part, Some((400, 1400)), None).unwrap();
    let tracks = crate::mp4mux::tracks_in_file(&part).unwrap();
    let video = tracks
        .iter()
        .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
        .unwrap();
    // The pictures the edit list plays, one per tick of the GIF, counted
    // from the samples' own times as the reader's contract says.
    let scale = f64::from(video.timescale);
    let edit = video.edits.iter().find(|e| e.media_time >= 0).unwrap();
    let from = edit.media_time as f64 / scale;
    let to = from + edit.duration as f64 / f64::from(video.edits_timescale);
    let mut decode_time = video.start.unwrap_or(0) as i64;
    let mut ticks = std::collections::BTreeSet::new();
    for sample in &video.samples {
        let at = (decode_time + i64::from(sample.cts)) as f64 / scale;
        decode_time += i64::from(sample.duration);
        if at >= from - 1e-6 && at < to {
            ticks.insert(((at - from) * 12.0).floor() as u64);
        }
    }
    let some = read(&part, 12, 480).unwrap().2.len();
    assert!(some > 0 && some < all, "{some} of {all}");
    assert_eq!(some, ticks.len(), "the span {from:.2} to {to:.2} s");
}

/// A file with no picture is refused in words, a sound alone, text, or
/// a name the reader does not know the kind of; nothing panics.
#[cfg(windows)]
#[test]
fn a_file_with_no_picture_is_refused_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let sound = crate::fixtures().join("mp4/sound.f140.m4a");
    assert!(
        read(&sound, 12, 480).is_err(),
        "a sound alone has no picture"
    );
    for name in ["not-video.mp4", "not-video.part"] {
        let source = dir.path().join(name);
        std::fs::write(&source, b"this is text, not a video").unwrap();
        let said = read(&source, 12, 480).err().expect("text is not a video");
        assert!(!said.is_empty(), "{name}");
    }
    assert!(read(&dir.path().join("missing.mp4"), 12, 480).is_err());
}
