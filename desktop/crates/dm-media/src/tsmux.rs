// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
//! encoded. A stream in anything else (MPEG-1/2 or MPEG-4 Part 2 pictures,
//! AVS, LATM AAC; `OTHERS`) is refused, and the caller keeps the `.ts`.
//!
//! The input came from a server: nothing here indexes past what it checked.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::mp4mux::{boxed, full, Edit, Sample, Source, Track, MATRIX};

const PACKET: usize = 188;
const TIMESCALE: u32 = 90_000;
/// PTS and DTS are 33 bits and wrap.
const WRAP: i64 = 1 << 33;
/// A stream's clock leaping ahead by more than this is a discontinuity, not time that passed:
/// ten seconds, where a picture follows the one before it within a fraction of one.
const JUMP: i64 = 10 * TIMESCALE as i64;
/// Or going back by more than this: half a second. A picture is decoded after the one before
/// it, and a sound frame played after it, so their clocks never go back; a few frames' slack.
const JUMP_BACK: i64 = TIMESCALE as i64 / 2;
/// How far after the last picture the first one past a discontinuity is put: a frame at 30 a
/// second.
const STEP: i64 = TIMESCALE as i64 / 30;

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
/// What HLS's SAMPLE-AES calls H.264 and ADTS AAC whose samples are scrambled, and AC-3 and
/// E-AC-3 that are (Apple's transport-stream signalling for its sample encryption).
const SEALED_H264: u8 = 0xdb;
const SEALED_AAC: u8 = 0xcf;
const SEALED_AC3: u8 = 0xc1;
const SEALED_EAC3: u8 = 0xc2;

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
    /// The last PTS, as the samples carry it (moved on by `shift`).
    last_pts: Option<i64>,
    /// The last PTS as the stream stamped it, carried past a wrap: what the next is unwrapped
    /// against.
    raw_last: Option<i64>,
    /// When the last PES is decoded (its DTS, else its PTS), moved on by `shift`: what a
    /// discontinuity is told by.
    last_at: Option<i64>,
    /// Added to every timestamp from a discontinuity on (`take_pes`): an HLS stream's
    /// `EXT-X-DISCONTINUITY` (an advert, another encoder) starts its clock afresh, which read
    /// as it is put pictures back in time (gaps row Y16).
    shift: i64,
    /// `shift` after each discontinuity, in order: what the subtitles of the stretches after
    /// one are moved by ([`Clock::moved`], gaps row Y20).
    moves: Vec<i64>,
    /// Its samples are scrambled (HLS's SAMPLE-AES, gaps row Y16): each PES is put in the clear
    /// with the key and vector of the segment it came in.
    sealed: bool,
    /// The key and vector of the PES being gathered, and of the one being taken.
    gathering: crate::sealed::Lock,
    lock: crate::sealed::Lock,
    /// Where its samples go: its scratch file (`out`), counted alone, or kept to be read.
    sink: Sink,
}

/// What a remux tells the subtitles it carries (gaps row Y20): where the MP4 starts on the
/// stream's 90 kHz clock, and how far the picture's clock was moved at each discontinuity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clock {
    /// When the MP4 starts on the stream's clock: the earlier of its first picture and its
    /// first sound (both are started from it).
    pub zero: i64,
    /// How far each stretch of the picture's clock was moved, in its ticks: the first stretch
    /// (never, 0), then the one after each discontinuity its timestamps show (a leap ahead or
    /// back), in order. What the stretch stamped `t` plays at `t + moved - zero` of the MP4.
    pub moved: Vec<i64>,
}

/// Puts the transport stream at `input` into an MP4 at `out`, reading it twice and writing
/// nothing beside `out` (`replay`). Returns its length.
pub fn to_mp4(input: &Path, out: &Path) -> Result<u64, String> {
    to_mp4_clear(input, out, &crate::sealed::Locks::default())
}

/// [`to_mp4`] of a stream read from its segments where they lie, one after another as one
/// stream (an HLS stream's, gaps row Y21): no gathered copy of it is made. `segments` says
/// whether each is freed once its samples are in the MP4 ([`Segments::Freed`]) or left for the
/// caller ([`Segments::Kept`]).
pub fn to_mp4_from(inputs: &[PathBuf], out: &Path, segments: Segments) -> Result<u64, String> {
    to_mp4_from_with_text(inputs, out, segments, |_| Vec::new())
}

