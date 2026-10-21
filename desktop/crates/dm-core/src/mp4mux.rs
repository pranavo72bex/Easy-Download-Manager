//! Joins a video's picture and sound into one MP4, without ffmpeg.
//!
//! A video site serves its better qualities as two files, the picture alone
//! and the sound alone, each an MP4 — fragmented, as DASH serves them, or
//! plain. yt-dlp fetches both and leaves the joining to ffmpeg, which a
//! person would have to install (or dm fetch, 40–80 MB). This does the one
//! thing dm needs of it: each input's samples are read from its own index
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

/// The output's movie timescale: milliseconds, which every player reads.
const MOVIE_TIMESCALE: u32 = 1000;
/// How much of a track goes in one chunk before the other track's turn.
const CHUNK_SECONDS: u64 = 1;
/// The most read at once while copying samples.
const COPY_BUFFER: usize = 1 << 20;

/// One sample: where its bytes are in its input, and when it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sample {
    offset: u64,
    size: u32,
    duration: u32,
    /// Composition offset: when it shows, after it is decoded.
    cts: i32,
    sync: bool,
}

/// One track read out of an input, with the boxes copied to the output.
#[derive(Debug, Default)]
struct Track {
    input: usize,
    id: u32,
    /// The input's `tkhd` from its layer field on: volume, matrix, size.
    tkhd_tail: Vec<u8>,
    timescale: u32,
    language: u16,
    hdlr: Vec<u8>,
    /// `vmhd` for a picture, `smhd` for sound.
    media_header: Vec<u8>,
    stsd: Vec<u8>,
    /// The input's edit list and its movie timescale: what an AAC track
    /// uses to skip its encoder's priming samples.
    edits: Vec<Edit>,
    edits_timescale: u32,
    samples: Vec<Sample>,
}

#[derive(Debug, Clone, Copy)]
struct Edit {
    duration: u64,
    media_time: i64,
}

/// A track's defaults from `trex`, for its fragments.
#[derive(Debug, Clone, Copy, Default)]
struct Defaults {
    description: u32,
    duration: u32,
    size: u32,
    flags: u32,
}

/// Joins `inputs`' tracks into one MP4 at `out`. Returns its length.
pub fn join(inputs: &[PathBuf], out: &Path) -> Result<u64, String> {
    let mut tracks = Vec::new();
    let mut files = Vec::new();
    for (index, path) in inputs.iter().enumerate() {
        let mut file =
            File::open(path).map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        let length = file
            .metadata()
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?
            .len();
        let read = read_input(&mut file, length, index)
            .map_err(|e| format!("{} is not an MP4 dm can join: {e}", path.display()))?;
        for track in &read {
            if let Some(end) = track
                .samples
                .iter()
                .map(|s| s.offset + u64::from(s.size))
                .max()
            {
                if end > length {
                    return Err(format!("{} is cut short", path.display()));
                }
            }
        }
        tracks.extend(read.into_iter().filter(|t| !t.samples.is_empty()));
        files.push(file);
    }
    if tracks.is_empty() {
        return Err("there is nothing to join".into());
    }
    write_output(&tracks, &mut files, out)
}

/// [`join`], off the async runtime.
pub async fn join_async(inputs: Vec<PathBuf>, out: PathBuf) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join(&inputs, &out))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}

// ---- reading ------------------------------------------------------------

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// The boxes in `data`, in order: each one's type, its body, and the whole
/// box. Stops at the first that does not fit.
fn boxes(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8], &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let size = u32_at(data, at)?;
        let kind: [u8; 4] = data.get(at + 4..at + 8)?.try_into().ok()?;
        let (header, size) = match size {
            0 => (8, data.len().checked_sub(at)?),
            1 => (16, usize::try_from(u64_at(data, at + 8)?).ok()?),
            n => (8, n as usize),
        };
        if size < header {
            return None;
        }
        let whole = data.get(at..at.checked_add(size)?)?;
        let body = whole.get(header..)?;
        at += size;
        Some((kind, body, whole))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _, _)| k == kind).map(|(_, b, _)| b)
}

fn child_whole<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _, _)| k == kind).map(|(_, _, w)| w)
}

