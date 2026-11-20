// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Joins a video's picture and sound into one MP4, by dm itself.
//!
//! A video site serves its better qualities as two files, the picture alone
//! and the sound alone, each an MP4 — fragmented, as DASH serves them, or
//! plain. Other tools fetch both and leave the joining to a separate
//! program; this
//! does the joining itself: each input's samples are read from its own index
//! (the `moov` sample tables, or the `moof` fragments after an empty one),
//! and written as one plain MP4, the index first so it plays while it is
//! still being read, and picture and sound interleaved half a second at a
//! time. The samples themselves are copied byte for byte: nothing is
//! decoded or encoded, so it runs at the disk's speed.
//!
//! The inputs are read from files this computer was handed by a server, so
//! nothing here indexes past what it checked, and a file whose index points
//! outside it is refused rather than copied short.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::Error;

/// The output's movie timescale: milliseconds, which every player reads.
const MOVIE_TIMESCALE: u32 = 1000;
/// How much of a track goes in one chunk before the other track's turn.
const CHUNK_SECONDS: u64 = 1;
/// The most read at once while copying samples.
const COPY_BUFFER: usize = 1 << 20;

/// One sample: where its bytes are in its input, and when it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    pub(crate) offset: u64,
    pub(crate) size: u32,
    pub(crate) duration: u32,
    /// Composition offset: when it shows, after it is decoded.
    pub(crate) cts: i32,
    pub(crate) sync: bool,
}

/// One track read out of an input, with the boxes copied to the output.
#[derive(Debug, Default)]
pub(crate) struct Track {
    pub(crate) input: usize,
    pub(crate) id: u32,
    /// The input's `tkhd` from its layer field on: volume, matrix, size.
    pub(crate) tkhd_tail: Vec<u8>,
    pub(crate) timescale: u32,
    pub(crate) language: u16,
    pub(crate) hdlr: Vec<u8>,
    /// `vmhd` for a picture, `smhd` for sound.
    pub(crate) media_header: Vec<u8>,
    pub(crate) stsd: Vec<u8>,
    /// The input's edit list and its movie timescale: what an AAC track
    /// uses to skip its encoder's priming samples.
    pub(crate) edits: Vec<Edit>,
    pub(crate) edits_timescale: u32,
    pub(crate) samples: Vec<Sample>,
    /// When its first sample plays, in its timescale, where a fragmented
    /// file says (`tfdt`): a live stream's sound may start after its
    /// picture.
    pub(crate) start: Option<u64>,
    /// Which entry of `stsd` (from 1) its samples use, from the sample each run of them starts
    /// at; empty where every sample uses the first. A stream whose initialisation segment
    /// changes part-way has one entry for each (gaps row Y16).
    pub(crate) descriptions: Vec<(usize, u32)>,
}

impl Track {
    /// The entry of `stsd` (from 1) sample `index` uses.
    pub(crate) fn description_of(&self, index: usize) -> u32 {
        let after = self
            .descriptions
            .partition_point(|(first, _)| *first <= index);
        after
            .checked_sub(1)
            .and_then(|i| self.descriptions.get(i))
            .map_or(1, |(_, d)| *d)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Edit {
    pub(crate) duration: u64,
    pub(crate) media_time: i64,
}

/// A track's defaults from `trex`, for its fragments.
#[derive(Debug, Clone, Copy, Default)]
struct Defaults {
    description: u32,
    duration: u32,
    size: u32,
    flags: u32,
}

/// The tracks of the MP4 at `path`, as the muxer reads them (for the
/// system's video decoder, `frames`; macOS so far).
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub(crate) fn tracks_in_file(path: &Path) -> Result<Vec<Track>, Error> {
    tracks_of_input(path, 0)
}

/// The tracks of the MP4 at `path`, as input `input` of a join, refused
/// where its index points past its end (for the MKV writer, `mkvmux`).
pub(crate) fn tracks_of_input(path: &Path, input: usize) -> Result<Vec<Track>, Error> {
    let mut file =
        File::open(path).map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    let length = file
        .metadata()
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?
        .len();
    let tracks = read_input(&mut file, length, input)?;
    within(&tracks, length, path)?;
    Ok(tracks)
}

/// Refuses tracks whose index points past the end of the file they were
/// read from: every reader seeks and sizes its buffers by those numbers.
fn within(tracks: &[Track], length: u64, path: &Path) -> Result<(), Error> {
    let past = tracks.iter().flat_map(|t| &t.samples).any(|s| {
        s.offset
            .checked_add(u64::from(s.size))
            .is_none_or(|end| end > length)
    });
    if past {
        return Err(format!("{} is cut short", path.display()).into());
    }
    Ok(())
}

/// What a joined file says of itself, written into it (gaps row Y11): a video's title, its
/// channel or artist, its date and the page it came from. Empty words are left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    /// The title.
    pub title: String,
    /// The artist, or the channel that published it.
    pub artist: String,
    /// When it was published, as the site says it.
    pub date: String,
    /// Where it came from: its page's address.
    pub source: String,
    /// Its picture (the video's thumbnail), JPEG or PNG as the site served it: an MP4's
    /// `covr`, an MP3's `APIC`.
    pub cover: Vec<u8>,
    /// Its chapters, each where it starts (milliseconds) and its name, in order: an MP4's
    /// `chpl`, as players read Nero's.
    pub chapters: Vec<(u64, String)>,
}

/// A picture's kind by its first bytes, as a tag names it: JPEG, PNG or neither.
pub fn picture_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else {
        None
    }
}

