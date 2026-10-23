//! The muxers against real encoder output: H.264 with B-frames and AAC,
//! fragmented as DASH serves them and plain; transport streams of H.264 or
//! HEVC with AAC, MP3, MP2, AC-3 or E-AC-3; VP9, AV1 and Opus in WebM. The
//! inputs are small files under `tests/fixtures`, made once with a standard
//! encoder and kept, so every computer runs every case with nothing
//! installed.
//!
//! dm copies samples and never re-encodes them, so a join is judged by
//! reading its output back with the readers in this file — written for the
//! test, sharing nothing with dm's own — and holding every sample to the
//! input's: the same bytes, in the same order, at the same times.

use std::path::{Path, PathBuf};

fn media(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media/mp4mux")
        .join(name)
}

fn be(bytes: &[u8], at: usize, len: usize) -> u64 {
    bytes[at..at + len]
        .iter()
        .fold(0, |n, b| (n << 8) | u64::from(*b))
}

// ---------------------------------------------------------------------------
// MP4: plain (`moov` sample tables) and fragmented (`moof`), read whole.

#[derive(Debug, Clone, PartialEq)]
struct Mp4Sample {
    data: Vec<u8>,
    dts: u64,
    duration: u32,
    cts: i32,
    sync: bool,
}

#[derive(Debug, Default)]
struct Mp4Track {
    id: u32,
    handler: String,
    codec: String,
    timescale: u32,
    language: String,
    width: u32,
    height: u32,
    /// (duration in the movie's timescale, media time); -1 is an empty edit.
    edits: Vec<(u64, i64)>,
    samples: Vec<Mp4Sample>,
}

impl Mp4Track {
    /// When its first sample is shown, in seconds, by its edit list.
    fn start(&self, movie_timescale: u32) -> f64 {
        let mut start = 0.0;
        let mut media_time = 0;
        for &(duration, time) in &self.edits {
            if time == -1 {
                start += duration as f64 / f64::from(movie_timescale);
            } else {
                media_time = time;
                break;
            }
        }
        let first = self
            .samples
            .iter()
            .map(|s| s.dts as i64 + i64::from(s.cts))
            .min()
            .unwrap();
        start + (first - media_time).max(0) as f64 / f64::from(self.timescale)
    }

    fn seconds(&self) -> f64 {
        let ticks: u64 = self.samples.iter().map(|s| u64::from(s.duration)).sum();
        ticks as f64 / f64::from(self.timescale)
    }
}

struct Mp4 {
    timescale: u32,
    tracks: Vec<Mp4Track>,
}

impl Mp4 {
    fn track(&self, handler: &str) -> &Mp4Track {
        let found: Vec<_> = self
            .tracks
            .iter()
            .filter(|t| t.handler == handler)
            .collect();
        assert_eq!(found.len(), 1, "one {handler} track");
        found[0]
    }
}

/// The boxes in `data[start..end]`: kind, where the box starts, where its
/// body starts, and where it ends.
fn boxes(data: &[u8], start: usize, end: usize) -> Vec<([u8; 4], usize, usize, usize)> {
    let mut found = Vec::new();
    let mut at = start;
    while at + 8 <= end {
        let mut size = be(data, at, 4) as usize;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().unwrap();
        let mut body = at + 8;
        if size == 1 {
            size = be(data, at + 8, 8) as usize;
            body += 8;
        } else if size == 0 {
            size = end - at;
        }
        assert!(size >= body - at && at + size <= end, "a box overruns");
        found.push((kind, at, body, at + size));
        at += size;
    }
    found
}

fn child<'a>(data: &'a [u8], parent: (usize, usize), kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data, parent.0, parent.1)
        .into_iter()
        .find(|b| &b.0 == kind)
        .map(|(_, _, body, end)| &data[body..end])
}

fn child_range(data: &[u8], parent: (usize, usize), kind: &[u8; 4]) -> Option<(usize, usize)> {
    boxes(data, parent.0, parent.1)
        .into_iter()
        .find(|b| &b.0 == kind)
        .map(|(_, _, body, end)| (body, end))
}

