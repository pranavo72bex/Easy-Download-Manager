// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Joins a video's picture and sound into one WebM or Matroska file, by EDM
//! itself.
//!
//! A video site's WebM qualities (VP9 or AV1 picture, Opus sound) come as
//! two WebM files, as its MP4 ones do (`mp4mux`). This reads each input's
//! tracks and blocks — the frames, with when each plays — and writes one
//! file: both tracks, the blocks of both in time order in clusters that
//! start at a picture's key frame, an index of those (`Cues`) so a player
//! can seek, and a `SeekHead` to find it. The frames are copied byte for
//! byte, laced or not: nothing is decoded or encoded.
//!
//! An input may be an MP4 as well (`from_mp4`): its samples become blocks,
//! so a pair no MP4 holds (a VP8 or Vorbis WebM beside an MP4) is joined
//! into a Matroska file. The file says it is WebM where every track is a
//! WebM codec, Matroska otherwise.
//!
//! The inputs came from a server: nothing here indexes past what it
//! checked, and a file whose blocks point outside it is refused.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::Error;

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const SEEK: u32 = 0x4DBB;
const SEEK_ID: u32 = 0x53AB;
const SEEK_POSITION: u32 = 0x53AC;
const INFO: u32 = 0x1549_A966;
const TIMECODE_SCALE: u32 = 0x2A_D7B1;
const DURATION: u32 = 0x4489;
const MUXING_APP: u32 = 0x4D80;
const WRITING_APP: u32 = 0x5741;
const TITLE: u32 = 0x7BA9;
const TAGS: u32 = 0x1254_C367;
const TAG: u32 = 0x7373;
const TARGETS: u32 = 0x63C0;
const TARGET_TYPE_VALUE: u32 = 0x68CA;
const SIMPLE_TAG: u32 = 0x67C8;
const TAG_NAME: u32 = 0x45A3;
const TAG_STRING: u32 = 0x4487;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const TRACK_UID: u32 = 0x73C5;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const CLUSTER: u32 = 0x1F43_B675;
const TIMECODE: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const DISCARD_PADDING: u32 = 0x75A2;
const REFERENCE_BLOCK: u32 = 0xFB;
const CUES: u32 = 0x1C53_BB6B;
const CUE_POINT: u32 = 0xBB;
const CUE_TIME: u32 = 0xB3;
const CUE_TRACK_POSITIONS: u32 = 0xB7;
const CUE_TRACK: u32 = 0xF7;
const CUE_CLUSTER_POSITION: u32 = 0xF1;

/// Elements whose start ends a cluster of unknown size.
const TOP_LEVEL: [u32; 8] = [
    CLUSTER,
    CUES,
    INFO,
    TRACKS,
    SEEK_HEAD,
    0x1254_C367,
    0x1043_A770,
    0x1941_A469,
];

/// The output's tick: a millisecond, as WebM writers use.
const SCALE_NS: u64 = 1_000_000;
/// A cluster holds at most this long, so a block's offset fits in 16 bits.
const CLUSTER_MS: i64 = 30_000;

#[derive(Debug, Clone)]
struct TrackIn {
    number: u64,
    kind: u64,
    codec: String,
    /// The entry's elements but its number and UID, as they are.
    rest: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Block {
    input: usize,
    track: u64,
    /// Cluster time plus the block's own offset, in the input's ticks:
    /// when the frame shows.
    ticks: i64,
    /// When the frame is decoded, in the same ticks: the order blocks are
    /// written in. A WebM's are its show times; an MP4 picture with frames
    /// shown out of order has its own (`from_mp4`).
    decode: i64,
    key: bool,
    flags: u8,
    /// The frame data after the block's header, in its input.
    data_at: u64,
    data_len: u64,
    /// A block group's other elements (duration, references, padding).
    group: Option<Vec<u8>>,
}

struct Input {
    scale: u64,
    tracks: Vec<TrackIn>,
    blocks: Vec<Block>,
    /// Where an MP4 input's fragments say its first track starts (`tfdt`),
    /// in its ticks: the inputs that say are moved together so the
    /// earliest starts at zero, as the MP4 writer does.
    origin: Option<i64>,
}

// ---- EBML ---------------------------------------------------------------

/// A variable-length integer's length from its first byte.
fn vint_len(first: u8) -> Option<usize> {
    let zeros = first.leading_zeros() as usize;
    (zeros < 8).then_some(zeros + 1)
}

/// An element ID (marker kept) from `data`.
fn read_id(data: &[u8]) -> Option<(u32, usize)> {
    let len = vint_len(*data.first()?)?;
    if len > 4 {
        return None;
    }
    let mut id = 0u32;
    for b in data.get(..len)? {
        id = (id << 8) | u32::from(*b);
    }
    Some((id, len))
}

/// A size (marker stripped) from `data`; `None` inside for "unknown".
fn read_size(data: &[u8]) -> Option<(Option<u64>, usize)> {
    let first = *data.first()?;
    let len = vint_len(first)?;
    let mask = 0xFFu8.checked_shr(len as u32).unwrap_or(0);
    let mut value = u64::from(first & mask);
    let mut all_ones = value == u64::from(mask);
    for b in data.get(1..len)? {
        value = (value << 8) | u64::from(*b);
        all_ones &= *b == 0xFF;
    }
    Some((if all_ones { None } else { Some(value) }, len))
}

/// The elements in `data`: each one's ID, body and whole.
fn elements(data: &[u8]) -> impl Iterator<Item = (u32, &[u8], &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let (id, id_len) = read_id(data.get(at..)?)?;
        let (size, size_len) = read_size(data.get(at + id_len..)?)?;
        let start = at + id_len + size_len;
        let end = match size {
            Some(s) => start.checked_add(usize::try_from(s).ok()?)?,
            None => data.len(),
        };
        let body = data.get(start..end)?;
        let whole = data.get(at..end)?;
        at = end;
        Some((id, body, whole))
    })
}