/// The words an MP4 or M4A says of itself (`moov/udta/meta/ilst`), as [`join_cut_tagged`]
/// writes them; empty where it says none or is not one. Only its top-level boxes' headers and
/// its `moov` are read, not its media. A WebM's are its Info's title and Matroska tags
/// (`mkvmux::tags`; its cover and chapters are not read).
pub fn tags_of(path: &Path) -> Tags {
    if crate::mkvmux::is_webm(path) {
        return crate::mkvmux::tags(path);
    }
    fn moov_of(path: &Path) -> Option<Vec<u8>> {
        use std::io::{Read as _, Seek as _, SeekFrom};
        let mut file = File::open(path).ok()?;
        let length = file.metadata().ok()?.len();
        let mut at = 0u64;
        while at + 8 <= length {
            let mut head = [0u8; 16];
            file.seek(SeekFrom::Start(at)).ok()?;
            file.read_exact(&mut head[..8]).ok()?;
            let mut size = u64::from(u32::from_be_bytes(head[..4].try_into().ok()?));
            let mut header = 8;
            if size == 1 {
                file.read_exact(&mut head[8..16]).ok()?;
                size = u64::from_be_bytes(head[8..16].try_into().ok()?);
                header = 16;
            } else if size == 0 {
                size = length - at;
            }
            if size < header || at + size > length {
                return None;
            }
            if &head[4..8] == b"moov" {
                // A moov is an index: a few megabytes at most for a long film.
                if size > 64 << 20 {
                    return None;
                }
                let mut moov = vec![0u8; usize::try_from(size - header).ok()?];
                file.read_exact(&mut moov).ok()?;
                return Some(moov);
            }
            at += size;
        }
        None
    }
    let Some(moov) = moov_of(path) else {
        return Tags::default();
    };
    let udta = read::child(&moov, b"udta");
    let chapters = udta
        .and_then(|udta| read::child(udta, b"chpl"))
        .map(read_chapters)
        .unwrap_or_default();
    let ilst = udta
        .and_then(|udta| read::child(udta, b"meta"))
        .and_then(|meta| meta.get(4..))
        .and_then(|meta| read::child(meta, b"ilst"));
    let Some(ilst) = ilst else {
        return Tags {
            chapters,
            ..Tags::default()
        };
    };
    let data = |key: &[u8; 4]| {
        read::child(ilst, key)
            .and_then(|item| read::child(item, b"data"))
            .and_then(|data| data.get(8..))
    };
    let said = |key: &[u8; 4]| {
        data(key)
            .map(|text| String::from_utf8_lossy(text).into_owned())
            .unwrap_or_default()
    };
    Tags {
        title: said(b"\xa9nam"),
        artist: said(b"\xa9ART"),
        date: said(b"\xa9day"),
        source: said(b"\xa9cmt"),
        cover: data(b"covr").map(<[u8]>::to_vec).unwrap_or_default(),
        chapters,
    }
}