fn fourcc(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn read_mp4(path: &Path) -> Mp4 {
    let data = std::fs::read(path).unwrap();
    let top = boxes(&data, 0, data.len());
    let moov = top.iter().find(|b| &b.0 == b"moov").expect("a moov");
    let moov = (moov.2, moov.3);
    let mvhd = child(&data, moov, b"mvhd").unwrap();
    let timescale = if mvhd[0] == 1 {
        be(mvhd, 20, 4)
    } else {
        be(mvhd, 12, 4)
    } as u32;

    let mut tracks = Vec::new();
    for (_, _, body, end) in boxes(&data, moov.0, moov.1) {
        if &data[body - 4..body] != b"trak" {
            continue;
        }
        let trak = (body, end);
        let tkhd = child(&data, trak, b"tkhd").unwrap();
        let id = if tkhd[0] == 1 {
            be(tkhd, 20, 4)
        } else {
            be(tkhd, 12, 4)
        } as u32;
        let width = (be(tkhd, tkhd.len() - 8, 4) >> 16) as u32;
        let height = (be(tkhd, tkhd.len() - 4, 4) >> 16) as u32;
        let mdia = child_range(&data, trak, b"mdia").unwrap();
        let mdhd = child(&data, mdia, b"mdhd").unwrap();
        let (track_timescale, packed) = if mdhd[0] == 1 {
            (be(mdhd, 20, 4), be(mdhd, 32, 2))
        } else {
            (be(mdhd, 12, 4), be(mdhd, 20, 2))
        };
        let language: String = [10, 5, 0]
            .iter()
            .map(|shift| char::from(((packed >> shift) & 0x1f) as u8 + 0x60))
            .collect();
        let handler = fourcc(&child(&data, mdia, b"hdlr").unwrap()[8..12]);
        let minf = child_range(&data, mdia, b"minf").unwrap();
        let stbl = child_range(&data, minf, b"stbl").unwrap();
        let stsd = child(&data, stbl, b"stsd").unwrap();
        let codec = fourcc(&stsd[12..16]);
        let mut edits = Vec::new();
        if let Some(edts) = child_range(&data, trak, b"edts") {
            if let Some(elst) = child(&data, edts, b"elst") {
                let v1 = elst[0] == 1;
                let entry = if v1 { 20 } else { 12 };
                for n in 0..be(elst, 4, 4) as usize {
                    let at = 8 + n * entry;
                    edits.push(if v1 {
                        (be(elst, at, 8), be(elst, at + 8, 8) as i64)
                    } else {
                        (
                            be(elst, at, 4),
                            i64::from(be(elst, at + 4, 4) as u32 as i32),
                        )
                    });
                }
            }
        }
        let samples = table_samples(&data, stbl);
        tracks.push(Mp4Track {
            id,
            handler,
            codec,
            timescale: track_timescale as u32,
            language,
            width,
            height,
            edits,
            samples,
        });
    }
    fragment_samples(&data, &top, moov, &mut tracks);
    Mp4 { timescale, tracks }
}

/// The samples a `stbl` lists, with their bytes.
fn table_samples(data: &[u8], stbl: (usize, usize)) -> Vec<Mp4Sample> {
    let stsz = child(data, stbl, b"stsz").unwrap();
    let count = be(stsz, 8, 4) as usize;
    let fixed = be(stsz, 4, 4);
    let sizes: Vec<u64> = (0..count)
        .map(|n| {
            if fixed != 0 {
                fixed
            } else {
                be(stsz, 12 + 4 * n, 4)
            }
        })
        .collect();
    if count == 0 {
        return Vec::new();
    }
    let offsets: Vec<u64> = if let Some(co64) = child(data, stbl, b"co64") {
        (0..be(co64, 4, 4) as usize)
            .map(|n| be(co64, 8 + 8 * n, 8))
            .collect()
    } else {
        let stco = child(data, stbl, b"stco").unwrap();
        (0..be(stco, 4, 4) as usize)
            .map(|n| be(stco, 8 + 4 * n, 4))
            .collect()
    };
    let stsc = child(data, stbl, b"stsc").unwrap();
    let runs: Vec<(u64, u64)> = (0..be(stsc, 4, 4) as usize)
        .map(|n| (be(stsc, 8 + 12 * n, 4), be(stsc, 12 + 12 * n, 4)))
        .collect();
    let mut places = Vec::new();
    for (chunk, offset) in offsets.iter().enumerate() {
        let number = chunk as u64 + 1;
        let per = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= number)
            .unwrap()
            .1;
        let mut at = *offset;
        for _ in 0..per {
            let size = sizes[places.len()];
            places.push((at as usize, size as usize));
            at += size;
        }
    }
    assert_eq!(places.len(), count, "the chunk map covers every sample");

    let stts = child(data, stbl, b"stts").unwrap();
    let mut durations = Vec::new();
    for n in 0..be(stts, 4, 4) as usize {
        let (times, delta) = (be(stts, 8 + 8 * n, 4), be(stts, 12 + 8 * n, 4));
        durations.extend(std::iter::repeat_n(delta as u32, times as usize));
    }
    let mut shifts = Vec::new();
    if let Some(ctts) = child(data, stbl, b"ctts") {
        for n in 0..be(ctts, 4, 4) as usize {
            let (times, shift) = (be(ctts, 8 + 8 * n, 4), be(ctts, 12 + 8 * n, 4));
            shifts.extend(std::iter::repeat_n(shift as u32 as i32, times as usize));
        }
    }
    let syncs: Option<Vec<u64>> = child(data, stbl, b"stss").map(|stss| {
        (0..be(stss, 4, 4) as usize)
            .map(|n| be(stss, 8 + 4 * n, 4))
            .collect()
    });
    assert_eq!(durations.len(), count, "every sample is timed");
    let mut dts = 0;
    places
        .iter()
        .enumerate()
        .map(|(n, (at, size))| {
            let sample = Mp4Sample {
                data: data[*at..at + size].to_vec(),
                dts,
                duration: durations[n],
                cts: shifts.get(n).copied().unwrap_or(0),
                sync: syncs.as_ref().is_none_or(|s| s.contains(&(n as u64 + 1))),
            };
            dts += u64::from(durations[n]);
            sample
        })
        .collect()
}