fn uint(body: &[u8]) -> u64 {
    body.iter().take(8).fold(0, |v, b| (v << 8) | u64::from(*b))
}

fn id_bytes(id: u32) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let skip = bytes.iter().take_while(|b| **b == 0).count().min(3);
    bytes.get(skip..).unwrap_or_default().to_vec()
}

/// A size written in eight bytes, so it can be patched in place.
fn size8(size: u64) -> [u8; 8] {
    let mut out = size.to_be_bytes();
    out[0] = 0x01;
    out
}

fn element(id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = id_bytes(id);
    out.extend_from_slice(&size8(body.len() as u64));
    out.extend_from_slice(body);
    out
}

fn uint_element(id: u32, value: u64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let skip = bytes.iter().take_while(|b| **b == 0).count().min(7);
    element(id, bytes.get(skip..).unwrap_or_default())
}

// ---- reading ------------------------------------------------------------

/// A buffered file that knows where it is, so a read a little way on moves
/// within its buffer rather than throwing it away (`seek` would, and a
/// block-by-block read then refilled a megabyte per block).
struct Positioned {
    file: BufReader<File>,
    pos: u64,
}

impl Positioned {
    fn open(path: &Path) -> std::io::Result<Positioned> {
        Ok(Positioned {
            file: BufReader::with_capacity(1 << 20, File::open(path)?),
            pos: 0,
        })
    }

    fn read_into(&mut self, at: u64, buf: &mut [u8]) -> std::io::Result<()> {
        if at != self.pos {
            let delta =
                i64::try_from(at).unwrap_or(i64::MAX) - i64::try_from(self.pos).unwrap_or(i64::MAX);
            self.file.seek_relative(delta)?;
        }
        self.file.read_exact(buf)?;
        self.pos = at + buf.len() as u64;
        Ok(())
    }
}

struct Reader {
    file: Positioned,
    length: u64,
}

// ---- writing ------------------------------------------------------------

/// A block as written: its new track, when it shows and when it is
/// decoded, in milliseconds.
struct Placed<'a> {
    block: &'a Block,
    track: u64,
    ms: i64,
    decode_ms: i64,
    video: bool,
}

/// Joins the WebM, Matroska or MP4 files `inputs` into one at `out`.
/// Returns its length.
pub fn join(inputs: &[PathBuf], out: &Path) -> Result<u64, String> {
    join_with_text(inputs, &[], out)
}