/// The top-level boxes of a file too big to read whole: `moov` and each
/// `moof` read into memory with where it starts, the rest skipped.
fn read_input(file: &mut File, length: u64, input: usize) -> Result<Vec<Track>, String> {
    let mut at = 0u64;
    let mut moov = None;
    let mut fragments: Vec<(u64, Vec<u8>)> = Vec::new();
    while at + 8 <= length {
        let mut header = [0u8; 16];
        file.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        file.read_exact(&mut header[..8])
            .map_err(|e| e.to_string())?;
        let small = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let kind = [header[4], header[5], header[6], header[7]];
        let (size, header_len) = match small {
            0 => (length - at, 8u64),
            1 => {
                file.read_exact(&mut header[8..16])
                    .map_err(|e| e.to_string())?;
                (
                    u64::from_be_bytes([
                        header[8], header[9], header[10], header[11], header[12], header[13],
                        header[14], header[15],
                    ]),
                    16,
                )
            }
            n => (n, 8),
        };
        if size < header_len || at + size > length {
            // A box running past the end: what is before it is all there is.
            break;
        }
        if &kind == b"moov" || &kind == b"moof" {
            if size > 512 << 20 {
                return Err("its index is too large".into());
            }
            let mut body = vec![0u8; (size - header_len) as usize];
            file.read_exact(&mut body).map_err(|e| e.to_string())?;
            if &kind == b"moov" {
                moov = Some(body);
            } else {
                fragments.push((at, body));
            }
        }
        at += size;
    }
    let moov = moov.ok_or("it has no index (moov)")?;
    let mut tracks = tracks_of(&moov, input)?;
    if !fragments.is_empty() {
        let defaults = trex_defaults(&moov);
        for (moof_at, moof) in &fragments {
            read_fragment(*moof_at, moof, &defaults, &mut tracks)?;
        }
    }
    Ok(tracks)
}

fn movie_timescale(moov: &[u8]) -> u32 {
    child(moov, b"mvhd")
        .and_then(|m| match m.first()? {
            1 => u32_at(m, 20),
            _ => u32_at(m, 12),
        })
        .filter(|t| *t > 0)
        .unwrap_or(MOVIE_TIMESCALE)
}

fn tracks_of(moov: &[u8], input: usize) -> Result<Vec<Track>, String> {
    let movie_scale = movie_timescale(moov);
    let mut tracks = Vec::new();
    for (kind, trak, _) in boxes(moov) {
        if &kind != b"trak" {
            continue;
        }
        let tkhd = child(trak, b"tkhd").ok_or("a track has no header")?;
        let (id, tail) = match tkhd.first() {
            Some(1) => (u32_at(tkhd, 20), tkhd.get(36..)),
            _ => (u32_at(tkhd, 12), tkhd.get(24..)),
        };
        let id = id.ok_or("a track header is short")?;
        let tail = tail
            .filter(|t| t.len() >= 60)
            .ok_or("a track header is short")?;
        let mdia = child(trak, b"mdia").ok_or("a track has no media")?;
        let mdhd = child(mdia, b"mdhd").ok_or("a track has no media header")?;
        let (timescale, language) = match mdhd.first() {
            Some(1) => (u32_at(mdhd, 20), u16_at(mdhd, 32)),
            _ => (u32_at(mdhd, 12), u16_at(mdhd, 20)),
        };
        let timescale = timescale
            .filter(|t| *t > 0)
            .ok_or("a track has no timescale")?;
        let hdlr = child_whole(mdia, b"hdlr").ok_or("a track has no handler")?;
        let minf = child(mdia, b"minf").ok_or("a track has no media information")?;
        let media_header = boxes(minf)
            .find(|(k, _, _)| k != b"dinf" && k != b"stbl")
            .map(|(_, _, w)| w.to_vec())
            .unwrap_or_default();
        let stbl = child(minf, b"stbl").ok_or("a track has no sample table")?;
        let stsd = child_whole(stbl, b"stsd").ok_or("a track has no sample description")?;
        let edits = child(trak, b"edts")
            .and_then(|e| child(e, b"elst"))
            .map(read_edits)
            .unwrap_or_default();
        tracks.push(Track {
            input,
            id,
            tkhd_tail: tail.get(..60).unwrap_or_default().to_vec(),
            timescale,
            language: language.unwrap_or(0x55c4), // "und"
            hdlr: hdlr.to_vec(),
            media_header,
            stsd: stsd.to_vec(),
            edits,
            edits_timescale: movie_scale,
            samples: plain_samples(stbl)?,
        });
    }
    Ok(tracks)
}

