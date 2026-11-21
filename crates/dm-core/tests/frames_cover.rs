// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The pictures of an H.264 MP4 read by the system's decoder (`frames`), for
//! a part saved as GIF: their size, how many there are and how long each
//! shows, the part an edit list plays, the shape `shape` gives the picture,
//! and every way a file that cannot be read is refused in words.
//!
//! The decoder is VideoToolbox on macOS; on Linux dm reads an AV1 stream
//! instead (its own tests are in `frames/av1.rs`), so the H.264 checks here
//! run on macOS only.

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

// For the H.264 checks, which run on macOS only.
#[cfg(target_os = "macos")]
fn media(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media/mp4mux")
        .join(name)
}

#[cfg(target_os = "macos")]
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mp4")
        .join(name)
}

/// A file that is not there is an error on every system, not a panic.
#[test]
fn a_file_that_is_not_there_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let said = dm_core::frames::read(&dir.path().join("missing.mp4"), 12, 480)
        .err()
        .expect("a missing file gave pictures");
    assert!(!said.is_empty());
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use dm_core::gif::Frame;

    /// Where the four letters `kind` first appear in `bytes`: a box's kind,
    /// so its size is the four bytes before.
    fn find(bytes: &[u8], kind: &[u8; 4]) -> usize {
        bytes
            .windows(4)
            .position(|w| w == kind)
            .unwrap_or_else(|| panic!("no {}", String::from_utf8_lossy(kind)))
    }

    /// The 160 by 90, two-second, 30-a-second picture made a plain MP4 the
    /// way a download is joined.
    fn joined(dir: &Path) -> PathBuf {
        let out = dir.join("whole.mp4");
        dm_core::mp4mux::join(&[media("picture.mp4")], &out).unwrap();
        out
    }

    /// The joined picture with its edit list's one entry set to `duration`
    /// (in the movie's timescale) and `media_time` (in the track's).
    fn with_edit(dir: &Path, duration: u64, media_time: i64) -> PathBuf {
        let mut bytes = std::fs::read(joined(dir)).unwrap();
        let at = find(&bytes, b"elst");
        let count = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap());
        assert_eq!(count, 1, "the joined picture has one edit");
        if bytes[at + 4] == 1 {
            bytes[at + 12..at + 20].copy_from_slice(&duration.to_be_bytes());
            bytes[at + 20..at + 28].copy_from_slice(&media_time.to_be_bytes());
        } else {
            bytes[at + 12..at + 16].copy_from_slice(&(duration as u32).to_be_bytes());
            bytes[at + 16..at + 20].copy_from_slice(&(media_time as i32).to_be_bytes());
        }
        let out = dir.join("edited.mp4");
        std::fs::write(&out, bytes).unwrap();
        out
    }

    fn check_pictures(w: u16, h: u16, frames: &[Frame]) {
        for frame in frames {
            assert_eq!(frame.rgb.len(), usize::from(w) * usize::from(h) * 3);
            assert!(frame.delay >= 2, "a delay of {}", frame.delay);
        }
    }

    /// The whole picture, twelve a second: its own size (it is narrower
    /// than 480), 24 pictures for two seconds, each shown 8 or 9 hundredths
    /// so the whole lasts two seconds; and the same pictures each time.
    #[test]
    fn a_whole_picture_is_read_twelve_a_second_at_its_own_size() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = joined(dir.path());
        let (w, h, frames) = dm_core::frames::read(&mp4, 12, 480).unwrap();
        assert_eq!((w, h), (160, 90));
        assert_eq!(frames.len(), 24);
        check_pictures(w, h, &frames);
        let delays: Vec<u16> = frames.iter().map(|f| f.delay).collect();
        assert!(delays.iter().all(|d| (8..=9).contains(d)), "{delays:?}");
        assert_eq!(delays.iter().map(|d| u32::from(*d)).sum::<u32>(), 200);
        let pixels: Vec<u64> = frames
            .iter()
            .map(|f| f.rgb.iter().map(|b| u64::from(*b)).sum())
            .collect();
        assert!(pixels.iter().any(|p| *p > 0), "every picture is black");
        let again = dm_core::frames::read(&mp4, 12, 480).unwrap().2;
        assert!(
            frames.iter().zip(&again).all(|(a, b)| a.rgb == b.rgb),
            "the same file read twice gave different pictures"
        );
    }

    /// Smaller where asked, its shape kept and both sides even; a width
    /// under 16 is read as 16 and no pictures a second as one.
    #[test]
    fn a_picture_is_made_smaller_and_the_rate_held_to_one_at_least() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = joined(dir.path());
        let (w, h, frames) = dm_core::frames::read(&mp4, 12, 64).unwrap();
        assert_eq!((w, h), (64, 36));
        check_pictures(w, h, &frames);
        let (w, h, frames) = dm_core::frames::read(&mp4, 0, 0).unwrap();
        assert_eq!((w, h), (16, 8), "a width of 0 read as 16, its shape kept");
        assert_eq!(frames.len(), 2, "one picture a second for two seconds");
        assert!(frames.iter().all(|f| f.delay == 100));
        check_pictures(w, h, &frames);
    }

    /// A fragmented stream as a site serves it (no edit list; its pieces
    /// say when they play) is read whole: its 10 pictures, shown from 0.2 s
    /// to 1.2 s, and asked at 12 a second they still last the second they
    /// play (timed by their ticks, not their place in the list).
    #[test]
    fn a_fragmented_stream_with_no_edit_list_is_read_whole() {
        let (w, h, frames) = dm_core::frames::read(&fixture("picture.f137.mp4"), 12, 480).unwrap();
        assert_eq!((w, h), (64, 36));
        assert_eq!(frames.len(), 10, "each of its 10 pictures, none repeated");
        let lasts: u32 = frames.iter().map(|f| u32::from(f.delay)).sum();
        assert!(
            (98..=102).contains(&lasts),
            "the second lasts {lasts} hundredths"
        );
        check_pictures(w, h, &frames);
    }

    /// A part cut by dm's own cutter gives the pictures of its span only:
    /// a second of the two, so 12 pictures, not 24.
    #[test]
    fn a_cut_part_gives_the_pictures_of_its_span_only() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("part.mp4");
        dm_core::mp4mux::join_cut(&[media("picture.mp4")], &[], &part, Some((500, 1500)), None)
            .unwrap();
        let (w, h, frames) = dm_core::frames::read(&part, 12, 480).unwrap();
        assert_eq!((w, h), (160, 90));
        assert!(
            (11..=13).contains(&frames.len()),
            "{} pictures",
            frames.len()
        );
        check_pictures(w, h, &frames);
    }

    /// The first half second of a three-second video (the HLS fixture's
    /// three segments made one MP4, ten pictures a second): five pictures,
    /// where the whole gives thirty (a tick of the GIF with no picture of
    /// its own is left out); the samples after the part's end and the
    /// decoder's look-ahead are not read at all.
    #[test]
    fn a_short_part_of_a_longer_video_stops_reading_after_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let segments = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/hls/h264");
        let ts = dir.path().join("all.ts");
        let mut bytes = Vec::new();
        for n in 0..3 {
            bytes.extend(std::fs::read(segments.join(format!("seg{n}.ts"))).unwrap());
        }
        std::fs::write(&ts, bytes).unwrap();
        let whole = dir.path().join("whole.mp4");
        dm_core::tsmux::to_mp4(&ts, &whole).unwrap();
        let (w, h, all) = dm_core::frames::read(&whole, 12, 480).unwrap();
        assert_eq!((w, h), (128, 72));
        assert!((29..=31).contains(&all.len()), "{} pictures", all.len());
        let part = dir.path().join("part.mp4");
        dm_core::mp4mux::join_cut(&[whole], &[], &part, Some((0, 500)), None).unwrap();
        let (_, _, frames) = dm_core::frames::read(&part, 12, 480).unwrap();
        assert!((4..=6).contains(&frames.len()), "{} pictures", frames.len());
    }

    /// The crop `shape` writes is the picture the decoder gives: made
    /// vertical, 160 by 90 shows as its middle 50 by 90; made square, 90 by
    /// 90. The GIF is the shape asked for, not the coded picture squeezed.
    #[test]
    fn a_shaped_picture_is_read_in_the_shape_it_was_given() {
        let dir = tempfile::tempdir().unwrap();
        for (shape, size) in [
            (dm_core::shape::Shape::Vertical, (50, 90)),
            (dm_core::shape::Shape::Square, (90, 90)),
        ] {
            let out = dir.path().join(format!("{}.mp4", shape.name()));
            dm_core::mp4mux::join_cut(&[media("picture.mp4")], &[], &out, None, Some(shape))
                .unwrap();
            let (w, h, frames) = dm_core::frames::read(&out, 12, 480).unwrap();
            assert_eq!((w, h), size, "{}", shape.name());
            assert_eq!(frames.len(), 24);
            check_pictures(w, h, &frames);
        }
    }

    /// An edit list of no length plays the media to its end: the whole.
    #[test]
    fn an_edit_of_no_length_plays_to_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = with_edit(dir.path(), 0, 0);
        let (_, _, frames) = dm_core::frames::read(&mp4, 12, 480).unwrap();
        // Two seconds of pictures, shown from about 0.08 s (the stream's
        // B-frame offset, which an edit at media time 0 keeps): 24 or 25
        // ticks, and the length of the media in time.
        assert!(
            (24..=25).contains(&frames.len()),
            "{} pictures",
            frames.len()
        );
        let lasts: u32 = frames.iter().map(|f| u32::from(f.delay)).sum();
        assert!(
            (200..=210).contains(&lasts),
            "the part lasts {lasts} hundredths"
        );
    }

    /// More than two minutes is refused before anything is decoded.
    #[test]
    fn a_part_longer_than_two_minutes_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = with_edit(dir.path(), u64::from(u32::MAX), 0);
        let said = dm_core::frames::read(&mp4, 12, 480).err().unwrap();
        assert!(said.contains("two minutes"), "{said}");
    }

    /// An edit that starts after the last picture leaves nothing to show,
    /// which is said, not written as an empty GIF.
    #[test]
    fn an_edit_past_every_picture_decodes_nothing_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = with_edit(dir.path(), 1000, i64::from(i32::MAX / 2));
        let said = dm_core::frames::read(&mp4, 12, 480).err().unwrap();
        assert!(said.contains("no picture"), "{said}");
    }

    /// A sound alone has no picture.
    #[test]
    fn a_sound_alone_is_refused() {
        let said = dm_core::frames::read(&media("sound.m4a"), 12, 480)
            .err()
            .unwrap();
        assert_eq!(said, "the part has no picture");
    }

    /// An AV1 picture is not decoded as H.264 on macOS: refused, naming it.
    #[test]
    fn a_picture_that_is_not_h264_is_refused_naming_its_kind() {
        let said = dm_core::frames::read(&fixture("red-blue.av1.mp4"), 12, 480)
            .err()
            .unwrap();
        assert!(said.contains("H.264") && said.contains("av01"), "{said}");
    }

    /// The picture's H.264 settings missing, cut short, or not ones the
    /// decoder takes: each refused in words.
    #[test]
    fn broken_h264_settings_are_refused_in_words() {
        let dir = tempfile::tempdir().unwrap();
        let whole = std::fs::read(joined(dir.path())).unwrap();
        let at = find(&whole, b"avcC");
        let write = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };

        let mut renamed = whole.clone();
        renamed[at..at + 4].copy_from_slice(b"avcX");
        let said = dm_core::frames::read(&write("no-avcc.mp4", &renamed), 12, 480)
            .err()
            .unwrap();
        assert!(said.contains("avcC"), "{said}");

        // The first SPS said to be longer than the box holds.
        let mut long = whole.clone();
        long[at + 10..at + 12].copy_from_slice(&0xffffu16.to_be_bytes());
        let said = dm_core::frames::read(&write("long-sps.mp4", &long), 12, 480)
            .err()
            .unwrap();
        assert_eq!(said, "cut short");

        // A box in the entry smaller than its own header ends the search.
        let mut tiny = whole.clone();
        tiny[at - 4..at].copy_from_slice(&4u32.to_be_bytes());
        let said = dm_core::frames::read(&write("tiny-box.mp4", &tiny), 12, 480)
            .err()
            .unwrap();
        assert!(said.contains("avcC"), "{said}");

        // The SPS's bytes all ones: no decoder takes them.
        let mut garbled = whole.clone();
        let len = usize::from(u16::from_be_bytes([whole[at + 10], whole[at + 11]]));
        for b in &mut garbled[at + 13..at + 12 + len] {
            *b = 0xff;
        }
        let said = dm_core::frames::read(&write("bad-sps.mp4", &garbled), 12, 480)
            .err()
            .unwrap();
        assert!(!said.is_empty());
    }

    /// A file whose samples lie past its end is refused before a byte of
    /// it is given to the decoder.
    #[test]
    fn a_file_cut_short_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = std::fs::read(joined(dir.path())).unwrap();
        bytes.truncate(bytes.len() - 1000);
        let cut = dir.path().join("cut.mp4");
        std::fs::write(&cut, bytes).unwrap();
        let said = dm_core::frames::read(&cut, 12, 480).err().unwrap();
        assert!(said.contains("cut short"), "{said}");
    }

    /// And the pictures make a GIF of the size they say.
    #[test]
    fn the_pictures_make_a_gif_of_their_size() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = joined(dir.path());
        let gif = dir.path().join("part.gif");
        let length = dm_core::gif::from_video(&mp4, &gif).unwrap();
        let bytes = std::fs::read(&gif).unwrap();
        assert_eq!(bytes.len() as u64, length);
        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 160);
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 90);
        assert_eq!(bytes.last(), Some(&0x3B));
    }
}