/// The subtitles as WebVTT tracks, their cues' text written on the end of
/// `data`, which is input `input`.
fn text_input(texts: &[crate::subtitles::Subtitle], input: usize, data: &mut Vec<u8>) -> Input {
    let mut tracks = Vec::new();
    let mut blocks = Vec::new();
    for (n, text) in texts.iter().enumerate() {
        let number = n as u64 + 1;
        let mut rest = uint_element(TRACK_TYPE, 0x11);
        rest.extend(element(CODEC_ID, b"D_WEBVTT/SUBTITLES"));
        rest.extend(element(
            0x22_B59C,
            &crate::subtitles::iso639_2(&text.language),
        ));
        rest.extend(uint_element(0x88, 0)); // not shown unless picked
        rest.extend(uint_element(0x9C, 0)); // no lacing
        tracks.push(TrackIn {
            number,
            kind: 0x11,
            codec: "D_WEBVTT/SUBTITLES".into(),
            rest,
        });
        for cue in &text.cues {
            // WebM's WebVTT block: the cue's identifier line and settings
            // line (both empty here), then its text. Readers drop a block
            // without the first two.
            let data_at = data.len() as u64;
            data.extend_from_slice(b"\n\n");
            data.extend_from_slice(cue.text.as_bytes());
            let ticks = i64::try_from(cue.start).unwrap_or(i64::MAX);
            blocks.push(Block {
                input,
                track: number,
                ticks,
                decode: ticks,
                key: true,
                flags: 0,
                data_at,
                data_len: cue.text.len() as u64 + 2,
                // How long it shows.
                group: Some(uint_element(0x9B, cue.end.saturating_sub(cue.start))),
            });
        }
    }
    Input {
        scale: SCALE_NS,
        tracks,
        blocks,
        origin: None,
    }
}

/// [`join`], with `texts` carried as subtitle tracks, one per language.
pub fn join_with_text(
    inputs: &[PathBuf],
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
) -> Result<u64, String> {
    join_tagged(inputs, texts, out, &crate::mp4mux::Tags::default())
}

/// [`join_with_text`], the file saying its title in its Info and its artist, date and page in
/// Matroska's Tags, as players read them (gaps row Y11).
pub fn join_tagged(
    inputs: &[PathBuf],
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
    tags: &crate::mp4mux::Tags,
) -> Result<u64, String> {
    placed::join_tagged_placed(inputs, |_| texts.to_vec(), out, tags).map_err(String::from)
}

/// Moves the MP4 inputs whose fragments say when they start together, so
/// the earliest starts at zero, as the MP4 writer does. A WebM beside them
/// (`webm`, by input) is placed by its first block, as the MP4 writer
/// places it: a live stream's WebM picture keeps its own clock, which is
/// the clock the MP4 sound's fragments count on. An MP4 that does not say
/// keeps its own times, and so does every input where none says.
fn line_up(read: &mut [Input], webm: &[bool]) {
    if read.iter().all(|input| input.origin.is_none()) {
        return;
    }
    for (input, _) in read
        .iter_mut()
        .zip(webm)
        .filter(|(input, webm)| **webm && input.origin.is_none())
    {
        input.origin = input.blocks.iter().map(|b| b.ticks.max(0)).min();
    }
    let earliest_ns = read
        .iter()
        .filter_map(|input| {
            input
                .origin
                .map(|o| i128::from(o) * i128::from(input.scale))
        })
        .min();
    let Some(earliest_ns) = earliest_ns else {
        return;
    };
    for input in read.iter_mut().filter(|i| i.origin.is_some()) {
        let by = i64::try_from(earliest_ns / i128::from(input.scale.max(1))).unwrap_or(0);
        for block in &mut input.blocks {
            block.ticks = block.ticks.saturating_sub(by);
            block.decode = block.decode.saturating_sub(by);
        }
    }
}

fn join_read(
    read: &[Input],
    inputs: &[PathBuf],
    out: &Path,
    tags: &crate::mp4mux::Tags,
) -> Result<u64, Error> {
    // Pictures first, then sound, then subtitles; each numbered afresh.
    let mut order: Vec<(usize, &TrackIn)> = read
        .iter()
        .enumerate()
        .flat_map(|(i, input)| input.tracks.iter().map(move |t| (i, t)))
        .filter(|(i, t)| {
            read.get(*i)
                .is_some_and(|r| r.blocks.iter().any(|b| b.track == t.number))
        })
        .collect();
    order.sort_by_key(|(i, t)| (t.kind != 1, t.kind == 0x11, *i));
    if order.is_empty() {
        return Err("there is nothing to join".into());
    }
    let number_of = |input: usize, track: u64| {
        order
            .iter()
            .position(|(i, t)| *i == input && t.number == track)
            .map(|p| p as u64 + 1)
    };
    let mut placed: Vec<Placed> = Vec::new();
    for input in read {
        for block in &input.blocks {
            let Some(track) = number_of(block.input, block.track) else {
                continue;
            };
            let ms = |ticks: i64| {
                let ns = i128::from(ticks) * i128::from(input.scale);
                (ns / i128::from(SCALE_NS)) as i64
            };
            placed.push(Placed {
                block,
                track,
                ms: ms(block.ticks),
                decode_ms: ms(block.decode),
                video: order
                    .get(track as usize - 1)
                    .is_some_and(|(_, t)| t.kind == 1),
            });
        }
    }
    // A block before zero (an Opus encoder's delay) is kept there: the
    // first cluster starts at zero and the block's own offset is negative.
    if placed.iter().any(|p| p.ms < -30_000) {
        return Err("a track starts too far before the others".into());
    }
    // In decoding order: a picture shown out of order (an MP4's B-frames)
    // keeps the order its decoder needs, each block with its show time.
    placed.sort_by_key(|p| (p.decode_ms, !p.video));
    let webm = order.iter().all(|(_, t)| {
        [
            "V_VP8",
            "V_VP9",
            "V_AV1",
            "A_OPUS",
            "A_VORBIS",
            "D_WEBVTT/SUBTITLES",
        ]
        .contains(&t.codec.as_str())
    });
    let has_video = order.iter().any(|(_, t)| t.kind == 1);
    let mut files: Vec<Positioned> = inputs
        .iter()
        .map(|p| Positioned::open(p))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("an input could not be read: {e}"))?;
    write(out, &order, &placed, &mut files, webm, has_video, tags)
}

