// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The remux from a stream's segments where they lie (gaps row Y21): the MP4 it makes is the one
//! made through scratch files before, and with the segments freed as it goes, the disk holds
//! little more than the stream at any time.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;

/// `data` on `pid`, 188 bytes a packet, the first starting a unit, the last filled out by its
/// adaptation field.
fn packets(out: &mut Vec<u8>, pid: u16, data: &[u8], counter: &mut u8) {
    for (n, piece) in data.chunks(184).enumerate() {
        let mut packet = vec![0x47, (u8::from(n == 0) << 6) | (pid >> 8) as u8, pid as u8];
        let stuffing = 184 - piece.len();
        if stuffing == 0 {
            packet.push(0x10 | *counter);
        } else {
            packet.push(0x30 | *counter);
            packet.push((stuffing - 1) as u8);
            if stuffing > 1 {
                packet.push(0);
                packet.resize(packet.len() + stuffing - 2, 0xff);
            }
        }
        packet.extend_from_slice(piece);
        out.extend(packet);
        *counter = (*counter + 1) & 15;
    }
}

/// A PES packet stamped `pts` (90 kHz).
fn pes(stream_id: u8, pts: u64, body: &[u8]) -> Vec<u8> {
    let mut pes = vec![0, 0, 1, stream_id, 0, 0, 0x80, 0x80, 5];
    pes.extend_from_slice(&[
        0x21 | ((pts >> 29) & 0x0e) as u8,
        (pts >> 22) as u8,
        0x01 | ((pts >> 14) & 0xfe) as u8,
        (pts >> 7) as u8,
        0x01 | ((pts << 1) & 0xfe) as u8,
    ]);
    pes.extend_from_slice(body);
    pes
}

/// Picture `n`, a key frame a second with its parameter sets: about `size` bytes.
fn picture(n: u32, size: usize) -> Vec<u8> {
    let key = n.is_multiple_of(25);
    let mut body = vec![0, 0, 0, 1, 9, 0xf0];
    if key {
        body.extend_from_slice(&[0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x1e, 0x95, 0xa0, 0x50, 0x5f]);
        body.extend_from_slice(&[0, 0, 0, 1, 0x68, 0xce, 0x3c, 0x80]);
    }
    body.extend_from_slice(&[0, 0, 0, 1, if key { 0x65 } else { 0x41 }]);
    body.extend((0..size).map(|i| ((i + n as usize) % 250 + 1) as u8));
    body
}

/// An ADTS frame of `len` bytes: AAC-LC, 44.1 kHz, stereo.
fn sound_frame(len: usize) -> Vec<u8> {
    let mut frame = vec![
        0xff,
        0xf1,
        0x50,
        0x80 | ((len >> 11) & 3) as u8,
        (len >> 3) as u8,
        (((len & 7) << 5) as u8) | 0x1f,
        0xfc,
    ];
    frame.resize(len, 0x11);
    frame
}

/// A stream of `seconds` seconds in a segment a second, as HLS serves one, each led by its
/// tables: 25 H.264 pictures a second of about `size` bytes, and AAC two frames to a PES, muxed
/// in time order. Written into `dir`, as EDM names a stream's segments.
fn segments(dir: &Path, seconds: u32, size: usize) -> Vec<PathBuf> {
    let pat = [0, 0, 0xb0, 13, 0, 1, 0xc1, 0, 0, 0, 1, 0xf0, 0, 0, 0, 0, 0];
    let pmt = [
        0, 2, 0xb0, 23, 0, 1, 0xc1, 0, 0, 0xe1, 0, 0xf0, 0, 0x1b, 0xe1, 0, 0xf0, 0, 0x0f, 0xe1, 1,
        0xf0, 0, 0, 0, 0, 0,
    ];
    let mut counters = [0u8; 4];
    let mut heard = 0u64;
    let mut paths = Vec::new();
    for second in 0..seconds {
        let mut segment = Vec::new();
        packets(&mut segment, 0, &pat, &mut counters[0]);
        packets(&mut segment, 0x1000, &pmt, &mut counters[1]);
        for n in second * 25..(second + 1) * 25 {
            let at = 90_000 * u64::from(n) / 25;
            while heard * 90_000 / 44_100 <= at {
                let frames: Vec<u8> = (0..2).flat_map(|_| sound_frame(371)).collect();
                let unit = pes(0xc0, heard * 90_000 / 44_100 + 9_000, &frames);
                packets(&mut segment, 0x101, &unit, &mut counters[3]);
                heard += 2048;
            }
            let unit = pes(0xe0, at + 9_000, &picture(n, size));
            packets(&mut segment, 0x100, &unit, &mut counters[2]);
        }
        let path = dir.join(format!("part-{second:04}"));
        std::fs::write(&path, segment).unwrap();
        paths.push(path);
    }
    paths
}