fn read_edits(elst: &[u8]) -> Vec<Edit> {
    let version = elst.first().copied().unwrap_or(0);
    let count = u32_at(elst, 4).unwrap_or(0) as usize;
    let width = if version == 1 { 20 } else { 12 };
    (0..count.min(16))
        .map_while(|i| {
            let at = 8 + i * width;
            if version == 1 {
                Some(Edit {
                    duration: u64_at(elst, at)?,
                    media_time: u64_at(elst, at + 8)? as i64,
                })
            } else {
                Some(Edit {
                    duration: u64::from(u32_at(elst, at)?),
                    media_time: i64::from(u32_at(elst, at + 4)? as i32),
                })
            }
        })
        .collect()
}

/// A full box's entries: `count` at 4, then `count` entries of `width`
/// bytes from 8, each read by `entry`. A count the box is too short for is
/// an error, not a short table.
fn entries<T>(
    full: &[u8],
    width: usize,
    entry: impl Fn(&[u8]) -> Option<T>,
) -> Result<Vec<T>, String> {
    let count = u32_at(full, 4).ok_or("a sample table is short")? as usize;
    if count.saturating_mul(width) > full.len().saturating_sub(8) {
        return Err("a sample table is short".into());
    }
    (0..count)
        .map(|i| {
            full.get(8 + i * width..8 + (i + 1) * width)
                .and_then(&entry)
                .ok_or_else(|| "a sample table is short".to_string())
        })
        .collect()
}

/// A plain MP4's samples, from its `stbl`. Empty for a fragmented one,
/// whose tables are empty and whose samples are in its fragments.
fn plain_samples(stbl: &[u8]) -> Result<Vec<Sample>, String> {
    let Some(stsz) = child(stbl, b"stsz") else {
        return Ok(Vec::new());
    };
    let fixed = u32_at(stsz, 4).ok_or("a sample table is short")?;
    let count = u32_at(stsz, 8).ok_or("a sample table is short")? as usize;
    let sizes: Vec<u32> = if fixed != 0 {
        vec![fixed; count]
    } else {
        entries(stsz.get(4..).unwrap_or_default(), 4, |e| u32_at(e, 0))?
    };
    if sizes.is_empty() {
        return Ok(Vec::new());
    }
    let offsets: Vec<u64> = if let Some(co64) = child(stbl, b"co64") {
        entries(co64, 8, |e| u64_at(e, 0))?
    } else {
        let stco = child(stbl, b"stco").ok_or("a track has no chunk offsets")?;
        entries(stco, 4, |e| u32_at(e, 0).map(u64::from))?
    };
    let stsc = child(stbl, b"stsc").ok_or("a track has no chunk map")?;
    let runs = entries(stsc, 12, |e| Some((u32_at(e, 0)?, u32_at(e, 4)?)))?;
    let stts = child(stbl, b"stts").ok_or("a track has no timing")?;
    let times = entries(stts, 8, |e| Some((u32_at(e, 0)?, u32_at(e, 4)?)))?;
    let shifts = match child(stbl, b"ctts") {
        Some(ctts) => entries(ctts, 8, |e| Some((u32_at(e, 0)?, u32_at(e, 4)? as i32)))?,
        None => Vec::new(),
    };
    let syncs = match child(stbl, b"stss") {
        Some(stss) => Some(entries(stss, 4, |e| u32_at(e, 0))?),
        None => None,
    };

    let mut samples = Vec::with_capacity(sizes.len());
    let mut size_iter = sizes.iter();
    'chunks: for (chunk, offset) in offsets.iter().enumerate() {
        let number = chunk as u32 + 1;
        let per_chunk = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= number)
            .map_or(0, |(_, n)| *n);
        let mut at = *offset;
        for _ in 0..per_chunk {
            let Some(size) = size_iter.next() else {
                break 'chunks;
            };
            samples.push(Sample {
                offset: at,
                size: *size,
                duration: 0,
                cts: 0,
                sync: syncs.is_none(),
            });
            at += u64::from(*size);
        }
    }
    let mut durations = times
        .iter()
        .flat_map(|(n, d)| std::iter::repeat_n(*d, *n as usize));
    let mut cts = shifts
        .iter()
        .flat_map(|(n, c)| std::iter::repeat_n(*c, *n as usize));
    for sample in &mut samples {
        sample.duration = durations.next().unwrap_or(0);
        sample.cts = cts.next().unwrap_or(0);
    }
    if let Some(syncs) = syncs {
        for number in syncs {
            if let Some(sample) = (number as usize)
                .checked_sub(1)
                .and_then(|i| samples.get_mut(i))
            {
                sample.sync = true;
            }
        }
    }
    Ok(samples)
}

