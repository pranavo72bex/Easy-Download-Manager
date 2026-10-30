//! Writing the joined file: chunks, sample tables and the movie box.

use super::*;

pub(crate) fn boxed(kind: &[u8; 4], parts: &[&[u8]]) -> Vec<u8> {
    let len: usize = parts.iter().map(|p| p.len()).sum::<usize>() + 8;
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&(len as u32).to_be_bytes());
    out.extend_from_slice(kind);
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

pub(crate) fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let head = ((u32::from(version) << 24) | (flags & 0x00ff_ffff)).to_be_bytes();
    boxed(kind, &[&head, body])
}

pub(crate) const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

pub(super) fn scale(value: u64, from: u32, to: u32) -> u64 {
    if from == 0 {
        return 0;
    }
    (u128::from(value) * u128::from(to) / u128::from(from)) as u64
}

/// [`scale`], rounded up.
pub(super) fn scale_up(value: u64, from: u32, to: u32) -> u64 {
    if from == 0 {
        return 0;
    }
    (u128::from(value) * u128::from(to)).div_ceil(u128::from(from)) as u64
}

/// A run of one track's samples written together.
pub(super) struct Chunk {
    pub(super) track: usize,
    pub(super) first: usize,
    pub(super) count: usize,
    /// When it starts, in seconds scaled to nanoseconds, for interleaving.
    pub(super) start: u128,
}