/// The bytes of those of `paths` still on the disk.
fn on_disk(paths: &[PathBuf]) -> u64 {
    paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

#[test]
fn the_mp4_made_from_the_segments_is_the_one_made_through_scratch_files() {
    let dir = tempfile::tempdir().unwrap();
    let parts = segments(dir.path(), 16, 12_000);
    let whole = dir.path().join("whole.ts");
    let joined: Vec<u8> = parts
        .iter()
        .flat_map(|p| std::fs::read(p).unwrap())
        .collect();
    std::fs::write(&whole, joined).unwrap();
    // As it was made before: the stream gathered, each track's samples in a scratch file.
    let before = dir.path().join("before.mp4");
    let scratch = |what: &str| dir.path().join(format!("scratch.{what}"));
    let mut streams = read_ts(&whole, &scratch).unwrap();
    build(&mut streams, &before, |_| Vec::new()).unwrap();
    let before = std::fs::read(&before).unwrap();
    assert_eq!(before.windows(4).filter(|w| *w == b"trak").count(), 2);

    let after = dir.path().join("after.mp4");
    to_mp4_from(&parts, &after, Segments::Kept).unwrap();
    assert!(
        std::fs::read(&after).unwrap() == before,
        "the same MP4 from the segments"
    );
    assert!(
        parts.iter().all(|p| p.exists()),
        "segments kept are left as they are"
    );
    let single = dir.path().join("single.mp4");
    to_mp4(&whole, &single).unwrap();
    assert!(
        std::fs::read(&single).unwrap() == before,
        "the same MP4 from one file"
    );
}

#[test]
fn segments_freed_as_they_are_read_keep_the_disk_near_the_stream() {
    let dir = tempfile::tempdir().unwrap();
    let parts = segments(dir.path(), 16, 12_000);
    let stream = on_disk(&parts);
    let seen: Rc<RefCell<Vec<(u64, u64)>>> = Rc::default();
    let (watched, told) = (parts.clone(), Rc::clone(&seen));
    let watch = move |copied: u64| told.borrow_mut().push((on_disk(&watched), copied));
    let locks = crate::sealed::Locks::default();
    let reading = Reading {
        inputs: &parts,
        locks: &locks,
        freed: true,
        watch: Some(Box::new(watch)),
    };
    let length = remux(reading, &dir.path().join("joined.mp4"), |_| Vec::new()).unwrap();
    assert!(parts.iter().all(|p| !p.exists()), "every segment is freed");

    // Told before each segment was freed, and once the MP4 was whole: what was left of the
    // stream, and the samples' bytes handed to the writer, counted as on the disk already.
    let seen = seen.borrow();
    assert_eq!(seen.len(), parts.len() + 1);
    let (left, samples) = *seen.last().unwrap();
    assert_eq!(left, 0);
    let index = length - samples;
    let peak = seen
        .iter()
        .map(|(left, copied)| left + index + copied)
        .max()
        .unwrap();
    let larger = stream.max(length);
    assert!(
        peak * 10 <= larger * 13,
        "the disk held {peak} bytes at most, over 1.3 times {larger} (stream {stream}, MP4 \
         {length})"
    );
    // Before: the gathered stream, a scratch file of its samples and the MP4, all at once.
    let gathered = stream + samples + length;
    assert!(peak * 2 < gathered, "{peak} against {gathered} before");
}

/// A stream whose clock starts afresh part-way, as after an HLS discontinuity (the same two
/// seconds served twice): the picture's second stretch is moved on to follow its first, a frame
/// at 30 a second after its last picture (2.06 s, from its 0.1 s), and the subtitles are told
/// how far (gaps row Y20).
#[test]
fn the_subtitles_are_told_how_far_each_stretch_after_a_discontinuity_was_moved() {
    let dir = tempfile::tempdir().unwrap();
    let (first, again) = (dir.path().join("first"), dir.path().join("again"));
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&again).unwrap();
    let mut parts = segments(&first, 2, 2_000);
    parts.extend(segments(&again, 2, 2_000));
    let mut told = None;
    let place = |clock: Clock| {
        told = Some(clock);
        Vec::new()
    };
    to_mp4_from_with_text(&parts, &dir.path().join("out.mp4"), Segments::Kept, place).unwrap();
    let last = 90_000 * 49 / 25 + 9_000;
    let expected = Clock {
        zero: 9_000,
        moved: vec![0, last + STEP - 9_000],
    };
    assert_eq!(told, Some(expected));
}
