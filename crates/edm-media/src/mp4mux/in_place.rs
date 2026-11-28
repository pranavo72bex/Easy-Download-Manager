// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A join written into the largest of its inputs, for a disk without room for the joined file
//! beside them (gaps row Y21).
//!
//! The usual join writes a new file while its inputs are all still there: at its end the disk
//! holds the video twice. Here the largest input, an MP4 (fragmented or plain), becomes the
//! joined file, its samples left where they lie. Each other input's samples are copied onto its
//! end, a chunk at a time, and where that input is one track whose samples lie in order, from its
//! last chunk back, the input shortened past each chunk once it is copied; a new index (`moov`)
//! follows them, and then the host's own boxes but its media (its index, `sidx`, each `moof`)
//! are made `free` boxes and its brands the usual join's. The disk holds the file and little more
//! throughout: a chunk and the index. What it gives up: the index is last, so the file does not
//! play while it is still being read (from the disk it plays as any other), and its sound is not
//! interleaved with its picture. A join cut short leaves inputs that are neither the inputs nor a
//! join: each is marked ([`joining_mark`]) before it is first changed, to be fetched again.

use super::text::text_track;
use super::write::{chunks, copy_chunks, moov_tables, scale, Chunk};
use super::*;

/// What a join in place is given.
pub struct InPlace {
    /// The inputs, in the order their tracks are written, as `join` takes them.
    pub inputs: Vec<PathBuf>,
    /// Where the part starts and ends, as `join_cut` takes it.
    pub cut: Option<(u64, u64)>,
    /// Vertical or square, as `join_cut` takes it.
    pub shape: Option<crate::shape::Shape>,
    /// The file's own words about itself.
    pub tags: Tags,
    /// Whether the subtitles start where the tracks do (an HLS stream's, as `join_cut_placed`
    /// times them); else at the file's own zero.
    pub placed: bool,
    /// Told before each input is shortened and once the file is whole: what a test measures the
    /// disk by.
    pub watch: Option<Box<dyn FnMut() + Send>>,
}

/// Why a join in place was not made.
#[derive(Debug)]
pub enum NotInPlace {
    /// Nothing was changed: no input can be joined into, or the disk has no room even for what
    /// the join adds.
    Untouched(String),
    /// Inputs were changed and the join not finished: each marked one is to be fetched again.
    Spoiled(String),
}

/// The mark made beside an input before a join in place first changes it: where it is found, the
/// input is not what was fetched.
pub fn joining_mark(input: &Path) -> PathBuf {
    let mut name = input.as_os_str().to_owned();
    name.push(".edm-joining");
    PathBuf::from(name)
}

/// One chunk copied onto the host: its track, its first sample, how many, and where it lands.
#[derive(Debug, Clone, Copy)]
struct Landed {
    track: usize,
    first: usize,
    count: usize,
    at: u64,
}

/// The host's top-level boxes as the join changes them.
struct Host {
    /// Where each box made `free` starts.
    freed: Vec<u64>,
    /// Where its `ftyp`'s brands are, and how many of their bytes are written again.
    brands: Option<(u64, u64)>,
}

/// What [`write_into`] writes.
struct Made<'a> {
    host: &'a Path,
    /// The host's length before the join.
    length: u64,
    /// The bytes of the samples copied onto it.
    data: u64,
    index: &'a [u8],
    boxes: &'a Host,
    inputs: &'a [PathBuf],
    /// By input: shortened as its chunks are copied.
    shortened: &'a [bool],
}