/// The samples in a fragmented file's `moof`s, added to their tracks.
fn fragment_samples(
    data: &[u8],
    top: &[([u8; 4], usize, usize, usize)],
    moov: (usize, usize),
    tracks: &mut [Mp4Track],
) {
    // Per track: default description, duration, size, flags.
    let mut defaults = std::collections::HashMap::new();
    if let Some(mvex) = child_range(data, moov, b"mvex") {
        for (kind, _, body, _) in boxes(data, mvex.0, mvex.1) {
            if &kind == b"trex" {
                let trex = &data[body..];
                defaults.insert(
                    be(trex, 4, 4) as u32,
                    (be(trex, 12, 4), be(trex, 16, 4), be(trex, 20, 4)),
                );
            }
        }
    }
    for &(kind, moof_start, body, end) in top {
        if &kind != b"moof" {
            continue;
        }
        for (kind, _, traf_body, traf_end) in boxes(data, body, end) {
            if &kind != b"traf" {
                continue;
            }
            let traf = (traf_body, traf_end);
            let tfhd = child(data, traf, b"tfhd").unwrap();
            let flags = be(tfhd, 1, 3);
            let id = be(tfhd, 4, 4) as u32;
            let track = tracks.iter_mut().find(|t| t.id == id).unwrap();
            let (mut duration, mut size, mut sample_flags) =
                defaults.get(&id).copied().unwrap_or_default();
            let mut at = 8;
            let mut base = moof_start as u64;
            if flags & 0x1 != 0 {
                base = be(tfhd, at, 8);
                at += 8;
            }
            if flags & 0x2 != 0 {
                at += 4;
            }
            if flags & 0x8 != 0 {
                duration = be(tfhd, at, 4);
                at += 4;
            }
            if flags & 0x10 != 0 {
                size = be(tfhd, at, 4);
                at += 4;
            }
            if flags & 0x20 != 0 {
                sample_flags = be(tfhd, at, 4);
            }
            let mut dts = match child(data, traf, b"tfdt") {
                Some(t) if t[0] == 1 => be(t, 4, 8),
                Some(t) => be(t, 4, 4),
                None => track
                    .samples
                    .last()
                    .map_or(0, |s| s.dts + u64::from(s.duration)),
            };
            for (kind, _, run_body, _) in boxes(data, traf.0, traf.1) {
                if &kind != b"trun" {
                    continue;
                }
                let trun = &data[run_body..];
                let run_flags = be(trun, 1, 3);
                let count = be(trun, 4, 4);
                let mut at = 8;
                let mut offset = base;
                if run_flags & 0x1 != 0 {
                    offset = (base as i64 + i64::from(be(trun, at, 4) as u32 as i32)) as u64;
                    at += 4;
                }
                let mut first_flags = None;
                if run_flags & 0x4 != 0 {
                    first_flags = Some(be(trun, at, 4));
                    at += 4;
                }
                for n in 0..count {
                    let mut d = duration;
                    let mut s = size;
                    let mut f = if n == 0 {
                        first_flags.unwrap_or(sample_flags)
                    } else {
                        sample_flags
                    };
                    let mut cts = 0;
                    if run_flags & 0x100 != 0 {
                        d = be(trun, at, 4);
                        at += 4;
                    }
                    if run_flags & 0x200 != 0 {
                        s = be(trun, at, 4);
                        at += 4;
                    }
                    if run_flags & 0x400 != 0 {
                        f = be(trun, at, 4);
                        at += 4;
                    }
                    if run_flags & 0x800 != 0 {
                        cts = be(trun, at, 4) as u32 as i32;
                        at += 4;
                    }
                    let from = offset as usize;
                    track.samples.push(Mp4Sample {
                        data: data[from..from + s as usize].to_vec(),
                        dts,
                        duration: d as u32,
                        cts,
                        sync: f & 0x1_0000 == 0,
                    });
                    dts += d;
                    offset += s;
                }
            }
        }
    }
}

/// Every sample of `ours` is the input's: its bytes, its length of time,
/// when it is shown after it is decoded, whether it is a key frame, and
/// when it is decoded counted from the first.
fn same_samples_as_input(ours: &Mp4Track, input: &Mp4Track) {
    assert_eq!(ours.codec, input.codec);
    assert_eq!(ours.timescale, input.timescale, "{}", ours.codec);
    assert_eq!(ours.samples.len(), input.samples.len(), "{}", ours.codec);
    let (first, input_first) = (ours.samples[0].dts, input.samples[0].dts);
    for (n, (a, b)) in ours.samples.iter().zip(&input.samples).enumerate() {
        assert!(a.data == b.data, "{} sample {n}: other bytes", ours.codec);
        assert_eq!(
            (a.dts - first, a.duration, a.cts, a.sync),
            (b.dts - input_first, b.duration, b.cts, b.sync),
            "{} sample {n}",
            ours.codec
        );
    }
}

// ---------------------------------------------------------------------------
// WebM: EBML elements, read in one pass.

#[derive(Debug, Default)]
struct WebmTrack {
    number: u64,
    kind: u64,
    codec: String,
    language: String,
}

#[derive(Debug, Clone)]
struct WebmBlock {
    track: u64,
    /// When it plays, in the file's timestamp units.
    time: i64,
    key: bool,
    data: Vec<u8>,
    duration: Option<u64>,
}

#[derive(Debug, Default)]
struct Webm {
    doc_type: String,
    /// Nanoseconds per timestamp unit.
    scale: u64,
    tracks: Vec<WebmTrack>,
    blocks: Vec<WebmBlock>,
    /// Each cluster: where it starts after the segment's head, and its time.
    clusters: Vec<(u64, i64)>,
    /// Each cue point: time, track, cluster position.
    cues: Vec<(i64, u64, u64)>,
}

