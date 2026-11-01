//! A part of a plain MP4, cut from its sample tables.
//!
//! A plain MP4 (`moov` with its sample tables, the frames in `mdat`) says in
//! its index where every frame is and when it plays. So a part of it, from
//! second A to second B, needs only the index and the frames between: this
//! reads the index, keeps each track's samples from the last sync sample at
//! or before A to the last one that starts before B, works out which byte
//! ranges of the file hold them, and, once those are gathered into one
//! compact file, writes the part as an MP4 with the same box-building as a
//! join (`write_output`), the edit lists trimming it to A..B exactly.

use super::*;

/// A gap smaller than this between two wanted ranges is fetched rather than
/// skipped: picture and sound alternate in a file in pieces of about a
/// second, and a request each would cost more than the bytes between.
const MERGE_GAP: u64 = 128 << 10;

/// What to fetch of a plain MP4 and how to build the part from it.
pub(crate) struct PlainPart {
    /// The byte ranges of the original file to fetch, in order, first and
    /// last byte inclusive. Written one after another into the data file.
    pub(crate) ranges: Vec<(u64, u64)>,
    /// The tracks, trimmed, their sample offsets pointing into the data file.
    tracks: Vec<Track>,
}

impl PlainPart {
    /// How many bytes the ranges add up to.
    pub(crate) fn bytes(&self) -> u64 {
        self.ranges.iter().map(|(a, b)| b - a + 1).sum()
    }
}

