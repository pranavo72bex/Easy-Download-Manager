//! Reading the MP4 files and fragments that are joined.

use super::*;

pub(super) fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

pub(super) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

pub(super) fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// The boxes in `data`, in order: each one's type, its body, and the whole
/// box. Stops at the first that does not fit.
pub(super) fn boxes(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8], &[u8])> {
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

pub(super) fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _, _)| k == kind).map(|(_, b, _)| b)
}

pub(super) fn child_whole<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _, _)| k == kind).map(|(_, _, w)| w)
}

/// The top-level boxes of a file too big to read whole: `moov` and each
/// `moof` read into memory with where it starts, the rest skipped.
pub(super) fn read_input(file: &mut File, length: u64, input: usize) -> Result<Vec<Track>, String> {
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

pub(super) fn movie_timescale(moov: &[u8]) -> u32 {
    child(moov, b"mvhd")
        .and_then(|m| match m.first()? {
            1 => u32_at(m, 20),
            _ => u32_at(m, 12),
        })
        .filter(|t| *t > 0)
        .unwrap_or(MOVIE_TIMESCALE)
}

pub(super) fn tracks_of(moov: &[u8], input: usize) -> Result<Vec<Track>, String> {
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
            start: None,
        });
    }
    Ok(tracks)
}

pub(super) fn read_edits(elst: &[u8]) -> Vec<Edit> {
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
pub(super) fn entries<T>(
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
pub(super) fn plain_samples(stbl: &[u8]) -> Result<Vec<Sample>, String> {
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

pub(super) fn trex_defaults(moov: &[u8]) -> Vec<(u32, Defaults)> {
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
pub(super) fn is_sync(flags: u32) -> bool {
    flags & 0x0001_0000 == 0
}

pub(super) fn read_fragment(
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
        if track.samples.is_empty() && track.start.is_none() {
            track.start = child(traf, b"tfdt").and_then(|t| match t.first() {
                Some(1) => u64_at(t, 4),
                _ => u32_at(t, 4).map(u64::from),
            });
        }
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