fn trex_defaults(moov: &[u8]) -> Vec<(u32, Defaults)> {
    let Some(mvex) = child(moov, b"mvex") else {
        return Vec::new();
    };
    boxes(mvex)
        .filter(|(k, _, _)| k == b"trex")
        .filter_map(|(_, t, _)| {
            Some((
                u32_at(t, 4)?,
                Defaults {
                    description: u32_at(t, 8)?,
                    duration: u32_at(t, 12)?,
                    size: u32_at(t, 16)?,
                    flags: u32_at(t, 20)?,
                },
            ))
        })
        .collect()
}

/// A sample that others depend on to start decoding, by its flags.
fn is_sync(flags: u32) -> bool {
    flags & 0x0001_0000 == 0
}

fn read_fragment(
    moof_at: u64,
    moof: &[u8],
    defaults: &[(u32, Defaults)],
    tracks: &mut [Track],
) -> Result<(), String> {
    let short = || "a fragment is short".to_string();
    // Where a fragment's data is counted from when its header says nothing:
    // the moof for the first, then the end of the one before.
    let mut follows = moof_at;
    for (kind, traf, _) in boxes(moof) {
        if &kind != b"traf" {
            continue;
        }
        let tfhd = child(traf, b"tfhd").ok_or_else(short)?;
        let flags = u32_at(tfhd, 0).ok_or_else(short)? & 0x00ff_ffff;
        let id = u32_at(tfhd, 4).ok_or_else(short)?;
        let mut d = defaults
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, d)| *d)
            .unwrap_or_default();
        let mut at = 8;
        let mut base = if flags & 0x2_0000 != 0 {
            moof_at
        } else {
            follows
        };
        if flags & 0x1 != 0 {
            base = u64_at(tfhd, at).ok_or_else(short)?;
            at += 8;
        }
        if flags & 0x2 != 0 {
            d.description = u32_at(tfhd, at).ok_or_else(short)?;
            at += 4;
        }
        if flags & 0x8 != 0 {
            d.duration = u32_at(tfhd, at).ok_or_else(short)?;
            at += 4;
        }
        if flags & 0x10 != 0 {
            d.size = u32_at(tfhd, at).ok_or_else(short)?;
            at += 4;
        }
        if flags & 0x20 != 0 {
            d.flags = u32_at(tfhd, at).ok_or_else(short)?;
        }
        let Some(track) = tracks.iter_mut().find(|t| t.id == id) else {
            continue;
        };
        let mut next = base;
        for (kind, trun, _) in boxes(traf) {
            if &kind != b"trun" {
                continue;
            }
            let version = trun.first().copied().unwrap_or(0);
            let flags = u32_at(trun, 0).ok_or_else(short)? & 0x00ff_ffff;
            let count = u32_at(trun, 4).ok_or_else(short)? as usize;
            let mut at = 8;
            if flags & 0x1 != 0 {
                let offset = u32_at(trun, at).ok_or_else(short)? as i32;
                next = base
                    .checked_add_signed(i64::from(offset))
                    .ok_or_else(short)?;
                at += 4;
            }
            let mut first_flags = None;
            if flags & 0x4 != 0 {
                first_flags = Some(u32_at(trun, at).ok_or_else(short)?);
                at += 4;
            }
            let width = [0x100, 0x200, 0x400, 0x800]
                .iter()
                .filter(|f| flags & **f != 0)
                .count()
                * 4;
            if count.saturating_mul(width) > trun.len().saturating_sub(at) {
                return Err(short());
            }
            track.samples.reserve(count);
            for i in 0..count {
                let mut field = at + i * width;
                let mut take = |present: bool, default: u32| -> Option<u32> {
                    if !present {
                        return Some(default);
                    }
                    let v = u32_at(trun, field)?;
                    field += 4;
                    Some(v)
                };
                let duration = take(flags & 0x100 != 0, d.duration).ok_or_else(short)?;
                let size = take(flags & 0x200 != 0, d.size).ok_or_else(short)?;
                let sample_flags = take(flags & 0x400 != 0, d.flags).ok_or_else(short)?;
                let cts = take(flags & 0x800 != 0, 0).ok_or_else(short)?;
                let sample_flags = match (i, first_flags) {
                    (0, Some(f)) if flags & 0x400 == 0 => f,
                    _ => sample_flags,
                };
                track.samples.push(Sample {
                    offset: next,
                    size,
                    duration,
                    // Version 0 says unsigned; every writer means signed.
                    cts: if version == 0 && cts > i32::MAX as u32 {
                        0
                    } else {
                        cts as i32
                    },
                    sync: is_sync(sample_flags),
                });
                next += u64::from(size);
            }
        }
        follows = next;
    }
    Ok(())
}