impl Webm {
    fn track(&self, codec: &str) -> &WebmTrack {
        self.tracks
            .iter()
            .find(|t| t.codec == codec)
            .unwrap_or_else(|| panic!("no {codec} track in {:?}", self.tracks))
    }

    fn blocks_of(&self, number: u64) -> Vec<&WebmBlock> {
        self.blocks.iter().filter(|b| b.track == number).collect()
    }
}

/// A variable-length integer: its value (marker kept for an ID, dropped for
/// a size, where all ones means unknown) and its length.
fn vint(data: &[u8], at: usize, keep_marker: bool) -> (u64, usize, bool) {
    let first = data[at];
    let len = first.leading_zeros() as usize + 1;
    assert!(len <= 8, "a bad EBML number");
    let mut value = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first) & (0xff >> len)
    };
    for b in &data[at + 1..at + len] {
        value = (value << 8) | u64::from(*b);
    }
    let unknown = !keep_marker && value == (1u64 << (7 * len)) - 1;
    (value, len, unknown)
}

fn read_webm(path: &Path) -> Webm {
    const ENTERED: [u64; 10] = [
        0x1A45_DFA3, // EBML header
        0x1853_8067, // Segment
        0x1549_A966, // Info
        0x1654_AE6B, // Tracks
        0xAE,        // TrackEntry
        0x1F43_B675, // Cluster
        0xA0,        // BlockGroup
        0x1C53_BB6B, // Cues
        0xBB,        // CuePoint
        0xB7,        // CueTrackPositions
    ];
    let data = std::fs::read(path).unwrap();
    let mut webm = Webm {
        scale: 1_000_000,
        ..Webm::default()
    };
    let mut segment = 0;
    let mut cluster_time = 0;
    let mut at = 0;
    while at < data.len() {
        let (id, id_len, _) = vint(&data, at, true);
        let (size, size_len, unknown) = vint(&data, at + id_len, false);
        let body = at + id_len + size_len;
        if ENTERED.contains(&id) {
            match id {
                0x1853_8067 => segment = body,
                0xAE => webm.tracks.push(WebmTrack::default()),
                0x1F43_B675 => webm.clusters.push(((at - segment) as u64, 0)),
                0xBB => webm.cues.push((0, 0, 0)),
                _ => {}
            }
            at = body;
            continue;
        }
        assert!(!unknown, "a leaf element of unknown size");
        let value = &data[body..body + size as usize];
        let uint = || be(value, 0, value.len());
        let text = || {
            String::from_utf8_lossy(value)
                .trim_end_matches('\0')
                .to_owned()
        };
        match id {
            0x4282 => webm.doc_type = text(),
            0x2A_D7B1 => webm.scale = uint(),
            0xD7 => webm.tracks.last_mut().unwrap().number = uint(),
            0x83 => webm.tracks.last_mut().unwrap().kind = uint(),
            0x86 => webm.tracks.last_mut().unwrap().codec = text(),
            0x22_B59C => webm.tracks.last_mut().unwrap().language = text(),
            0xE7 => {
                cluster_time = uint() as i64;
                webm.clusters.last_mut().unwrap().1 = cluster_time;
            }
            0xA3 | 0xA1 => {
                let (track, len, _) = vint(value, 0, false);
                let relative = i64::from(be(value, len, 2) as u16 as i16);
                let flags = value[len + 2];
                assert_eq!(flags & 0x06, 0, "a laced block");
                webm.blocks.push(WebmBlock {
                    track,
                    time: cluster_time + relative,
                    // A group's block is a key frame unless it references another.
                    key: id == 0xA1 || flags & 0x80 != 0,
                    data: value[len + 3..].to_vec(),
                    duration: None,
                });
            }
            0xFB => webm.blocks.last_mut().unwrap().key = false,
            0x9B => webm.blocks.last_mut().unwrap().duration = Some(uint()),
            0xB3 => webm.cues.last_mut().unwrap().0 = uint() as i64,
            0xF7 => webm.cues.last_mut().unwrap().1 = uint(),
            0xF1 => webm.cues.last_mut().unwrap().2 = uint(),
            _ => {}
        }
        at = body + size as usize;
    }
    webm
}

/// A picture or sound as frames: bytes, when shown (seconds), key or not.
fn frames_of_webm(webm: &Webm, codec: &str) -> Vec<(Vec<u8>, f64, bool)> {
    webm.blocks_of(webm.track(codec).number)
        .iter()
        .map(|b| {
            (
                b.data.clone(),
                b.time as f64 * webm.scale as f64 / 1e9,
                b.key,
            )
        })
        .collect()
}

fn frames_of_mp4(track: &Mp4Track) -> Vec<(Vec<u8>, f64, bool)> {
    track
        .samples
        .iter()
        .map(|s| {
            let shown = s.dts as i64 + i64::from(s.cts);
            (
                s.data.clone(),
                shown as f64 / f64::from(track.timescale),
                s.sync,
            )
        })
        .collect()
}

/// The same frames in the same order, the same bytes, key frames where the
/// input has them, each shown when the input shows it (to the millisecond,
/// counted from the first).
fn same_frames(what: &str, ours: &[(Vec<u8>, f64, bool)], input: &[(Vec<u8>, f64, bool)]) {
    assert_eq!(ours.len(), input.len(), "{what}: frame count");
    for (n, (a, b)) in ours.iter().zip(input).enumerate() {
        assert!(a.0 == b.0, "{what} frame {n}: other bytes");
        assert_eq!(a.2, b.2, "{what} frame {n}: key frame");
        let (at, should) = (a.1 - ours[0].1, b.1 - input[0].1);
        assert!(
            (at - should).abs() < 0.0015,
            "{what} frame {n}: {at} / {should}"
        );
    }
}