/// A `chpl`'s chapters (its body, after the box's header): version and flags, four bytes
/// kept, a count, then each start in 100 ns and its name, a byte long and the name. What
/// runs past the box is left out.
fn read_chapters(body: &[u8]) -> Vec<(u64, String)> {
    let mut chapters = Vec::new();
    let (Some(&count), Some(mut rest)) = (body.get(8), body.get(9..)) else {
        return chapters;
    };
    for _ in 0..count {
        let (Some(start), Some(&length)) = (rest.get(..8), rest.get(8)) else {
            break;
        };
        let Some(name) = rest.get(9..9 + usize::from(length)) else {
            break;
        };
        let start = u64::from_be_bytes(start.try_into().unwrap_or_default());
        chapters.push((start / 10_000, String::from_utf8_lossy(name).into_owned()));
        rest = rest.get(9 + usize::from(length)..).unwrap_or_default();
    }
    chapters
}

/// What a fragmented stream's initialisation piece says that a later one must
/// say alike for the pieces after both to be read as one track: each track's
/// id, timescale and sample description (codec and its set-up), as one key.
/// `None` where it has no index dm can read.
pub fn init_shape(data: &[u8]) -> Option<Vec<u8>> {
    let (_, moov, _) = boxes(data).find(|(kind, _, _)| kind == b"moov")?;
    let tracks = tracks_of(moov, 0).ok()?;
    let mut key = Vec::new();
    for track in tracks {
        key.extend_from_slice(&track.id.to_be_bytes());
        key.extend_from_slice(&track.timescale.to_be_bytes());
        key.extend_from_slice(&(track.stsd.len() as u64).to_be_bytes());
        key.extend_from_slice(&track.stsd);
    }
    (!key.is_empty()).then_some(key)
}

/// Joins `inputs`' tracks into one MP4 at `out`. Returns its length.
pub fn join(inputs: &[PathBuf], out: &Path) -> Result<u64, String> {
    join_with_text(inputs, &[], out)
}

/// [`join`], with `texts` carried as subtitle tracks, one per language.
pub fn join_with_text(
    inputs: &[PathBuf],
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
) -> Result<u64, String> {
    join_cut(inputs, texts, out, None, None)
}

/// [`join_with_text`], cut to a part: `cut` is where the part starts and
/// ends, in milliseconds of the inputs' own timeline (a fragmented
/// stream's pieces say when they play, `tfdt`). The output holds the
/// pieces given and plays the part alone, by each track's edit list.
pub fn join_cut(
    inputs: &[PathBuf],
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
    cut: Option<(u64, u64)>,
    shape: Option<crate::shape::Shape>,
) -> Result<u64, String> {
    join_cut_tagged(inputs, texts, out, cut, shape, &Tags::default())
}

/// [`join_cut`], with the file's own words about itself written into it.
pub fn join_cut_tagged(
    inputs: &[PathBuf],
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
    cut: Option<(u64, u64)>,
    shape: Option<crate::shape::Shape>,
    tags: &Tags,
) -> Result<u64, String> {
    let (tracks, files) = read_inputs(inputs, shape)?;
    write_with_text(tracks, files, texts, out, cut, None, tags).map_err(String::from)
}

/// [`join_cut`], with subtitles timed once the inputs are read (an HLS stream's WebVTT
/// segments, gaps row Y20): `place` is given when the output starts on the inputs' own
/// timeline, in 90 kHz ticks (its earliest track's first time, `tfdt`; 0 where none says), and
/// gives back the subtitles timed from there.
pub fn join_cut_placed<F>(
    inputs: &[PathBuf],
    place: F,
    out: &Path,
    cut: Option<(u64, u64)>,
    shape: Option<crate::shape::Shape>,
) -> Result<u64, String>
where
    F: FnOnce(i64) -> Vec<crate::subtitles::Subtitle>,
{
    let (tracks, files) = read_inputs(inputs, shape)?;
    let starts = || {
        tracks
            .iter()
            .filter_map(|t| t.start.map(|s| (s, t.timescale)))
    };
    // As the index places each track (`write::moov`): by its start in milliseconds.
    let earliest_ms = starts()
        .map(|(s, scale_of)| write::scale(s, scale_of, MOVIE_TIMESCALE))
        .min();
    let zero = starts()
        .map(|(s, scale_of)| write::scale(s, scale_of, 90_000))
        .min()
        .unwrap_or(0);
    let texts = place(i64::try_from(zero).unwrap_or(i64::MAX));
    write_with_text(
        tracks,
        files,
        &texts,
        out,
        cut,
        earliest_ms,
        &Tags::default(),
    )
    .map_err(String::from)
}