/// The part `from_ms`..`to_ms` of the MP4 whose `moov` box alone is in
/// `index` (the box as it lies in the file, header included).
pub(crate) fn plan_plain_part(index: &Path, from_ms: u64, to_ms: u64) -> Result<PlainPart, String> {
    let mut file =
        File::open(index).map_err(|e| format!("{} could not be read: {e}", index.display()))?;
    let length = file
        .metadata()
        .map_err(|e| format!("{} could not be read: {e}", index.display()))?
        .len();
    let read = read_input(&mut file, length, 0)
        .map_err(|e| format!("this MP4's index could not be read: {e}"))?;

    let mut tracks = Vec::new();
    for mut track in read {
        if track.samples.is_empty() || track.timescale == 0 {
            continue;
        }
        let scale = u64::from(track.timescale);
        let from_t = from_ms.saturating_mul(scale) / 1000;
        let to_t = (to_ms.saturating_mul(scale)).div_ceil(1000);
        let mut starts = Vec::with_capacity(track.samples.len());
        let mut at = 0u64;
        for sample in &track.samples {
            starts.push(at);
            at = at.saturating_add(u64::from(sample.duration));
        }
        // From the last sample that starts at or before A, back to a sync
        // sample (a picture can only start at one); to the last that
        // starts before B.
        let at_from = starts.iter().rposition(|t| *t <= from_t).unwrap_or(0);
        // If the sample at at_from ends before from_t, the whole track is past the end.
        let at_from_end =
            starts[at_from] + u64::from(track.samples[at_from].duration);
        if at_from_end <= from_t {
            continue;
        }
        let first = track
            .samples
            .get(..=at_from)
            .and_then(|head| head.iter().rposition(|s| s.sync))
            .unwrap_or(0);
        let end = starts
            .iter()
            .position(|t| *t >= to_t)
            .unwrap_or(starts.len());
        if end <= first {
            continue;
        }
        track.start = starts.get(first).copied();
        // The input's edit list counted from its first sample, which is no
        // longer the first: the cut's own edit list takes its place.
        track.edits.clear();
        track.samples = track.samples.get(first..end).unwrap_or_default().to_vec();
        tracks.push(track);
    }
    if tracks.is_empty() {
        return Err("the video has nothing between those times".into());
    }

    // The bytes wanted, as ranges (end exclusive) merged where near.
    let mut wanted: Vec<(u64, u64)> = tracks
        .iter()
        .flat_map(|t| t.samples.iter())
        .filter(|s| s.size > 0)
        .map(|s| (s.offset, s.offset.saturating_add(u64::from(s.size))))
        .collect();
    wanted.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (a, b) in wanted {
        match merged.last_mut() {
            Some(last) if a <= last.1.saturating_add(MERGE_GAP) => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    if merged.is_empty() {
        return Err("the video has nothing between those times".into());
    }
    // Where each range begins in the data file.
    let mut bases = Vec::with_capacity(merged.len());
    let mut total = 0u64;
    for (a, b) in &merged {
        bases.push(total);
        total += b - a;
    }
    for track in &mut tracks {
        track.input = 0;
        for sample in &mut track.samples {
            let found = merged.partition_point(|r| r.1 <= sample.offset);
            let (range, base) = merged
                .get(found)
                .zip(bases.get(found))
                .filter(|(r, _)| r.0 <= sample.offset)
                .ok_or("a sample lies outside what was planned")?;
            sample.offset = base + (sample.offset - range.0);
        }
    }
    Ok(PlainPart {
        ranges: merged.into_iter().map(|(a, b)| (a, b - 1)).collect(),
        tracks,
    })
}

/// The part written to `out`: its frames read from `data` (the planned
/// ranges one after another), cut to `cut` (from and to, milliseconds) by
/// edit lists, with the picture's crop where `shape` asks.
pub(crate) fn write_plain_part(
    mut plan: PlainPart,
    data: &Path,
    out: &Path,
    cut: (u64, u64),
    shape: Option<crate::shape::Shape>,
) -> Result<u64, String> {
    if let Some(shape) = shape {
        for track in &mut plan.tracks {
            crate::shape::apply(track, shape)?;
        }
    }
    let mut files =
        vec![File::open(data).map_err(|e| format!("{} could not be read: {e}", data.display()))?];
    write_output(&plan.tracks, &mut files, out, Some(cut))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4mux::tests::plain;

    /// The `moov` box of a file, as it lies in it.
    fn moov_of_file(bytes: &[u8]) -> Vec<u8> {
        let at = bytes.windows(4).position(|w| w == b"moov").unwrap() - 4;
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        bytes[at..at + size].to_vec()
    }

    #[test]
    fn a_part_of_a_plain_file_needs_only_the_frames_between_its_times() {
        let dir = tempfile::tempdir().unwrap();
        // 200 samples of 1024 ticks at 48 kHz (4.27 s), 10 bytes each.
        let (path, samples) = plain(dir.path(), "whole.m4a", 200, 0);
        let whole = std::fs::read(&path).unwrap();
        let index = dir.path().join("index.mp4");
        std::fs::write(&index, moov_of_file(&whole)).unwrap();

        let plan = plan_plain_part(&index, 1000, 2000).unwrap();
        // Samples 46 to 93 start at or after the second and before the
        // second after it: 48 samples of 10 bytes, a single range.
        assert_eq!(plan.bytes(), 480);
        assert_eq!(plan.ranges.len(), 1);

        // Gathered as the download does, and built into a part.
        let data = dir.path().join("part.data");
        let mut gathered = Vec::new();
        for (a, b) in &plan.ranges {
            gathered.extend_from_slice(&whole[*a as usize..=*b as usize]);
        }
        std::fs::write(&data, &gathered).unwrap();
        assert_eq!(&gathered[..10], samples[46].as_slice());
        let out = dir.path().join("part.mp4");
        write_plain_part(plan, &data, &out, (1000, 2000), None).unwrap();
        let tracks = tracks_in_file(&out).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].samples.len(), 48);
    }

    #[test]
    fn times_past_the_end_of_the_file_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (path, _) = plain(dir.path(), "whole.m4a", 20, 0);
        let whole = std::fs::read(&path).unwrap();
        let index = dir.path().join("index.mp4");
        std::fs::write(&index, moov_of_file(&whole)).unwrap();
        assert!(plan_plain_part(&index, 60_000, 70_000).is_err());
    }
}