// ---------------------------------------------------------------------------
// MPEG transport stream: the PES packets of each stream.

#[derive(Debug)]
struct Pes {
    pts: Option<i64>,
    dts: Option<i64>,
    data: Vec<u8>,
}

/// Each stream's type and PES packets, by the order the PMT lists them.
fn read_ts(path: &Path) -> Vec<(u8, Vec<Pes>)> {
    let data = std::fs::read(path).unwrap();
    assert_eq!(data.len() % 188, 0);
    let mut pmt = None;
    let mut streams: Vec<(u16, u8, Vec<Pes>, Vec<u8>)> = Vec::new();
    let section = |payload: &[u8], start: bool| -> Option<Vec<u8>> {
        if !start {
            return None;
        }
        let skip = usize::from(payload[0]) + 1;
        let table = &payload[skip..];
        let length = (be(table, 1, 2) & 0xfff) as usize;
        // Past the header, without the CRC.
        Some(table[8..3 + length - 4].to_vec())
    };
    let flush = |stream: &mut (u16, u8, Vec<Pes>, Vec<u8>)| {
        let pes = std::mem::take(&mut stream.3);
        if pes.is_empty() {
            return;
        }
        assert_eq!(&pes[..3], &[0, 0, 1], "a PES start");
        let flags = pes[7];
        let header = usize::from(pes[8]);
        let stamp = |at: usize| {
            let b = &pes[at..at + 5];
            (i64::from(b[0] >> 1 & 7) << 30)
                | (i64::from(b[1]) << 22)
                | (i64::from(b[2] >> 1) << 15)
                | (i64::from(b[3]) << 7)
                | i64::from(b[4] >> 1)
        };
        let pts = (flags & 0x80 != 0).then(|| stamp(9));
        let dts = (flags & 0x40 != 0).then(|| stamp(14));
        stream.2.push(Pes {
            pts,
            dts,
            data: pes[9 + header..].to_vec(),
        });
    };
    for packet in data.chunks(188) {
        assert_eq!(packet[0], 0x47, "a sync byte");
        let start = packet[1] & 0x40 != 0;
        let pid = (be(packet, 1, 2) & 0x1fff) as u16;
        let control = packet[3] >> 4 & 3;
        if control & 1 == 0 {
            continue;
        }
        let mut payload = &packet[4..];
        if control & 2 != 0 {
            payload = &payload[1 + usize::from(payload[0])..];
        }
        if pid == 0 {
            if let Some(pat) = section(payload, start) {
                pmt = Some((be(&pat, 2, 2) & 0x1fff) as u16);
            }
        } else if Some(pid) == pmt {
            if let Some(table) = section(payload, start) {
                let info = (be(&table, 2, 2) & 0xfff) as usize;
                let mut at = 4 + info;
                while at + 5 <= table.len() {
                    let kind = table[at];
                    let es = (be(&table, at + 1, 2) & 0x1fff) as u16;
                    if !streams.iter().any(|s| s.0 == es) {
                        streams.push((es, kind, Vec::new(), Vec::new()));
                    }
                    at += 5 + (be(&table, at + 3, 2) & 0xfff) as usize;
                }
            }
        } else if let Some(stream) = streams.iter_mut().find(|s| s.0 == pid) {
            if start {
                flush(stream);
            }
            stream.3.extend_from_slice(payload);
        }
    }
    for stream in &mut streams {
        flush(stream);
    }
    streams.into_iter().map(|s| (s.1, s.2)).collect()
}

/// The NAL units of an Annex B byte stream.
fn nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &from)| {
            let mut to = starts.get(n + 1).map_or(data.len(), |next| next - 3);
            while to > from && data[to - 1] == 0 {
                to -= 1;
            }
            &data[from..to]
        })
        .filter(|u| !u.is_empty())
        .collect()
}

/// A picture's sample as the MP4 form has it: each NAL unit after its
/// length, without the parameter sets (they are in the sample description),
/// access unit delimiters or filler.
fn as_mp4_picture(pes: &[u8], hevc: bool) -> (Vec<u8>, bool) {
    let mut sample = Vec::new();
    let mut key = false;
    for unit in nal_units(pes) {
        let (skip, idr) = if hevc {
            let kind = unit[0] >> 1 & 0x3f;
            (matches!(kind, 32..=35 | 38), (16..=21).contains(&kind))
        } else {
            let kind = unit[0] & 0x1f;
            (matches!(kind, 7 | 8 | 9 | 12), kind == 5)
        };
        if skip {
            continue;
        }
        key |= idr;
        sample.extend_from_slice(&(unit.len() as u32).to_be_bytes());
        sample.extend_from_slice(unit);
    }
    (sample, key)
}

/// An ADTS stream's frames without their headers: the AAC an MP4 carries.
fn adts_frames(stream: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut at = 0;
    while at + 7 <= stream.len() {
        assert!(
            stream[at] == 0xff && stream[at + 1] & 0xf0 == 0xf0,
            "ADTS sync"
        );
        let header = if stream[at + 1] & 1 == 1 { 7 } else { 9 };
        let length = (usize::from(stream[at + 3] & 3) << 11)
            | (usize::from(stream[at + 4]) << 3)
            | usize::from(stream[at + 5] >> 5);
        frames.push(stream[at + header..at + length].to_vec());
        at += length;
    }
    assert_eq!(at, stream.len(), "whole ADTS frames");
    frames
}

