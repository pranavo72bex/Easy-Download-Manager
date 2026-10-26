//! Puts an HLS stream's MPEG transport stream (`.ts`) into an MP4, by dm
//! itself.
//!
//! Players open an MP4; few open a `.ts`. An HLS stream's segments are a
//! transport stream: 188-byte packets carrying H.264 or HEVC pictures and
//! AAC, MP3/MP2, AC-3 or E-AC-3 sound
//! in PES packets, each stamped with when it plays. This reads them out —
//! the tables that say which packets are which stream, the pictures as the
//! byte stream an encoder wrote and the sound as ADTS frames — and hands
//! them to the MP4 writer (`mp4mux`) as samples: each picture with its
//! start codes turned into lengths, each sound frame without its ADTS
//! header, picture and sound started together. Nothing is decoded or
//! encoded. A stream in anything else (HEVC, MP3, AC-3) is refused, and the
//! caller keeps the `.ts`.
//!
//! The input came from a server: nothing here indexes past what it checked.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::mp4mux::{boxed, full, Edit, Sample, Track, MATRIX};

const PACKET: usize = 188;
const TIMESCALE: u32 = 90_000;
/// PTS and DTS are 33 bits and wrap.
const WRAP: i64 = 1 << 33;

const H264: u8 = 0x1b;
const HEVC: u8 = 0x24;
const AAC: u8 = 0x0f;
const MPEG1_AUDIO: u8 = 0x03;
const MPEG2_AUDIO: u8 = 0x04;
const AC3: u8 = 0x81;
const EAC3: u8 = 0x87;
const PICTURES: [u8; 2] = [H264, HEVC];
const SOUNDS: [u8; 5] = [AAC, MPEG1_AUDIO, MPEG2_AUDIO, AC3, EAC3];
/// Picture and sound kinds this does not carry: MPEG-1/2 and MPEG-4 Part 2
/// pictures, AVS; LATM AAC. A stream with one is refused whole rather than
/// put into an MP4 without it.
const OTHERS: [u8; 5] = [0x01, 0x02, 0x10, 0x42, 0x11];

/// The shape of a sound stream, from its first frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sound {
    Aac {
        profile: u8,
        rate_index: u8,
        channels: u8,
    },
    /// MPEG-1 or -2 audio (MP3, MP2): its frames go in whole.
    Mpeg {
        mpeg1: bool,
        rate: u32,
        channels: u16,
        per_frame: u32,
    },
    Ac3 {
        dac3: [u8; 3],
        rate: u32,
        channels: u16,
    },
    Eac3 {
        dec3: [u8; 5],
        rate: u32,
        channels: u16,
        per_frame: u32,
    },
}

impl Sound {
    pub(crate) fn per_frame(self) -> u32 {
        match self {
            Sound::Aac { .. } => 1024,
            Sound::Mpeg { per_frame, .. } => per_frame,
            Sound::Ac3 { .. } => 1536,
            Sound::Eac3 { per_frame, .. } => per_frame,
        }
    }
}

/// One elementary stream being read out.
#[derive(Default)]
struct Elementary {
    kind: u8,
    /// The PES packet being gathered.
    pes: Vec<u8>,
    /// Where its converted samples go.
    out: Option<BufWriter<File>>,
    path: PathBuf,
    written: u64,
    /// Picture: per sample, its offset, size, PTS, DTS and whether it is a
    /// key frame. Sound: per frame, its offset and size.
    samples: Vec<(u64, u32, i64, i64, bool)>,
    vps: Option<Vec<u8>>,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    /// Sound: the bytes not yet a whole frame, and the stream's shape.
    adts: Vec<u8>,
    sound: Option<Sound>,
    first_pts: Option<i64>,
    last_pts: Option<i64>,
}

/// Puts the transport stream at `input` into an MP4 at `out`, with scratch
/// files beside `out` that are gone when it returns. Returns its length.
pub fn to_mp4(input: &Path, out: &Path) -> Result<u64, String> {
    let scratch = |what: &str| {
        let mut name = out.as_os_str().to_owned();
        name.push(format!(".{what}"));
        PathBuf::from(name)
    };
    let mut streams = read_ts(input, &scratch)?;
    let result = build(&mut streams, out);
    for stream in &streams {
        let _ = std::fs::remove_file(&stream.path);
    }
    result
}

/// [`to_mp4`], off the async runtime.
pub async fn to_mp4_async(input: PathBuf, out: PathBuf) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || to_mp4(&input, &out))
        .await
        .map_err(|e| format!("the remux stopped: {e}"))?
}

mod demux;
mod headers;
mod packed;
mod samples;
mod sound;
mod tracks;

use demux::*;
pub(crate) use headers::*;
pub use packed::{is_packed_aac, is_transport_stream, join_packed, join_packed_async};
use samples::*;
pub(crate) use sound::*;
use tracks::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_keep_rising_across_a_wrap() {
        assert_eq!(unwrap(100, None), 100);
        assert_eq!(unwrap(10, Some(WRAP - 10)), WRAP + 10);
        assert_eq!(unwrap(WRAP - 10, Some(WRAP + 5)), WRAP - 10);
    }

    #[test]
    fn a_byte_stream_splits_into_its_units() {
        let data = [
            0, 0, 0, 1, 9, 0xf0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x65, 7, 7, 0,
        ];
        let units = nal_units(&data);
        assert_eq!(units, [&[9, 0xf0][..], &[0x67, 1, 2], &[0x65, 7, 7]]);
    }

    #[test]
    fn what_is_not_a_transport_stream_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("page.ts");
        std::fs::write(&input, vec![b'x'; 188 * 1200]).unwrap();
        assert!(to_mp4(&input, &dir.path().join("out.mp4")).is_err());
        let empty = dir.path().join("empty.ts");
        std::fs::write(&empty, b"").unwrap();
        assert!(to_mp4(&empty, &dir.path().join("out.mp4")).is_err());
        // Nothing left behind.
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert_eq!(left.len(), 2);
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..2000)) {
            let _ = picture_size(&data);
            let _ = read_pat(&data);
            let _ = read_pmt(&data);
            let _ = nal_units(&data);
            let mut stream = Elementary { kind: AAC, ..Elementary::default() };
            let _ = take_pes(&mut stream, &data);
            let mut stream = Elementary { kind: H264, ..Elementary::default() };
            let _ = take_pes(&mut stream, &data);
        }

        #[test]
        fn any_packets_are_read_without_a_panic(seed in proptest::collection::vec(proptest::num::u8::ANY, 0..188 * 6)) {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("in.ts");
            let mut data = seed.clone();
            for chunk in data.chunks_mut(188) {
                chunk[0] = 0x47;
            }
            std::fs::write(&input, &data).unwrap();
            let _ = to_mp4(&input, &dir.path().join("out.mp4"));
        }
    }
}