pub(super) fn chunks(tracks: &[Track]) -> Vec<Chunk> {
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

pub(super) fn media_duration(track: &Track) -> u64 {
    track.samples.iter().map(|s| u64::from(s.duration)).sum()
}

/// When a track's first frame is shown and when its last one ends, in its
/// own timescale. The first is later than zero for a picture whose frames
/// are decoded ahead of being shown (B-frames), and the last then ends as
/// much after the decoding does.
pub(super) fn shown(track: &Track) -> (i64, i64) {
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
/// A part's one edit for a track: from `skip_ms` into it (past its own
/// start, an AAC track's priming), for `length_ms` or what is left of the
/// track, whichever is shorter. The media time, and the length in
/// milliseconds.
pub(super) fn cut_edit(track: &Track, skip_ms: u64, length_ms: u64) -> (i64, u64) {
    let (first, last) = shown(track);
    let own = track
        .edits
        .iter()
        .find(|e| e.media_time >= 0)
        .map(|e| e.media_time)
        .unwrap_or(first.max(0));
    let media_time = own + scale(skip_ms, MOVIE_TIMESCALE, track.timescale) as i64;
    let left = u64::try_from(last.saturating_sub(media_time)).unwrap_or(0);
    let left_ms = scale_up(left, track.timescale, MOVIE_TIMESCALE);
    (media_time, length_ms.min(left_ms))
}

/// How long a track plays for, in milliseconds: its wait and its part
/// where it is cut, else all of it.
pub(super) fn presented_ms(track: &Track, lead_ms: u64, trim: Option<(u64, u64)>) -> u64 {
    match trim {
        Some((skip_ms, length_ms)) => lead_ms + cut_edit(track, skip_ms, length_ms).1,
        None => lead_ms + scale(media_duration(track), track.timescale, MOVIE_TIMESCALE),
    }
}

pub(super) fn edit_list(track: &Track, lead_ms: u64, trim: Option<(u64, u64)>) -> Option<Vec<u8>> {
    let (first, last) = shown(track);
    let edits_scale = if track.edits_timescale > 0 {
        track.edits_timescale
    } else {
        MOVIE_TIMESCALE
    };
    let mut edits = if let Some((skip_ms, length_ms)) = trim {
        let (media_time, length_ms) = cut_edit(track, skip_ms, length_ms);
        vec![Edit {
            duration: scale(length_ms, MOVIE_TIMESCALE, edits_scale).max(1),
            media_time,
        }]
    } else if track.edits.is_empty() {
        if first <= 0 && lead_ms == 0 {
            return None;
        }
        vec![Edit {
            duration: 0,
            media_time: first.max(0),
        }]
    } else {
        track.edits.clone()
    };
    // A track that starts later than the earliest waits that long.
    if lead_ms > 0 {
        edits.insert(
            0,
            Edit {
                duration: scale(lead_ms, MOVIE_TIMESCALE, track.edits_timescale),
                media_time: -1,
            },
        );
    }
    let mut body = Vec::new();
    for edit in &edits {
        let duration = if edit.media_time < 0 {
            scale(edit.duration, track.edits_timescale, MOVIE_TIMESCALE)
        } else if edit.duration == 0 {
            // Rounded up: to the end means every sample, and 6.037188 s
            // written as 6.037 had a player cut the last eight.
            scale_up(
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

pub(super) fn sample_table(track: &Track, offsets: &[u64], per_chunk: &[u32]) -> Vec<u8> {
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

pub(super) fn trak(
    track: &Track,
    id: u32,
    offsets: &[u64],
    per_chunk: &[u32],
    lead_ms: u64,
    trim: Option<(u64, u64)>,
) -> Vec<u8> {
    let media = media_duration(track);
    // What it plays for, with its wait: a cut's part, else the whole.
    let movie = presented_ms(track, lead_ms, trim);
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
    let edts = edit_list(track, lead_ms, trim).unwrap_or_default();
    boxed(b"trak", &[&full(b"tkhd", 1, 3, &tkhd), &edts, &mdia])
}

/// The index of the output: every track, laid out as `layout` says, its
/// samples starting at `data_at`. With a `cut` (from and to, in
/// milliseconds of the inputs' own timeline), each track's edit list
/// starts it at `from` and ends it at `to`: the output plays that part
/// alone, whatever its pieces hold around it.
pub(super) fn moov(
    tracks: &[Track],
    layout: &[Chunk],
    data_at: u64,
    cut: Option<(u64, u64)>,
) -> Vec<u8> {
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
    let start_ms = |t: &Track| t.start.map(|s| scale(s, t.timescale, MOVIE_TIMESCALE));
    let earliest = tracks.iter().filter_map(start_ms).min().unwrap_or(0);
    // Each track's wait and part, as its edit list will say.
    let placed = |track: &Track| match cut {
        Some((from, to)) => {
            // A track that says when it starts (a stream's pieces, tfdt)
            // is placed there; one that does not (subtitles, a plain file)
            // starts at the video's own zero.
            let start = start_ms(track).unwrap_or(0);
            let lead = start.saturating_sub(from);
            let skip = from.saturating_sub(start);
            (
                lead,
                Some((skip, to.saturating_sub(from).saturating_sub(lead))),
            )
        }
        None => (start_ms(track).map_or(0, |s| s - earliest), None),
    };
    let duration = tracks
        .iter()
        .map(|t| {
            let (lead, trim) = placed(t);
            presented_ms(t, lead, trim)
        })
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
        // The output starts at `from`: a track that begins before it is
        // entered that far in, one that begins after it waits.
        let (lead, trim) = placed(track);
        parts.push(trak(
            track,
            i as u32 + 1,
            offsets.get(i).map(Vec::as_slice).unwrap_or_default(),
            per_chunk.get(i).map(Vec::as_slice).unwrap_or_default(),
            lead,
            trim,
        ));
    }
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    boxed(b"moov", &refs)
}

pub(crate) fn write_output(
    tracks: &[Track],
    files: &mut [File],
    out: &Path,
    cut: Option<(u64, u64)>,
) -> Result<u64, String> {
    let ftyp = boxed(
        b"ftyp",
        &[b"isom", &0x200u32.to_be_bytes(), b"isomiso2avc1mp41"],
    );
    let layout = chunks(tracks);
    // The index's length does not depend on the offsets in it (all 64-bit),
    // so it is laid out once to measure and once for real.
    let measured = moov(tracks, &layout, 0, cut).len() as u64;
    let data_at = ftyp.len() as u64 + measured + 16;
    let index = moov(tracks, &layout, data_at, cut);
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