/// A transport stream put into an MP4 by dm, held to the stream itself:
/// every picture's bytes and times and every sound frame's bytes, picture
/// and sound as far apart as they were.
fn check_ts_against(ts: &Path, out: &Path, picture_codec: &str, sound_codec: &str) {
    let streams = read_ts(ts);
    let mp4 = read_mp4(out);
    assert_eq!(mp4.tracks.len(), 2, "picture and sound");
    let video = mp4.track("vide");
    let audio = mp4.track("soun");
    assert_eq!(video.codec, picture_codec);
    assert_eq!(audio.codec, sound_codec);
    assert_eq!((video.width, video.height), (160, 90));

    let (kind, pictures) = streams
        .iter()
        .find(|(k, _)| matches!(k, 0x1b | 0x24))
        .unwrap();
    let hevc = *kind == 0x24;
    assert_eq!(
        video.samples.len(),
        pictures.len(),
        "one sample per picture"
    );
    let dts = |p: &Pes| p.dts.or(p.pts).unwrap();
    let ticks = |t: i64| t * i64::from(video.timescale) / 90_000;
    for (n, (sample, pes)) in video.samples.iter().zip(pictures).enumerate() {
        let (bytes, key) = as_mp4_picture(&pes.data, hevc);
        assert!(sample.data == bytes, "picture {n}: other bytes");
        assert_eq!(sample.sync, key, "picture {n}: key frame");
        let pts = pes.pts.unwrap();
        assert_eq!(
            sample.dts as i64 - video.samples[0].dts as i64,
            ticks(dts(pes) - dts(&pictures[0])),
            "picture {n}: decoded when"
        );
        assert_eq!(
            i64::from(sample.cts),
            ticks(pts - dts(pes)),
            "picture {n}: shown when"
        );
    }

    let (kind, sounds) = streams
        .iter()
        .find(|(k, _)| matches!(k, 0x0f | 0x03 | 0x04 | 0x81 | 0x87))
        .unwrap();
    let stream: Vec<u8> = sounds.iter().flat_map(|p| p.data.clone()).collect();
    if *kind == 0x0f {
        let frames = adts_frames(&stream);
        assert_eq!(audio.samples.len(), frames.len());
        for (n, (sample, frame)) in audio.samples.iter().zip(&frames).enumerate() {
            assert!(sample.data == *frame, "sound frame {n}: other bytes");
        }
    } else {
        // The frames go in whole, each one starting at its sync word.
        let ours: Vec<u8> = audio.samples.iter().flat_map(|s| s.data.clone()).collect();
        assert!(ours == stream, "the sound's bytes");
        for (n, sample) in audio.samples.iter().enumerate() {
            let sync = if matches!(kind, 0x81 | 0x87) {
                sample.data[..2] == [0x0b, 0x77]
            } else {
                sample.data[0] == 0xff && sample.data[1] & 0xe0 == 0xe0
            };
            assert!(sync, "sound frame {n} starts mid-frame");
        }
    }
    let seconds = audio.seconds();
    assert!((seconds - 2.0).abs() < 0.1, "sound lasts {seconds}s");

    // Picture and sound keep the distance apart they had in the stream.
    let first_picture = pictures.iter().map(|p| p.pts.unwrap()).min().unwrap();
    let first_sound = sounds[0].pts.unwrap();
    let gap = (first_picture - first_sound) as f64 / 90_000.0;
    let ours = video.start(mp4.timescale) - audio.start(mp4.timescale);
    assert!(
        (ours - gap).abs() < 0.002,
        "picture after sound by {ours}s, in the stream {gap}s"
    );
}

// ---------------------------------------------------------------------------
// The cases.

fn check(fragmented: bool) {
    let (video, audio) = if fragmented {
        (media("picture.f137.mp4"), media("sound.f140.m4a"))
    } else {
        (media("picture.mp4"), media("sound.m4a"))
    };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("joined.mp4");
    dm_core::mp4mux::join(&[video.clone(), audio.clone()], &out).unwrap();

    let ours = read_mp4(&out);
    let kinds: Vec<&str> = ours.tracks.iter().map(|t| t.handler.as_str()).collect();
    assert_eq!(kinds, ["vide", "soun"]);
    same_samples_as_input(ours.track("vide"), read_mp4(&video).track("vide"));
    same_samples_as_input(ours.track("soun"), read_mp4(&audio).track("soun"));
    assert_eq!(ours.track("vide").codec, "avc1");
    assert_eq!(ours.track("soun").codec, "mp4a");
    for track in &ours.tracks {
        // Picture and sound start together: a picture with B-frames has
        // its first one shown two frames in, a sound its encoder's
        // priming to skip, and the edit lists say so.
        let start = track.start(ours.timescale);
        assert!(start.abs() < 0.001, "{} starts at {start}s", track.handler);
        let seconds = track.seconds();
        assert!(
            (seconds - 2.0).abs() < 0.1,
            "{} lasts {seconds}s",
            track.handler
        );
    }
}

#[test]
fn a_dash_picture_and_sound_join_into_one_mp4_that_decodes_the_same() {
    check(true);
}

#[test]
fn plain_files_join_the_same_way() {
    check(false);
}

