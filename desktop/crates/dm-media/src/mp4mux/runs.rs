// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! One stream in runs, each read against an index of its own: an HLS stream whose
//! initialisation segment changes part-way (`EXT-X-MAP`, an advert break and back), its
//! segments joined as they came (gaps row Y16).
//!
//! Each run's tracks join the stream's track of the same kind (the first picture with the first
//! picture, the first sound with the first sound), in the first run's timescale, and each run's
//! sample description becomes one more entry of that track's (`Track::descriptions`), so every
//! sample is decoded with the set-up it was made with. A run's clock may start afresh (the
//! advert's starts at nought), so a run is placed by its own clock only within itself: it starts
//! where the run before it ended, by its longest track, and a track that ended sooner holds its
//! last sample until then. Picture and sound start each run together, as they did in it, and
//! never drift apart over many breaks.

use super::*;

const NANOS: u128 = 1_000_000_000;

/// What kind of track: its handler (`vide`, `soun`, `text`...).
fn handler(track: &Track) -> [u8; 4] {
    track
        .hdlr
        .get(16..20)
        .and_then(|h| h.try_into().ok())
        .unwrap_or_default()
}

/// The entries of a whole `stsd` box, each a whole box.
pub(super) fn entries_of(stsd: &[u8]) -> Vec<&[u8]> {
    boxes(stsd.get(16..).unwrap_or_default())
        .map(|(_, _, whole)| whole)
        .collect()
}

fn nanos(ticks: u64, timescale: u32) -> u128 {
    u128::from(ticks) * NANOS / u128::from(timescale.max(1))
}

fn ticks(nanos: u128, timescale: u32) -> u64 {
    u64::try_from(nanos * u128::from(timescale) / NANOS).unwrap_or(u64::MAX)
}

/// A track of the stream, made from the first run's track of its kind, with no samples yet.
fn empty_like(track: &Track) -> Track {
    // An edit that skips an encoder's priming applies once, at the start, and runs to the end.
    let edits = track
        .edits
        .iter()
        .find(|e| e.media_time >= 0)
        .map(|e| Edit {
            duration: 0,
            media_time: e.media_time,
        })
        .into_iter()
        .collect();
    Track {
        input: track.input,
        id: track.id,
        tkhd_tail: track.tkhd_tail.clone(),
        timescale: track.timescale,
        language: track.language,
        hdlr: track.hdlr.clone(),
        media_header: track.media_header.clone(),
        stsd: Vec::new(),
        edits,
        edits_timescale: track.edits_timescale,
        samples: Vec::new(),
        start: None,
        descriptions: Vec::new(),
    }
}

/// The stream's tracks, from its runs in order.
pub(super) fn merge(runs: Vec<Vec<Track>>) -> Result<Vec<Track>, String> {
    let mut out: Vec<Track> = Vec::new();
    // Each track's sample descriptions so far, and where its next sample starts, in its ticks.
    let mut described: Vec<Vec<Vec<u8>>> = Vec::new();
    let mut cursor: Vec<u64> = Vec::new();
    // Where the run being placed starts, in nanoseconds of the first run's clock.
    let mut place: Option<u128> = None;
    for run in runs {
        let tracks: Vec<Track> = run.into_iter().filter(|t| !t.samples.is_empty()).collect();
        let starts: Vec<u128> = tracks
            .iter()
            .map(|t| nanos(t.start.unwrap_or(0), t.timescale))
            .collect();
        let Some(origin) = starts.iter().min().copied() else {
            continue;
        };
        let end = tracks
            .iter()
            .zip(&starts)
            .map(|(t, s)| s + nanos(write::media_duration(t), t.timescale))
            .max()
            .unwrap_or(origin);
        let at = *place.get_or_insert(origin);
        let mut seen: Vec<[u8; 4]> = Vec::new();
        for (track, start) in tracks.into_iter().zip(starts) {
            // The n-th track of a kind in this run is the n-th of that kind in the stream.
            let kind = handler(&track);
            let nth = seen.iter().filter(|k| **k == kind).count();
            seen.push(kind);
            let k = match out
                .iter()
                .enumerate()
                .filter(|(_, t)| handler(t) == kind)
                .nth(nth)
            {
                Some((k, _)) => k,
                None => {
                    out.push(empty_like(&track));
                    described.push(Vec::new());
                    cursor.push(0);
                    out.len() - 1
                }
            };
            let (Some(joined), Some(entries), Some(next)) =
                (out.get_mut(k), described.get_mut(k), cursor.get_mut(k))
            else {
                return Err("a run of the stream went missing".into());
            };
            append(joined, entries, next, &track, at + (start - origin))?;
        }
        place = Some(at + (end - origin));
    }
    for (track, entries) in out.iter_mut().zip(&described) {
        let mut body = (entries.len() as u32).to_be_bytes().to_vec();
        for entry in entries {
            body.extend_from_slice(entry);
        }
        track.stsd = full(b"stsd", 0, 0, &body);
    }
    Ok(out)
}

/// One run's track added to the stream's `joined`, its first sample at `target` nanoseconds:
/// its sample descriptions among `entries`, its times in `joined`'s timescale, and the gap since
/// `joined`'s last sample (at `next`) held by that sample.
fn append(
    joined: &mut Track,
    entries: &mut Vec<Vec<u8>>,
    next: &mut u64,
    track: &Track,
    target: u128,
) -> Result<(), String> {
    let (into, from) = (joined.timescale, track.timescale.max(1));
    let target = ticks(target, into);
    if joined.samples.is_empty() {
        joined.start = Some(target);
        *next = target;
    } else if target > *next {
        let gap = u32::try_from(target - *next).unwrap_or(u32::MAX);
        if let Some(last) = joined.samples.last_mut() {
            last.duration = last.duration.saturating_add(gap);
        }
        *next = target;
    }
    // This run's descriptions, as entries of the stream's: one already there is used again.
    let mut mapping = Vec::new();
    for entry in entries_of(&track.stsd) {
        let index = match entries.iter().position(|e| e == entry) {
            Some(i) => i,
            None => {
                entries.push(entry.to_vec());
                entries.len() - 1
            }
        };
        mapping.push(index as u32 + 1);
    }
    if mapping.is_empty() {
        return Err("a run of the stream has no sample description".into());
    }
    let base = *next;
    let rescale = |t: u64| base + write::scale(t, from, into);
    let mut elapsed = 0u64;
    joined.samples.reserve(track.samples.len());
    for (i, sample) in track.samples.iter().enumerate() {
        let own = track.description_of(i) as usize;
        let description = own
            .checked_sub(1)
            .and_then(|d| mapping.get(d))
            .copied()
            .ok_or("a sample names a description its run does not have")?;
        if joined.descriptions.last().map_or(1, |(_, d)| *d) != description {
            joined
                .descriptions
                .push((joined.samples.len(), description));
        }
        let begins = rescale(elapsed);
        elapsed += u64::from(sample.duration);
        let cts = i128::from(sample.cts) * i128::from(into) / i128::from(from);
        joined.samples.push(Sample {
            duration: u32::try_from(rescale(elapsed) - begins).unwrap_or(u32::MAX),
            cts: i32::try_from(cts).unwrap_or(0),
            ..*sample
        });
    }
    *next = rescale(elapsed);
    Ok(())
}