// ---- writing ------------------------------------------------------------

fn boxed(kind: &[u8; 4], parts: &[&[u8]]) -> Vec<u8> {
    let len: usize = parts.iter().map(|p| p.len()).sum::<usize>() + 8;
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&(len as u32).to_be_bytes());
    out.extend_from_slice(kind);
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let head = ((u32::from(version) << 24) | (flags & 0x00ff_ffff)).to_be_bytes();
    boxed(kind, &[&head, body])
}

const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

fn scale(value: u64, from: u32, to: u32) -> u64 {
    if from == 0 {
        return 0;
    }
    (u128::from(value) * u128::from(to) / u128::from(from)) as u64
}

/// A run of one track's samples written together.
struct Chunk {
    track: usize,
    first: usize,
    count: usize,
    /// When it starts, in seconds scaled to nanoseconds, for interleaving.
    start: u128,
}

fn chunks(tracks: &[Track]) -> Vec<Chunk> {
    let mut all = Vec::new();
    for (t, track) in tracks.iter().enumerate() {
        let span = u64::from(track.timescale) * CHUNK_SECONDS;
        let mut first = 0;
        let mut time = 0u64;
        let mut chunk_time = 0u64;
        for (i, sample) in track.samples.iter().enumerate() {
            if i > first && time - chunk_time >= span {
                all.push(Chunk {
                    track: t,
                    first,
                    count: i - first,
                    start: u128::from(chunk_time) * 1_000_000_000 / u128::from(track.timescale),
                });
                first = i;
                chunk_time = time;
            }
            time += u64::from(sample.duration);
        }
        all.push(Chunk {
            track: t,
            first,
            count: track.samples.len() - first,
            start: u128::from(chunk_time) * 1_000_000_000 / u128::from(track.timescale),
        });
    }
    all.sort_by_key(|c| (c.start, c.track));
    all
}

fn media_duration(track: &Track) -> u64 {
    track.samples.iter().map(|s| u64::from(s.duration)).sum()
}

/// When a track's first frame is shown and when its last one ends, in its
/// own timescale. The first is later than zero for a picture whose frames
/// are decoded ahead of being shown (B-frames), and the last then ends as
/// much after the decoding does.
fn shown(track: &Track) -> (i64, i64) {
    let mut decoded = 0i64;
    let (mut first, mut last) = (i64::MAX, 0i64);
    for sample in &track.samples {
        let at = decoded + i64::from(sample.cts);
        first = first.min(at);
        last = last.max(at + i64::from(sample.duration));
        decoded += i64::from(sample.duration);
    }
    if first == i64::MAX {
        (0, 0)
    } else {
        (first, last)
    }
}

/// The edit list written for a track: an empty edit kept as it is; a
/// track's own start (an AAC encoder's priming) kept, with its length
/// worked out where a fragmented file left it at zero. A DASH picture has
/// none and its first frame shown a frame or two in, which played that
/// much behind its sound: it is given one that starts it at that frame, as
/// a joined file from any other tool would have.
fn edit_list(track: &Track) -> Option<Vec<u8>> {
    let (first, last) = shown(track);
    let made;
    let edits = if track.edits.is_empty() {
        if first <= 0 {
            return None;
        }
        made = [Edit {
            duration: 0,
            media_time: first,
        }];
        &made[..]
    } else {
        &track.edits[..]
    };
    let mut body = Vec::new();
    for edit in edits {
        let duration = if edit.media_time < 0 {
            scale(edit.duration, track.edits_timescale, MOVIE_TIMESCALE)
        } else if edit.duration == 0 {
            scale(
                u64::try_from(last.saturating_sub(edit.media_time)).unwrap_or(0),
                track.timescale,
                MOVIE_TIMESCALE,
            )
        } else {
            scale(edit.duration, track.edits_timescale, MOVIE_TIMESCALE)
        };
        body.extend_from_slice(&duration.to_be_bytes());
        body.extend_from_slice(&edit.media_time.to_be_bytes());
        body.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    }
    let mut elst = (edits.len() as u32).to_be_bytes().to_vec();
    elst.extend_from_slice(&body);
    Some(boxed(b"edts", &[&full(b"elst", 1, 0, &elst)]))
}