/// Matroska's Tags for the whole file (target 50, the album or film): its title, artist, date
/// and page as simple tags, by the names Matroska gives them; `None` with nothing to say.
fn tags_element(tags: &crate::mp4mux::Tags) -> Option<Vec<u8>> {
    let simple: Vec<u8> = [
        ("TITLE", &tags.title),
        ("ARTIST", &tags.artist),
        ("DATE_RELEASED", &tags.date),
        ("URL", &tags.source),
    ]
    .into_iter()
    .filter(|(_, value)| !value.trim().is_empty())
    .flat_map(|(name, value)| {
        let body = [
            element(TAG_NAME, name.as_bytes()),
            element(TAG_STRING, value.trim().as_bytes()),
        ]
        .concat();
        element(SIMPLE_TAG, &body)
    })
    .collect();
    if simple.is_empty() {
        return None;
    }
    let targets = element(TARGETS, &uint_element(TARGET_TYPE_VALUE, 50));
    Some(element(TAGS, &element(TAG, &[targets, simple].concat())))
}

fn write(
    out: &Path,
    order: &[(usize, &TrackIn)],
    placed: &[Placed],
    files: &mut [Positioned],
    webm: bool,
    has_video: bool,
    tags: &crate::mp4mux::Tags,
) -> Result<u64, Error> {
    let wrote = |e: std::io::Error| format!("{} could not be written: {e}", out.display());
    let mut w = BufWriter::with_capacity(1 << 20, File::create(out).map_err(wrote)?);
    let header = [
        uint_element(0x4286, 1),
        uint_element(0x42F7, 1),
        uint_element(0x42F2, 4),
        uint_element(0x42F3, 8),
        element(0x4282, if webm { b"webm" } else { b"matroska" }),
        uint_element(0x4287, 4),
        uint_element(0x4285, 2),
    ]
    .concat();
    w.write_all(&element(EBML, &header)).map_err(wrote)?;
    let segment_at = (id_bytes(EBML).len() + 8 + header.len()) as u64;
    w.write_all(&id_bytes(SEGMENT)).map_err(wrote)?;
    w.write_all(&size8(0)).map_err(wrote)?;
    let data_start = segment_at + 4 + 8;

    // The SeekHead, its positions written in eight bytes to patch later.
    let seek = |id: u32, position: u64| {
        let mut body = element(SEEK_ID, &id_bytes(id));
        body.extend(element(SEEK_POSITION, &position.to_be_bytes()));
        element(SEEK, &body)
    };
    let seek_head = |info: u64, tracks: u64, cues: u64| {
        element(
            SEEK_HEAD,
            &[seek(INFO, info), seek(TRACKS, tracks), seek(CUES, cues)].concat(),
        )
    };
    let seek_len = seek_head(0, 0, 0).len() as u64;
    w.write_all(&seek_head(0, 0, 0)).map_err(wrote)?;

    let duration = placed.iter().map(|p| p.ms).max().unwrap_or(0) as f64 + 1.0;
    let info = element(
        INFO,
        &[
            uint_element(TIMECODE_SCALE, SCALE_NS),
            element(DURATION, &duration.to_be_bytes()),
            element(MUXING_APP, b"EDM"),
            element(WRITING_APP, b"EDM"),
            // Its title, as players show it.
            if tags.title.trim().is_empty() {
                Vec::new()
            } else {
                element(TITLE, tags.title.trim().as_bytes())
            },
        ]
        .concat(),
    );
    let info_at = seek_len;
    w.write_all(&info).map_err(wrote)?;
    let tracks_at = info_at + info.len() as u64;
    let mut entries = Vec::new();
    for (n, (_, track)) in order.iter().enumerate() {
        let number = n as u64 + 1;
        let mut body = uint_element(TRACK_NUMBER, number);
        body.extend(uint_element(TRACK_UID, number));
        body.extend_from_slice(&track.rest);
        entries.extend(element(TRACK_ENTRY, &body));
    }
    let tracks = element(TRACKS, &entries);
    w.write_all(&tracks).map_err(wrote)?;
    let mut at = tracks_at + tracks.len() as u64;
    // Its words, before the first cluster, where a reader meets them as it reads the head.
    if let Some(said) = tags_element(tags) {
        w.write_all(&said).map_err(wrote)?;
        at += said.len() as u64;
    }

    let (at, cues) = clusters::write_clusters(&mut w, out, placed, files, has_video, at)?;

    let cues_at = at;
    let mut points = Vec::new();
    for (ms, track, position) in cues {
        let positions = [
            uint_element(CUE_TRACK, track),
            uint_element(CUE_CLUSTER_POSITION, position),
        ]
        .concat();
        points.extend(element(
            CUE_POINT,
            &[
                uint_element(CUE_TIME, ms as u64),
                element(CUE_TRACK_POSITIONS, &positions),
            ]
            .concat(),
        ));
    }
    let cues = element(CUES, &points);
    w.write_all(&cues).map_err(wrote)?;
    let segment_len = cues_at + cues.len() as u64;
    w.flush().map_err(wrote)?;
    let mut file = w.into_inner().map_err(|e| wrote(e.into_error()))?;
    // The sizes and positions only known now.
    file.seek(SeekFrom::Start(segment_at + 4)).map_err(wrote)?;
    file.write_all(&size8(segment_len)).map_err(wrote)?;
    file.seek(SeekFrom::Start(data_start)).map_err(wrote)?;
    file.write_all(&seek_head(info_at, tracks_at, cues_at))
        .map_err(wrote)?;
    file.sync_all().map_err(wrote)?;
    Ok(data_start + segment_len)
}