fn check_ts(segmented: bool) {
    let dir = tempfile::tempdir().unwrap();
    let ts = if segmented {
        // An HLS stream as dm gathers it: its `.ts` segments one after
        // another.
        let joined: Vec<u8> = (0..4)
            .flat_map(|n| std::fs::read(media(&format!("hls/segment{n}.ts"))).unwrap())
            .collect();
        let ts = dir.path().join("joined.ts");
        std::fs::write(&ts, joined).unwrap();
        ts
    } else {
        media("stream.ts")
    };
    let out = dir.path().join("joined.mp4");
    dm_core::tsmux::to_mp4(&ts, &out).unwrap();
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("joined.mp4."))
        .collect();
    assert!(left.is_empty(), "scratch files left: {left:?}");
    check_ts_against(&ts, &out, "avc1", "mp4a");
}

#[test]
fn a_transport_stream_becomes_an_mp4_that_decodes_the_same() {
    check_ts(false);
}

#[test]
fn an_hls_streams_segments_become_one_mp4_that_decodes_the_same() {
    check_ts(true);
}

/// WebM, as a video site serves its VP9 and Opus qualities: picture and
/// sound apart, joined by dm into one WebM with the same frames, and an
/// index a player seeks by.
#[test]
fn a_webm_picture_and_sound_join_into_one_webm_that_decodes_the_same() {
    let video = media("picture.f248.webm");
    let audio = media("sound.f251.webm");
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("joined.webm");
    dm_core::mkvmux::join(&[video.clone(), audio.clone()], &out).unwrap();

    let ours = read_webm(&out);
    assert_eq!(ours.doc_type, "webm");
    let codecs: Vec<&str> = ours.tracks.iter().map(|t| t.codec.as_str()).collect();
    assert_eq!(codecs, ["V_VP9", "A_OPUS"]);
    same_frames(
        "picture",
        &frames_of_webm(&ours, "V_VP9"),
        &frames_of_webm(&read_webm(&video), "V_VP9"),
    );
    same_frames(
        "sound",
        &frames_of_webm(&ours, "A_OPUS"),
        &frames_of_webm(&read_webm(&audio), "A_OPUS"),
    );

    // It seeks: each cue points at a cluster that starts with the
    // picture's key frame at the cue's time, and there are cues past the
    // start.
    let picture = ours.track("V_VP9").number;
    assert!(ours.cues.len() >= 3, "{:?}", ours.cues);
    assert!(ours
        .cues
        .iter()
        .any(|c| c.0 * ours.scale as i64 >= 2_000_000_000));
    for &(time, track, position) in &ours.cues {
        assert_eq!(track, picture);
        let &(_, cluster_time) = ours
            .clusters
            .iter()
            .find(|c| c.0 == position)
            .unwrap_or_else(|| panic!("no cluster at {position}"));
        let block = ours
            .blocks
            .iter()
            .find(|b| b.track == picture && b.time >= cluster_time)
            .unwrap();
        assert!(block.key && block.time == time, "cue {time}: {block:?}");
    }
}

const VTT_EN: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:02.500\nHello <i>there</i>\n\n00:00:03.000 --> 00:00:04.000\nA &amp; B\n";
const VTT_DE: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nHallo\n";

/// An MP4 text track's cues: start and end in milliseconds, and text.
fn mp4_cues(track: &Mp4Track) -> Vec<(u64, u64, String)> {
    let ms = |t: u64| t * 1000 / u64::from(track.timescale);
    track
        .samples
        .iter()
        .filter_map(|s| {
            let length = be(&s.data, 0, 2) as usize;
            (length > 0).then(|| {
                (
                    ms(s.dts),
                    ms(s.dts + u64::from(s.duration)),
                    String::from_utf8(s.data[2..2 + length].to_vec()).unwrap(),
                )
            })
        })
        .collect()
}