fn sample_table(track: &Track, offsets: &[u64], per_chunk: &[u32]) -> Vec<u8> {
    let samples = &track.samples;
    let run_length = |values: &mut dyn Iterator<Item = u32>| -> Vec<(u32, u32)> {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for v in values {
            match runs.last_mut() {
                Some((n, last)) if *last == v => *n += 1,
                _ => runs.push((1, v)),
            }
        }
        runs
    };
    let table = |runs: &[(u32, u32)]| {
        let mut body = (runs.len() as u32).to_be_bytes().to_vec();
        for (n, v) in runs {
            body.extend_from_slice(&n.to_be_bytes());
            body.extend_from_slice(&v.to_be_bytes());
        }
        body
    };
    let stts = full(
        b"stts",
        0,
        0,
        &table(&run_length(&mut samples.iter().map(|s| s.duration))),
    );
    let ctts = samples.iter().any(|s| s.cts != 0).then(|| {
        full(
            b"ctts",
            1,
            0,
            &table(&run_length(&mut samples.iter().map(|s| s.cts as u32))),
        )
    });
    let stss = samples.iter().any(|s| !s.sync).then(|| {
        let numbers: Vec<u32> = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.sync)
            .map(|(i, _)| i as u32 + 1)
            .collect();
        let mut body = (numbers.len() as u32).to_be_bytes().to_vec();
        for n in numbers {
            body.extend_from_slice(&n.to_be_bytes());
        }
        full(b"stss", 0, 0, &body)
    });
    let mut stsz = 0u32.to_be_bytes().to_vec();
    stsz.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for s in samples {
        stsz.extend_from_slice(&s.size.to_be_bytes());
    }
    let mut stsc_runs: Vec<(u32, u32)> = Vec::new();
    for (i, n) in per_chunk.iter().enumerate() {
        if stsc_runs.last().map(|(_, last)| last) != Some(n) {
            stsc_runs.push((i as u32 + 1, *n));
        }
    }
    let mut stsc = (stsc_runs.len() as u32).to_be_bytes().to_vec();
    for (first, n) in stsc_runs {
        stsc.extend_from_slice(&first.to_be_bytes());
        stsc.extend_from_slice(&n.to_be_bytes());
        stsc.extend_from_slice(&1u32.to_be_bytes());
    }
    let mut co64 = (offsets.len() as u32).to_be_bytes().to_vec();
    for o in offsets {
        co64.extend_from_slice(&o.to_be_bytes());
    }
    let mut parts: Vec<Vec<u8>> = vec![track.stsd.clone(), stts];
    parts.extend(ctts);
    parts.extend(stss);
    parts.push(full(b"stsz", 0, 0, &stsz));
    parts.push(full(b"stsc", 0, 0, &stsc));
    parts.push(full(b"co64", 0, 0, &co64));
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    boxed(b"stbl", &refs)
}

fn trak(track: &Track, id: u32, offsets: &[u64], per_chunk: &[u32]) -> Vec<u8> {
    let media = media_duration(track);
    let movie = scale(media, track.timescale, MOVIE_TIMESCALE);
    let mut tkhd = Vec::new();
    tkhd.extend_from_slice(&0u64.to_be_bytes());
    tkhd.extend_from_slice(&0u64.to_be_bytes());
    tkhd.extend_from_slice(&id.to_be_bytes());
    tkhd.extend_from_slice(&0u32.to_be_bytes());
    tkhd.extend_from_slice(&movie.to_be_bytes());
    tkhd.extend_from_slice(&track.tkhd_tail);
    let mut mdhd = Vec::new();
    mdhd.extend_from_slice(&0u64.to_be_bytes());
    mdhd.extend_from_slice(&0u64.to_be_bytes());
    mdhd.extend_from_slice(&track.timescale.to_be_bytes());
    mdhd.extend_from_slice(&media.to_be_bytes());
    mdhd.extend_from_slice(&track.language.to_be_bytes());
    mdhd.extend_from_slice(&0u16.to_be_bytes());
    let url = full(b"url ", 0, 1, &[]);
    let dref = full(b"dref", 0, 0, &[&1u32.to_be_bytes()[..], &url].concat());
    let dinf = boxed(b"dinf", &[&dref]);
    let minf = boxed(
        b"minf",
        &[
            &track.media_header,
            &dinf,
            &sample_table(track, offsets, per_chunk),
        ],
    );
    let mdia = boxed(b"mdia", &[&full(b"mdhd", 1, 0, &mdhd), &track.hdlr, &minf]);
    let edts = edit_list(track).unwrap_or_default();
    boxed(b"trak", &[&full(b"tkhd", 1, 3, &tkhd), &edts, &mdia])
}