// ---- which container a join needs ---------------------------------------

/// The codecs an MP4 EDM writes takes from a WebM (`mp4_tracks`).
const MP4_TAKES: [&str; 3] = ["V_VP9", "V_AV1", "A_OPUS"];

mod clusters;
mod codecs;
mod from_mp4;
mod placed;
pub use placed::join_placed;
mod probe;
mod read;
mod sound;
mod to_mp4;

pub(crate) use codecs::mp4_tracks;
#[cfg(test)]
use codecs::sint;
use codecs::{opus_samples, opus_shown, vp9_shape};
use probe::find;
pub use probe::needs_matroska;
pub(crate) use probe::{is_webm, sound_codec, tags};
#[cfg(test)]
use read::{block_header, read_tracks};
use read::{read_input, read_webm};
pub(crate) use sound::sound_track;

/// Small WebM files made for tests (edm-core's tests too, through the `testing` feature).
#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_element_is_read_with_its_sign() {
        assert_eq!(sint(&[0x01, 0x00]), 256);
        assert_eq!(sint(&[0xFF]), -1);
        assert_eq!(sint(&[0xFE, 0x0C]), -500);
        assert_eq!(sint(&[]), 0);
    }

    #[test]
    fn sizes_and_ids_read_back() {
        assert_eq!(read_size(&[0x81]), Some((Some(1), 1)));
        assert_eq!(read_size(&[0x40, 0x02]), Some((Some(2), 2)));
        assert_eq!(read_size(&[0xFF]), Some((None, 1)));
        assert_eq!(read_size(&size8(300)), Some((Some(300), 8)));
        assert_eq!(read_id(&[0x1A, 0x45, 0xDF, 0xA3]), Some((EBML, 4)));
        assert_eq!(read_id(&id_bytes(TIMECODE)), Some((TIMECODE, 1)));
        assert_eq!(read_size(&[0x00]), None);
    }

    #[test]
    fn what_is_not_a_webm_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.webm");
        std::fs::write(&path, b"<!doctype html>").unwrap();
        assert!(join(&[path], &dir.path().join("out.webm")).is_err());
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..800)) {
            let _ = elements(&data).count();
            let _ = read_tracks(&data);
            let _ = block_header(&data);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("in.webm");
            let mut file = element(EBML, &[]);
            file.extend(&data);
            std::fs::write(&path, &file).unwrap();
            let _ = join(std::slice::from_ref(&path), &dir.path().join("out.webm"));
            // The reader that puts a WebM's tracks into an MP4.
            let _ = mp4_tracks(&path, 0);
        }
    }
}
