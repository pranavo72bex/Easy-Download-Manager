// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Small MP4 files made for tests: this crate's, and dm-core's through the `testing`
//! feature.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

/// A minimal track: one sample description of `codec`, `samples` of
/// `size` bytes each lasting `duration`.
fn moov_of(id: u32, handler: &[u8; 4], timescale: u32, stbl: &[u8], mvex: bool) -> Vec<u8> {
    let mut tkhd = vec![0u8; 8];
    tkhd.extend_from_slice(&id.to_be_bytes());
    tkhd.extend_from_slice(&[0u8; 8]);
    tkhd.extend_from_slice(&[0u8; 60]);
    let mut mdhd = vec![0u8; 8];
    mdhd.extend_from_slice(&timescale.to_be_bytes());
    mdhd.extend_from_slice(&[0u8; 4]);
    mdhd.extend_from_slice(&0x55c4u16.to_be_bytes());
    mdhd.extend_from_slice(&[0u8; 2]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0u8; 13]);
    let minf = boxed(b"minf", &[&full(b"smhd", 0, 0, &[0u8; 4]), stbl]);
    let mdia = boxed(
        b"mdia",
        &[
            &full(b"mdhd", 0, 0, &mdhd),
            &full(b"hdlr", 0, 0, &hdlr),
            &minf,
        ],
    );
    let trak = boxed(b"trak", &[&full(b"tkhd", 0, 3, &tkhd), &mdia]);
    let mut mvhd = vec![0u8; 8];
    mvhd.extend_from_slice(&1000u32.to_be_bytes());
    mvhd.extend_from_slice(&[0u8; 84]);
    let mut parts = vec![full(b"mvhd", 0, 0, &mvhd), trak];
    if mvex {
        let mut trex = id.to_be_bytes().to_vec();
        trex.extend_from_slice(&1u32.to_be_bytes());
        trex.extend_from_slice(&1024u32.to_be_bytes());
        trex.extend_from_slice(&0u32.to_be_bytes());
        trex.extend_from_slice(&0x0101_0000u32.to_be_bytes());
        parts.push(boxed(b"mvex", &[&full(b"trex", 0, 0, &trex)]));
    }
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    boxed(b"moov", &refs)
}

fn stsd() -> Vec<u8> {
    full(
        b"stsd",
        0,
        0,
        &[&1u32.to_be_bytes()[..], &boxed(b"test", &[&[0u8; 8]])].concat(),
    )
}

fn empty_stbl() -> Vec<u8> {
    let none = 0u32.to_be_bytes();
    boxed(
        b"stbl",
        &[
            &stsd(),
            &full(b"stts", 0, 0, &none),
            &full(b"stsc", 0, 0, &none),
            &full(b"stsz", 0, 0, &[0u8; 8]),
            &full(b"stco", 0, 0, &none),
        ],
    )
}

/// A fragmented file: an empty index, then fragments of three samples,
/// the second sample of each not a sync sample, data counted from its
/// moof. Sample n of the file is n bytes of the byte n.
pub fn fragmented(dir: &Path, name: &str, fragments: u32) -> (PathBuf, Vec<Vec<u8>>) {
    fragmented_from(dir, name, fragments, 0)
}

/// [`fragmented`], its first fragment starting at `start` (`tfdt`,
/// written only where that is not zero).
pub fn fragmented_from(
    dir: &Path,
    name: &str,
    fragments: u32,
    start: u64,
) -> (PathBuf, Vec<Vec<u8>>) {
    let mut file = boxed(b"ftyp", &[b"iso5", &[0u8; 4]]);
    file.extend(moov_of(1, b"vide", 90000, &empty_stbl(), true));
    let mut samples = Vec::new();
    let mut n = 1u8;
    for f in 0..fragments {
        let sizes: Vec<u32> = (0..3).map(|i| u32::from(n) + i).collect();
        let mut trun_entries = Vec::new();
        for (i, size) in sizes.iter().enumerate() {
            trun_entries.extend_from_slice(&3000u32.to_be_bytes());
            trun_entries.extend_from_slice(&size.to_be_bytes());
            let flags: u32 = if i == 1 { 0x0101_0000 } else { 0x0200_0000 };
            trun_entries.extend_from_slice(&flags.to_be_bytes());
        }
        let build = |offset: u32| {
            let mfhd = full(b"mfhd", 0, 0, &(f + 1).to_be_bytes());
            let tfhd = full(b"tfhd", 0, 0x2_0000, &1u32.to_be_bytes());
            let decode_time = start + u64::from(f) * 9000;
            let tfdt = full(b"tfdt", 1, 0, &decode_time.to_be_bytes());
            let mut trun = 3u32.to_be_bytes().to_vec();
            trun.extend_from_slice(&offset.to_be_bytes());
            trun.extend_from_slice(&trun_entries);
            let traf = if start > 0 {
                boxed(b"traf", &[&tfhd, &tfdt, &full(b"trun", 0, 0x701, &trun)])
            } else {
                boxed(b"traf", &[&tfhd, &full(b"trun", 0, 0x701, &trun)])
            };
            boxed(b"moof", &[&mfhd, &traf])
        };
        let moof_len = build(0).len() as u32;
        file.extend(build(moof_len + 8));
        let mut data = Vec::new();
        for size in &sizes {
            let sample = vec![n; *size as usize];
            data.extend_from_slice(&sample);
            samples.push(sample);
            n += 1;
        }
        file.extend(boxed(b"mdat", &[&data]));
    }
    let path = dir.join(name);
    std::fs::write(&path, file).unwrap();
    (path, samples)
}