/// A WebM subtitle track's cues: start and end in milliseconds, and text.
fn webm_cues(webm: &Webm, track: &WebmTrack) -> Vec<(u64, u64, String)> {
    let ms = |t: u64| t * webm.scale / 1_000_000;
    webm.blocks_of(track.number)
        .iter()
        .map(|b| {
            let start = b.time as u64;
            (
                ms(start),
                ms(start + b.duration.unwrap()),
                String::from_utf8(b.data.clone()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn subtitles_are_carried_in_the_mp4_and_the_webm_dm_joins() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = tempfile::tempdir().unwrap();
    let texts = vec![
        dm_core::subtitles::Subtitle {
            language: "de".into(),
            cues: dm_core::subtitles::parse_vtt(VTT_DE),
        },
        dm_core::subtitles::Subtitle {
            language: "en".into(),
            cues: dm_core::subtitles::parse_vtt(VTT_EN),
        },
    ];
    let picture = fixtures.join("mp4/picture.f137.mp4");
    let sound = fixtures.join("mp4/sound.f140.m4a");
    let mp4 = dir.path().join("joined.mp4");
    dm_core::mp4mux::join_with_text(&[picture.clone(), sound.clone()], &texts, &mp4).unwrap();
    let ours = read_mp4(&mp4);
    let text: Vec<&Mp4Track> = ours.tracks.iter().filter(|t| t.handler == "sbtl").collect();
    let kinds: Vec<(&str, &str)> = text
        .iter()
        .map(|t| (t.codec.as_str(), t.language.as_str()))
        .collect();
    assert_eq!(kinds, [("tx3g", "deu"), ("tx3g", "eng")]);
    assert_eq!(mp4_cues(text[0]), [(1000, 2000, "Hallo".to_owned())]);
    assert_eq!(
        mp4_cues(text[1]),
        [
            (1000, 2500, "Hello there".to_owned()),
            (3000, 4000, "A & B".to_owned())
        ]
    );
    // Picture and sound are still the inputs'.
    same_samples_as_input(ours.track("vide"), read_mp4(&picture).track("vide"));
    same_samples_as_input(ours.track("soun"), read_mp4(&sound).track("soun"));

    let picture = fixtures.join("webm/picture.f247.webm");
    let sound = fixtures.join("webm/sound.f251.webm");
    let webm = dir.path().join("joined.webm");
    dm_core::mkvmux::join_with_text(&[picture.clone(), sound.clone()], &texts, &webm).unwrap();
    let ours = read_webm(&webm);
    let text: Vec<&WebmTrack> = ours.tracks.iter().filter(|t| t.kind == 0x11).collect();
    let kinds: Vec<(&str, &str)> = text
        .iter()
        .map(|t| (t.codec.as_str(), t.language.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [("D_WEBVTT/SUBTITLES", "deu"), ("D_WEBVTT/SUBTITLES", "eng")]
    );
    // A WebVTT block: an empty identifier line, an empty settings line,
    // then the cue's text as WebVTT writes it, its markup and escapes kept.
    assert_eq!(
        webm_cues(&ours, text[0]),
        [(1000, 2000, "\n\nHallo".to_owned())]
    );
    assert_eq!(
        webm_cues(&ours, text[1]),
        [
            (1000, 2500, "\n\nHello <i>there</i>".to_owned()),
            (3000, 4000, "\n\nA &amp; B".to_owned())
        ]
    );
    for codec in ["V_VP9", "A_OPUS"] {
        let input = if codec == "V_VP9" { &picture } else { &sound };
        same_frames(
            codec,
            &frames_of_webm(&ours, codec),
            &frames_of_webm(&read_webm(input), codec),
        );
    }
}

/// A transport stream of the given kinds, put into an MP4 by dm: every
/// picture and sound frame is the stream's.
fn check_ts_kind(name: &str, picture_codec: &str, sound_codec: &str) {
    let ts = media(name);
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("joined.mp4");
    dm_core::tsmux::to_mp4(&ts, &out).unwrap_or_else(|e| panic!("{name}: {e}"));
    check_ts_against(&ts, &out, picture_codec, sound_codec);
}

#[test]
fn a_transport_stream_with_mp3_sound_becomes_an_mp4() {
    check_ts_kind("mp3.ts", "avc1", "mp4a");
}

#[test]
fn a_transport_stream_with_mp2_sound_becomes_an_mp4() {
    check_ts_kind("mp2.ts", "avc1", "mp4a");
}

#[test]
fn a_transport_stream_with_ac3_sound_becomes_an_mp4() {
    check_ts_kind("ac3.ts", "avc1", "ac-3");
}

#[test]
fn a_transport_stream_with_hevc_pictures_becomes_an_mp4() {
    check_ts_kind("hevc.ts", "hvc1", "mp4a");
}

#[test]
fn a_transport_stream_with_eac3_sound_becomes_an_mp4() {
    check_ts_kind("eac3.ts", "avc1", "ec-3");
}

/// A WebM picture with its sound into one MP4, as when a video's tallest
/// picture comes only as WebM: VP9 with AAC, AV1 with Opus.
fn check_webm_into_mp4(video: &str, audio: &str, picture_codec: &str, sound_codec: &str) {
    let (video, audio) = (media(video), media(audio));
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("joined.mp4");
    dm_core::mp4mux::join(&[video.clone(), audio.clone()], &out)
        .unwrap_or_else(|e| panic!("{picture_codec}/{sound_codec}: {e}"));
    let ours = read_mp4(&out);
    assert_eq!(ours.tracks.len(), 2);
    let picture = ours.track("vide");
    let sound = ours.track("soun");
    assert_eq!(
        (picture.codec.as_str(), sound.codec.as_str()),
        (picture_codec, sound_codec)
    );
    let input = read_webm(&video);
    let webm_codec = &input.tracks[0].codec;
    same_frames(
        "picture",
        &frames_of_mp4(picture),
        &frames_of_webm(&input, webm_codec),
    );
    if audio.extension().is_some_and(|e| e == "webm") {
        let input = read_webm(&audio);
        let webm_codec = &input.tracks[0].codec;
        same_frames(
            "sound",
            &frames_of_mp4(sound),
            &frames_of_webm(&input, webm_codec),
        );
    } else {
        same_samples_as_input(sound, read_mp4(&audio).track("soun"));
    }
    for track in &ours.tracks {
        let start = track.start(ours.timescale);
        assert!(start.abs() < 0.03, "{} starts at {start}s", track.handler);
    }
}

#[test]
fn a_vp9_webm_picture_and_aac_sound_join_into_one_mp4() {
    check_webm_into_mp4("vp9.f313.webm", "vp9-sound.f140.m4a", "vp09", "mp4a");
}

#[test]
fn an_av1_webm_picture_and_opus_sound_join_into_one_mp4() {
    check_webm_into_mp4("av1.f399.webm", "av1-sound.f251.webm", "av01", "Opus");
}