/// `job.inputs` joined into the largest of them that is an MP4, as `join_cut_tagged` joins them
/// into a new file, with the subtitles `place` gives (told when the file starts on the inputs'
/// own timeline, in 90 kHz ticks, as `join_cut_placed` tells it). Gives back which input that
/// was and the file's length. `room` is asked, with that input and the most the disk must hold
/// beyond what it holds now, whether there is room: where it says not, nothing is changed. The
/// other inputs are left, shortened, for the caller to remove with every input's mark.
pub fn join_in_place<F, R>(mut job: InPlace, place: F, room: R) -> Result<(usize, u64), NotInPlace>
where
    F: FnOnce(i64) -> Vec<crate::subtitles::Subtitle>,
    R: FnOnce(&Path, u64) -> bool,
{
    let untouched = |e: Error| NotInPlace::Untouched(e.into());
    let host = largest_mp4(&job.inputs)
        .ok_or_else(|| untouched("none of its inputs is an MP4 to join into".into()))?;
    let host_path = job
        .inputs
        .get(host)
        .cloned()
        .ok_or_else(|| untouched("an input went missing".into()))?;
    let (mut tracks, files) = read_inputs(&job.inputs, job.shape).map_err(untouched)?;
    let length = std::fs::metadata(&host_path)
        .map_err(|e| untouched(Error::from(e.to_string())))?
        .len();
    let boxes = {
        let mut file = File::open(&host_path).map_err(|e| untouched(Error::from(e.to_string())))?;
        host_boxes(&mut file, length).map_err(untouched)?
    };
    if crosses(&tracks, host, &boxes) {
        return Err(untouched("its samples lie across its boxes".into()));
    }
    // The subtitles, timed as the usual join times them.
    let starts = || {
        tracks
            .iter()
            .filter_map(|t| t.start.map(|s| (s, t.timescale)))
    };
    let earliest_ms = starts().map(|(s, of)| scale(s, of, MOVIE_TIMESCALE)).min();
    let zero = starts()
        .map(|(s, of)| scale(s, of, 90_000))
        .min()
        .unwrap_or(0);
    let texts = place(i64::try_from(zero).unwrap_or(i64::MAX));
    let mut files: Vec<Box<dyn Source>> = files
        .into_iter()
        .map(|file| Box::new(file) as Box<dyn Source>)
        .collect();
    if !texts.is_empty() {
        let mut data = Vec::new();
        for text in &texts {
            let mut track = text_track(text, files.len(), &mut data);
            track.start = if job.placed { earliest_ms } else { None };
            tracks.push(track);
        }
        files.push(Box::new(std::io::Cursor::new(data)));
    }
    let shortened = shortened_inputs(&tracks, job.inputs.len(), host);
    let landed = land(&tracks, &copy_order(&tracks, host, &shortened), length + 16);
    let (offsets, per_chunk) = tables(&tracks, host, &landed);
    let index = moov_tables(&tracks, &offsets, &per_chunk, job.cut, &job.tags);
    let bytes = |l: &Landed| bytes_of(&tracks, l);
    let let_go = |l: &Landed| {
        tracks
            .get(l.track)
            .is_some_and(|t| shortened.get(t.input).copied().unwrap_or(false))
    };
    let data: u64 = landed.iter().map(bytes).sum();
    // The most held beyond what is held now: the index, what is copied from inputs not shortened
    // (and the subtitles), and a chunk of a shortened one before it is let go.
    let kept: u64 = landed.iter().filter(|l| !let_go(l)).map(bytes).sum();
    let largest = landed.iter().filter(|l| let_go(l)).map(bytes).max();
    let need = 16 + index.len() as u64 + kept + largest.unwrap_or(0);
    if !room(host_path.as_path(), need) {
        return Err(untouched(
            format!("the disk has no room for the {need} bytes it adds").into(),
        ));
    }
    // Marked before anything is changed.
    for (n, input) in job.inputs.iter().enumerate() {
        if n == host || shortened.get(n).copied().unwrap_or(false) {
            std::fs::write(joining_mark(input), b"")
                .map_err(|e| untouched(Error::from(e.to_string())))?;
        }
    }
    let made = Made {
        host: &host_path,
        length,
        data,
        index: &index,
        boxes: &boxes,
        inputs: &job.inputs,
        shortened: &shortened,
    };
    write_into(&made, &tracks, &landed, &mut files, &mut job.watch)
        .map_err(|e| NotInPlace::Spoiled(e.into()))?;
    Ok((host, length + 16 + data + index.len() as u64))
}

/// The largest of `inputs` that is an MP4 (a WebM is not joined into): the first of equals.
fn largest_mp4(inputs: &[PathBuf]) -> Option<usize> {
    inputs
        .iter()
        .enumerate()
        .filter(|(_, path)| !crate::mkvmux::is_webm(path))
        .filter_map(|(n, path)| Some((std::fs::metadata(path).ok()?.len(), n)))
        .max_by_key(|(length, n)| (*length, std::cmp::Reverse(*n)))
        .map(|(_, n)| n)
}

/// The most of an `ftyp`'s brands written again; any after are left as they are.
const MOST_BRANDS: u64 = 64;