/// A plain file of `count` samples of 10 bytes, two per chunk.
pub fn plain(dir: &Path, name: &str, count: u32, byte: u8) -> (PathBuf, Vec<Vec<u8>>) {
    let ftyp = boxed(b"ftyp", &[b"M4A ", &[0u8; 4]]);
    let samples: Vec<Vec<u8>> = (0..count)
        .map(|i| vec![byte.wrapping_add(i as u8); 10])
        .collect();
    let build = |data_at: u32| {
        let chunks = count.div_ceil(2);
        let mut stco = chunks.to_be_bytes().to_vec();
        for c in 0..chunks {
            stco.extend_from_slice(&(data_at + c * 20).to_be_bytes());
        }
        let mut stsz = 10u32.to_be_bytes().to_vec();
        stsz.extend_from_slice(&count.to_be_bytes());
        let mut stsc = 1u32.to_be_bytes().to_vec();
        stsc.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1]);
        let mut stts = 1u32.to_be_bytes().to_vec();
        stts.extend_from_slice(&count.to_be_bytes());
        stts.extend_from_slice(&1024u32.to_be_bytes());
        let stbl = boxed(
            b"stbl",
            &[
                &stsd(),
                &full(b"stts", 0, 0, &stts),
                &full(b"stsc", 0, 0, &stsc),
                &full(b"stsz", 0, 0, &stsz),
                &full(b"stco", 0, 0, &stco),
            ],
        );
        moov_of(1, b"soun", 48000, &stbl, false)
    };
    let moov_len = build(0).len() as u32;
    let mut file = ftyp.clone();
    file.extend(build(ftyp.len() as u32 + moov_len + 8));
    file.extend(boxed(b"mdat", &[&samples.concat()]));
    let path = dir.join(name);
    std::fs::write(&path, file).unwrap();
    (path, samples)
}

/// The samples of the one track in the MP4 at `path`, as bytes, and their durations.
pub fn samples_in(path: &Path) -> (Vec<Vec<u8>>, Vec<u32>) {
    let data = std::fs::read(path).unwrap();
    let tracks = tracks_in_file(path).unwrap();
    assert_eq!(tracks.len(), 1, "one track");
    let bytes = tracks[0]
        .samples
        .iter()
        .map(|s| data[s.offset as usize..(s.offset + u64::from(s.size)) as usize].to_vec())
        .collect();
    let lengths = tracks[0].samples.iter().map(|s| s.duration).collect();
    (bytes, lengths)
}

/// One track of an MP4 as a test reads it back (gaps row Y16): its handler, timescale, each
/// sample description whole, each sample's bytes, length and description (from 1), and when its
/// first sample plays where its fragments say.
#[derive(Debug)]
pub struct ReadBack {
    /// `vide`, `soun`...
    pub handler: [u8; 4],
    /// Ticks a second.
    pub timescale: u32,
    /// Each sample description, whole.
    pub entries: Vec<Vec<u8>>,
    /// Each sample's bytes.
    pub samples: Vec<Vec<u8>>,
    /// Each sample's length, in ticks.
    pub durations: Vec<u32>,
    /// Each sample's description, from 1.
    pub described: Vec<u32>,
    /// When its first sample plays, where its fragments say.
    pub start: Option<u64>,
}

/// Every track of the MP4 at `path`, plain or fragmented (in runs too), read back.
pub fn read_back(path: &Path) -> Vec<ReadBack> {
    let data = std::fs::read(path).unwrap();
    tracks_of_input(path, 0)
        .unwrap()
        .into_iter()
        .map(|track| ReadBack {
            handler: track.hdlr[16..20].try_into().unwrap(),
            timescale: track.timescale,
            entries: runs::entries_of(&track.stsd)
                .into_iter()
                .map(<[u8]>::to_vec)
                .collect(),
            samples: track
                .samples
                .iter()
                .map(|s| data[s.offset as usize..(s.offset + u64::from(s.size)) as usize].to_vec())
                .collect(),
            durations: track.samples.iter().map(|s| s.duration).collect(),
            described: (0..track.samples.len())
                .map(|i| track.description_of(i))
                .collect(),
            start: track.start,
        })
        .collect()
}