fn moov(tracks: &[Track], layout: &[Chunk], data_at: u64) -> Vec<u8> {
    let mut offsets: Vec<Vec<u64>> = vec![Vec::new(); tracks.len()];
    let mut per_chunk: Vec<Vec<u32>> = vec![Vec::new(); tracks.len()];
    let mut at = data_at;
    for chunk in layout {
        let (Some(o), Some(p), Some(track)) = (
            offsets.get_mut(chunk.track),
            per_chunk.get_mut(chunk.track),
            tracks.get(chunk.track),
        ) else {
            continue;
        };
        o.push(at);
        p.push(chunk.count as u32);
        at += track
            .samples
            .iter()
            .skip(chunk.first)
            .take(chunk.count)
            .map(|s| u64::from(s.size))
            .sum::<u64>();
    }
    let duration = tracks
        .iter()
        .map(|t| scale(media_duration(t), t.timescale, MOVIE_TIMESCALE))
        .max()
        .unwrap_or(0);
    let mut mvhd = Vec::new();
    mvhd.extend_from_slice(&0u64.to_be_bytes());
    mvhd.extend_from_slice(&0u64.to_be_bytes());
    mvhd.extend_from_slice(&MOVIE_TIMESCALE.to_be_bytes());
    mvhd.extend_from_slice(&duration.to_be_bytes());
    mvhd.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    mvhd.extend_from_slice(&0x0100u16.to_be_bytes());
    mvhd.extend_from_slice(&[0u8; 10]);
    for m in MATRIX {
        mvhd.extend_from_slice(&m.to_be_bytes());
    }
    mvhd.extend_from_slice(&[0u8; 24]);
    mvhd.extend_from_slice(&(tracks.len() as u32 + 1).to_be_bytes());
    let mut parts = vec![full(b"mvhd", 1, 0, &mvhd)];
    for (i, track) in tracks.iter().enumerate() {
        parts.push(trak(
            track,
            i as u32 + 1,
            offsets.get(i).map(Vec::as_slice).unwrap_or_default(),
            per_chunk.get(i).map(Vec::as_slice).unwrap_or_default(),
        ));
    }
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    boxed(b"moov", &refs)
}

fn write_output(tracks: &[Track], files: &mut [File], out: &Path) -> Result<u64, String> {
    let ftyp = boxed(
        b"ftyp",
        &[b"isom", &0x200u32.to_be_bytes(), b"isomiso2avc1mp41"],
    );
    let layout = chunks(tracks);
    // The index's length does not depend on the offsets in it (all 64-bit),
    // so it is laid out once to measure and once for real.
    let measured = moov(tracks, &layout, 0).len() as u64;
    let data_at = ftyp.len() as u64 + measured + 16;
    let index = moov(tracks, &layout, data_at);
    let data: u64 = tracks
        .iter()
        .flat_map(|t| t.samples.iter())
        .map(|s| u64::from(s.size))
        .sum();

    let wrote = |e: std::io::Error| format!("{} could not be written: {e}", out.display());
    let mut writer = BufWriter::with_capacity(COPY_BUFFER, File::create(out).map_err(wrote)?);
    writer.write_all(&ftyp).map_err(wrote)?;
    writer.write_all(&index).map_err(wrote)?;
    writer.write_all(&1u32.to_be_bytes()).map_err(wrote)?;
    writer.write_all(b"mdat").map_err(wrote)?;
    writer
        .write_all(&(data + 16).to_be_bytes())
        .map_err(wrote)?;

    let mut buffer = vec![0u8; COPY_BUFFER];
    for chunk in &layout {
        let Some(track) = tracks.get(chunk.track) else {
            continue;
        };
        let file = files.get_mut(track.input).ok_or("an input went missing")?;
        let samples = track
            .samples
            .get(chunk.first..chunk.first + chunk.count)
            .unwrap_or_default();
        // Samples next to each other in the input are read together.
        let mut i = 0;
        while let Some(first) = samples.get(i) {
            let mut end = first.offset + u64::from(first.size);
            i += 1;
            while let Some(s) = samples.get(i).filter(|s| s.offset == end) {
                end += u64::from(s.size);
                i += 1;
            }
            file.seek(SeekFrom::Start(first.offset))
                .map_err(|e| e.to_string())?;
            let mut left = end - first.offset;
            while left > 0 {
                let take = left.min(buffer.len() as u64) as usize;
                let part = buffer.get_mut(..take).ok_or("a read went wrong")?;
                file.read_exact(part)
                    .map_err(|e| format!("an input could not be read: {e}"))?;
                writer.write_all(part).map_err(wrote)?;
                left -= take as u64;
            }
        }
    }
    writer.flush().map_err(wrote)?;
    let file = writer.into_inner().map_err(|e| wrote(e.into_error()))?;
    file.sync_all().map_err(wrote)?;
    Ok(ftyp.len() as u64 + index.len() as u64 + 16 + data)
}