/// [`to_mp4_from`], with subtitles as [`to_mp4_with_text`] times them.
pub fn to_mp4_from_with_text<F>(
    inputs: &[PathBuf],
    out: &Path,
    segments: Segments,
    place: F,
) -> Result<u64, String>
where
    F: FnOnce(Clock) -> Vec<crate::subtitles::Subtitle>,
{
    let locks = crate::sealed::Locks::default();
    let reading = Reading {
        inputs,
        locks: &locks,
        freed: segments == Segments::Freed,
        watch: None,
    };
    remux(reading, out, place)
}

/// [`to_mp4_clear`] of a stream read from its segments where they lie, each left as it is
/// (`locks` by where each segment starts in the segments one after another).
pub fn to_mp4_from_clear(
    inputs: &[PathBuf],
    out: &Path,
    locks: &crate::sealed::Locks,
) -> Result<u64, String> {
    let reading = Reading {
        inputs,
        locks,
        freed: false,
        watch: None,
    };
    remux(reading, out, |_| Vec::new())
}

/// [`to_mp4`] of a stream whose pictures and sound are scrambled by HLS's SAMPLE-AES (gaps row
/// Y16): each segment's samples put in the clear with its key and vector (`locks`, by where each
/// segment starts in `input`), as Apple's "MPEG-2 Stream Encryption Format for HTTP Live
/// Streaming" describes. H.264 and ADTS AAC; scrambled AC-3 is refused.
pub fn to_mp4_clear(input: &Path, out: &Path, locks: &crate::sealed::Locks) -> Result<u64, String> {
    to_mp4_placed(input, out, locks, |_| Vec::new())
}

/// [`to_mp4`], with subtitles timed once the stream is read (an HLS stream's WebVTT segments,
/// gaps row Y20): `place` is given when the MP4 starts on the stream's 90 kHz clock, the
/// earlier of its first picture and its first sound (both are started from it), with how far
/// the picture's clock was moved at each discontinuity ([`Clock`]), and gives back the
/// subtitles timed from there, carried as text tracks. Asked only where there is a picture.
pub fn to_mp4_with_text<F>(input: &Path, out: &Path, place: F) -> Result<u64, String>
where
    F: FnOnce(Clock) -> Vec<crate::subtitles::Subtitle>,
{
    to_mp4_placed(input, out, &crate::sealed::Locks::default(), place)
}

/// [`to_mp4_clear`], with subtitles as [`to_mp4_with_text`] times them.
fn to_mp4_placed<F>(
    input: &Path,
    out: &Path,
    locks: &crate::sealed::Locks,
    place: F,
) -> Result<u64, String>
where
    F: FnOnce(Clock) -> Vec<crate::subtitles::Subtitle>,
{
    let inputs = [input.to_path_buf()];
    let reading = Reading {
        inputs: &inputs,
        locks,
        freed: false,
        watch: None,
    };
    remux(reading, out, place)
}

/// What the first bytes of an MPEG-1/2 audio frame (MP3, MP2) say of its sound: what a
/// recorder that writes such frames whole into an MP4 (dm-core's RTMP recorder) needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpegAudio {
    /// MPEG-1 audio, not MPEG-2's lower rates.
    pub mpeg1: bool,
    /// Samples a second.
    pub rate: u32,
    /// One or two.
    pub channels: u16,
    /// Samples a frame holds.
    pub per_frame: u32,
}

/// [`MpegAudio`] of the frame `frame` starts with; `None` where no whole MPEG audio frame
/// starts there.
pub fn mpeg_audio(frame: &[u8]) -> Option<MpegAudio> {
    match mpeg_audio_frame(frame) {
        Frame::Whole {
            shape:
                Sound::Mpeg {
                    mpeg1,
                    rate,
                    channels,
                    per_frame,
                },
            ..
        } => Some(MpegAudio {
            mpeg1,
            rate,
            channels,
            per_frame,
        }),
        _ => None,
    }
}

/// The picture's size, width then height, that an HEVC sequence parameter set (its two-byte
/// NAL header first) says.
pub fn hevc_picture_size(sps: &[u8]) -> Option<(u32, u32)> {
    hevc_sps(sps).map(|info| (info.width, info.height))
}

mod demux;
mod headers;
mod packed;
mod replay;
mod samples;
mod sound;
mod tracks;

use demux::*;
pub use headers::picture_size;
pub(crate) use headers::*;
pub use packed::{
    is_packed_aac, is_transport_stream, join_packed, join_packed_with_text, join_ts,
    join_ts_with_text,
};
pub use replay::Segments;
use replay::*;
use samples::*;
pub(crate) use sound::*;
use tracks::*;

#[cfg(test)]
mod remux_tests;

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