/// The host's top-level boxes: which are made `free` (all but its media, `ftyp` and padding)
/// and where its brands are. Refused where the boxes do not run, each whole, to the file's end:
/// what is put after them would be read as part of the last.
fn host_boxes(file: &mut File, length: u64) -> Result<Host, Error> {
    let mut host = Host {
        freed: Vec::new(),
        brands: None,
    };
    let mut at = 0u64;
    while at < length {
        if length - at < 8 {
            return Err("its last box is cut short".into());
        }
        let mut header = [0u8; 16];
        file.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        file.read_exact(&mut header[..8])
            .map_err(|e| e.to_string())?;
        let small = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let kind = [header[4], header[5], header[6], header[7]];
        let (size, header_len) = match small {
            0 => return Err("its last box runs to the end with no size".into()),
            1 => {
                file.read_exact(&mut header[8..16])
                    .map_err(|e| e.to_string())?;
                (
                    u64::from_be_bytes([
                        header[8], header[9], header[10], header[11], header[12], header[13],
                        header[14], header[15],
                    ]),
                    16u64,
                )
            }
            n => (u64::from(n), 8),
        };
        let end = at
            .checked_add(size)
            .filter(|end| size >= header_len && *end <= length)
            .ok_or("a box runs past the end of the file")?;
        match &kind {
            b"mdat" | b"free" | b"skip" => {}
            b"ftyp" => host.brands = Some((at + header_len, (size - header_len).min(MOST_BRANDS))),
            _ => host.freed.push(at),
        }
        at = end;
    }
    Ok(host)
}

/// Whether any of the host's samples lies across a byte the join writes again: a box's type
/// made `free`, or the brands.
fn crosses(tracks: &[Track], host: usize, boxes: &Host) -> bool {
    let mut written: Vec<(u64, u64)> = boxes.freed.iter().map(|at| (at + 4, at + 8)).collect();
    written.extend(boxes.brands.map(|(at, length)| (at, at + length)));
    written.sort_unstable();
    tracks
        .iter()
        .filter(|t| t.input == host)
        .flat_map(|t| &t.samples)
        .any(|s| {
            let end = s.offset.saturating_add(u64::from(s.size));
            let after = written.partition_point(|(_, stop)| *stop <= s.offset);
            written.get(after).is_some_and(|(start, _)| *start < end)
        })
}

/// An `ftyp`'s first `length` bytes of brands as the usual join writes them: `isom`, version
/// 0x200, then `isom`, `iso2`, `avc1` and `mp41` over and over.
fn brands(length: u64) -> Vec<u8> {
    let mut body = b"isom".to_vec();
    body.extend_from_slice(&0x200u32.to_be_bytes());
    let mut ours = [b"isom", b"iso2", b"avc1", b"mp41"].into_iter().cycle();
    while (body.len() as u64) < length {
        if let Some(brand) = ours.next() {
            body.extend_from_slice(brand);
        }
    }
    body.truncate(length as usize);
    body
}

/// By input, whether it is shortened as its chunks are copied: an input but the host whose one
/// track's samples lie one after another in it, so that cutting it where a chunk starts leaves
/// every sample before whole.
fn shortened_inputs(tracks: &[Track], inputs: usize, host: usize) -> Vec<bool> {
    (0..inputs)
        .map(|input| {
            let mut own = tracks.iter().filter(|t| t.input == input);
            match (own.next(), own.next()) {
                (Some(track), None) if input != host => track.samples.windows(2).all(|pair| {
                    matches!(pair, [a, b] if a.offset.saturating_add(u64::from(a.size)) <= b.offset)
                }),
                _ => false,
            }
        })
        .collect()
}

/// The chunks copied onto the host, in the order they are copied: each other input's in turn,
/// in playing order, a shortened one's from its last.
fn copy_order(tracks: &[Track], host: usize, shortened: &[bool]) -> Vec<Chunk> {
    let input_of = |c: &Chunk| tracks.get(c.track).map_or(host, |t| t.input);
    let mut left: Vec<Chunk> = chunks(tracks)
        .into_iter()
        .filter(|c| input_of(c) != host)
        .collect();
    let mut order = Vec::with_capacity(left.len());
    while let Some(input) = left.first().map(input_of) {
        let (mut own, rest): (Vec<Chunk>, Vec<Chunk>) =
            left.into_iter().partition(|c| input_of(c) == input);
        if shortened.get(input).copied().unwrap_or(false) {
            own.reverse();
        }
        order.extend(own);
        left = rest;
    }
    order
}

/// Where each chunk lands, copied in `order` one after another from `at` on.
fn land(tracks: &[Track], order: &[Chunk], mut at: u64) -> Vec<Landed> {
    order
        .iter()
        .map(|c| {
            let landed = Landed {
                track: c.track,
                first: c.first,
                count: c.count,
                at,
            };
            at += bytes_of(tracks, &landed);
            landed
        })
        .collect()
}

/// The bytes of a chunk's samples.
fn bytes_of(tracks: &[Track], chunk: &Landed) -> u64 {
    tracks.get(chunk.track).map_or(0, |t| {
        t.samples
            .iter()
            .skip(chunk.first)
            .take(chunk.count)
            .map(|s| u64::from(s.size))
            .sum()
    })
}