#[cfg(test)]
mod tests {
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
    fn fragmented(dir: &Path, name: &str, fragments: u32) -> (PathBuf, Vec<Vec<u8>>) {
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
                let mut trun = 3u32.to_be_bytes().to_vec();
                trun.extend_from_slice(&offset.to_be_bytes());
                trun.extend_from_slice(&trun_entries);
                let traf = boxed(b"traf", &[&tfhd, &full(b"trun", 0, 0x701, &trun)]);
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
    fn plain(dir: &Path, name: &str, count: u32, byte: u8) -> (PathBuf, Vec<Vec<u8>>) {
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

    fn read_back(path: &Path) -> Vec<Track> {
        let mut file = File::open(path).unwrap();
        let length = file.metadata().unwrap().len();
        read_input(&mut file, length, 0).unwrap()
    }

    fn bytes_of(path: &Path, track: &Track) -> Vec<Vec<u8>> {
        let data = std::fs::read(path).unwrap();
        track
            .samples
            .iter()
            .map(|s| data[s.offset as usize..(s.offset + u64::from(s.size)) as usize].to_vec())
            .collect()
    }

    #[test]
    fn a_fragmented_picture_and_a_plain_sound_join_sample_for_sample() {
        let dir = tempfile::tempdir().unwrap();
        let (video, pictures) = fragmented(dir.path(), "v.mp4", 4);
        let (audio, sounds) = plain(dir.path(), "a.m4a", 7, 200);
        let out = dir.path().join("joined.mp4");
        let length = join(&[video, audio], &out).unwrap();
        assert_eq!(length, std::fs::metadata(&out).unwrap().len());

        let data = std::fs::read(&out).unwrap();
        let kinds: Vec<[u8; 4]> = boxes(&data).map(|(k, _, _)| k).collect();
        assert_eq!(
            kinds,
            [*b"ftyp", *b"moov", *b"mdat"],
            "the index comes first"
        );

        let tracks = read_back(&out);
        assert_eq!(tracks.len(), 2);
        assert_eq!((tracks[0].id, tracks[1].id), (1, 2));
        assert_eq!(tracks[0].timescale, 90000);
        assert_eq!(tracks[1].timescale, 48000);
        assert_eq!(bytes_of(&out, &tracks[0]), pictures);
        assert_eq!(bytes_of(&out, &tracks[1]), sounds);
        // Timing and the sync samples survive: every second picture of a
        // fragment of three is not one.
        assert!(tracks[0].samples.iter().all(|s| s.duration == 3000));
        let syncs: Vec<bool> = tracks[0].samples.iter().map(|s| s.sync).collect();
        assert_eq!(syncs, [true, false, true].repeat(4));
        assert!(tracks[1]
            .samples
            .iter()
            .all(|s| s.duration == 1024 && s.sync));
    }

    #[test]
    fn an_index_pointing_past_the_end_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (audio, _) = plain(dir.path(), "a.m4a", 4, 1);
        let data = std::fs::read(&audio).unwrap();
        std::fs::write(&audio, &data[..data.len() - 5]).unwrap();
        let err = join(&[audio], &dir.path().join("out.mp4")).unwrap_err();
        assert!(err.contains("cut short"), "{err}");
    }

    #[test]
    fn what_is_not_an_mp4_is_refused_not_panicked_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("page.html");
        std::fs::write(&path, b"<!doctype html><title>no</title>").unwrap();
        assert!(join(&[path], &dir.path().join("out.mp4")).is_err());
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..600)) {
            let _ = boxes(&data).count();
            let _ = tracks_of(&data, 0);
            let _ = plain_samples(&data);
            let mut tracks = vec![Track { id: 1, timescale: 1, ..Track::default() }];
            let _ = read_fragment(0, &data, &[], &mut tracks);
        }
    }
}