/// Every track of `inputs` that has samples, each input's file open, reshaped where `shape`
/// says.
fn read_inputs(
    inputs: &[PathBuf],
    shape: Option<crate::shape::Shape>,
) -> Result<(Vec<Track>, Vec<File>), Error> {
    let mut tracks = Vec::new();
    let mut files = Vec::new();
    for (index, path) in inputs.iter().enumerate() {
        let mut file =
            File::open(path).map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        let length = file
            .metadata()
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?
            .len();
        // A WebM picture or sound (VP9, AV1, Opus) goes into the MP4 as is,
        // the way players expect: a video's tallest picture can come only
        // as WebM.
        let read = if crate::mkvmux::is_webm(path) {
            crate::mkvmux::mp4_tracks(path, index)
        } else {
            read_input(&mut file, length, index)
        }
        .map_err(|e| format!("{} is not an MP4 dm can join: {e}", path.display()))?;
        within(&read, length, path)?;
        tracks.extend(read.into_iter().filter(|t| !t.samples.is_empty()));
        files.push(file);
    }
    if tracks.is_empty() {
        return Err("there is nothing to join".into());
    }
    // Vertical or square: the picture's crop, rewritten (`shape`).
    if let Some(shape) = shape {
        for track in &mut tracks {
            crate::shape::apply(track, shape)?;
        }
    }
    Ok((tracks, files))
}

/// `tracks`, read from `files`, written as one MP4 at `out` (cut where `cut` says, with
/// `tags`), and `texts` after them as subtitle tracks, one a language, each starting at
/// `start` (milliseconds on the tracks' own timeline, as `Track::start` places a track; `None`
/// for the output's own zero). Returns its length. What a transport stream's remux writes
/// through too (`tsmux`).
pub(crate) fn write_with_text(
    tracks: Vec<Track>,
    files: Vec<File>,
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
    cut: Option<(u64, u64)>,
    start: Option<u64>,
    tags: &Tags,
) -> Result<u64, Error> {
    let files = files
        .into_iter()
        .map(|file| Box::new(file) as Box<dyn Source>)
        .collect();
    write_with_text_from(tracks, files, texts, out, cut, start, tags)
}

/// What a track's samples are read from as the file is written: a file, or bytes made as they
/// are read, in order (a transport stream's samples made again from its segments, `tsmux`, gaps
/// row Y21).
pub(crate) trait Source: Read + Seek {}

impl<T: Read + Seek> Source for T {}

/// [`write_with_text`], each track's samples read from a [`Source`].
pub(crate) fn write_with_text_from(
    mut tracks: Vec<Track>,
    mut files: Vec<Box<dyn Source>>,
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
    cut: Option<(u64, u64)>,
    start: Option<u64>,
    tags: &Tags,
) -> Result<u64, Error> {
    if texts.is_empty() {
        return write::write_output_tagged(&tracks, files.as_mut_slice(), out, cut, tags);
    }
    // The subtitles' samples are written to a scratch file the writer copies
    // from, as it copies the inputs'.
    let mut scratch_name = out.as_os_str().to_owned();
    scratch_name.push(".text");
    let scratch = PathBuf::from(scratch_name);
    let written = (|| {
        let mut data = Vec::new();
        for text in texts {
            let mut track = text_track(text, files.len(), &mut data);
            track.start = start;
            tracks.push(track);
        }
        std::fs::write(&scratch, &data).map_err(|e| format!("subtitles: {e}"))?;
        files.push(Box::new(
            File::open(&scratch).map_err(|e| format!("subtitles: {e}"))?,
        ));
        write::write_output_tagged(&tracks, files.as_mut_slice(), out, cut, tags)
    })();
    let _ = std::fs::remove_file(&scratch);
    written
}

mod cenc;
mod in_place;
mod part;
mod read;
mod runs;
mod text;
mod write;

pub use cenc::clear_cbcs;
pub use in_place::{join_in_place, joining_mark, InPlace, NotInPlace};
pub use part::{plan_plain_part, write_plain_part, PlainPart};
use read::*;
pub(crate) use read::{boxes, child};
use text::*;
pub(crate) use write::*;

/// Small MP4 files made for tests (dm-core's tests too, through the `testing` feature).
#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod tests;