/// Each track's chunks, in its samples' order: where each starts and how many samples it holds.
/// A host track's as its samples lie (a run of them one after another is a chunk, as is each run
/// of one sample description); every other's where it landed.
fn tables(tracks: &[Track], host: usize, landed: &[Landed]) -> (Vec<Vec<u64>>, Vec<Vec<u32>>) {
    let mut offsets: Vec<Vec<u64>> = vec![Vec::new(); tracks.len()];
    let mut per_chunk: Vec<Vec<u32>> = vec![Vec::new(); tracks.len()];
    for (n, track) in tracks.iter().enumerate().filter(|(_, t)| t.input == host) {
        let (Some(o), Some(p)) = (offsets.get_mut(n), per_chunk.get_mut(n)) else {
            continue;
        };
        let mut end = None;
        for (i, sample) in track.samples.iter().enumerate() {
            let described = track.descriptions.iter().any(|(at, _)| *at == i);
            if end == Some(sample.offset) && !described && !p.is_empty() {
                if let Some(count) = p.last_mut() {
                    *count += 1;
                }
            } else {
                o.push(sample.offset);
                p.push(1);
            }
            end = Some(sample.offset.saturating_add(u64::from(sample.size)));
        }
    }
    let mut sorted = landed.to_vec();
    sorted.sort_by_key(|l| (l.track, l.first));
    for l in sorted {
        if let (Some(o), Some(p)) = (offsets.get_mut(l.track), per_chunk.get_mut(l.track)) {
            o.push(l.at);
            p.push(l.count as u32);
        }
    }
    (offsets, per_chunk)
}

/// The join written into the host: an `mdat` of the chunks `landed` on its end, a shortened
/// input cut where each of its chunks starts once that chunk is on the disk, then the index; then
/// its own boxes made `free` and its brands the usual join's.
fn write_into(
    made: &Made<'_>,
    tracks: &[Track],
    landed: &[Landed],
    files: &mut [Box<dyn Source>],
    watch: &mut Option<Box<dyn FnMut() + Send>>,
) -> Result<(), Error> {
    let host = made.host;
    let wrote = |e: std::io::Error| format!("{} could not be written: {e}", host.display());
    let mut cutters: Vec<Option<File>> = Vec::with_capacity(made.inputs.len());
    for (n, input) in made.inputs.iter().enumerate() {
        let cut = made.shortened.get(n).copied().unwrap_or(false);
        cutters.push(if cut {
            let opened = std::fs::OpenOptions::new().write(true).open(input);
            Some(opened.map_err(|e| format!("{} could not be opened: {e}", input.display()))?)
        } else {
            None
        });
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(host)
        .map_err(wrote)?;
    file.seek(SeekFrom::Start(made.length)).map_err(wrote)?;
    let mut writer = BufWriter::with_capacity(COPY_BUFFER, file);
    writer.write_all(&1u32.to_be_bytes()).map_err(wrote)?;
    writer.write_all(b"mdat").map_err(wrote)?;
    writer
        .write_all(&(made.data + 16).to_be_bytes())
        .map_err(wrote)?;
    for chunk in landed {
        let one = [Chunk {
            track: chunk.track,
            first: chunk.first,
            count: chunk.count,
            start: 0,
        }];
        copy_chunks(tracks, &one, files, &mut writer, host)?;
        let Some(track) = tracks.get(chunk.track) else {
            continue;
        };
        let from = track.samples.get(chunk.first).map(|s| s.offset);
        if let (Some(Some(cutter)), Some(from)) = (cutters.get(track.input), from) {
            // On the disk in the join first, then let go of.
            writer.flush().map_err(wrote)?;
            if let Some(watch) = watch.as_mut() {
                watch();
            }
            cutter
                .set_len(from)
                .map_err(|e| format!("an input could not be shortened: {e}"))?;
        }
    }
    writer.write_all(made.index).map_err(wrote)?;
    let mut file = writer.into_inner().map_err(|e| wrote(e.into_error()))?;
    for at in &made.boxes.freed {
        file.seek(SeekFrom::Start(at + 4)).map_err(wrote)?;
        file.write_all(b"free").map_err(wrote)?;
    }
    if let Some((at, length)) = made.boxes.brands {
        file.seek(SeekFrom::Start(at)).map_err(wrote)?;
        file.write_all(&brands(length)).map_err(wrote)?;
    }
    file.sync_all().map_err(wrote)?;
    if let Some(watch) = watch.as_mut() {
        watch();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
