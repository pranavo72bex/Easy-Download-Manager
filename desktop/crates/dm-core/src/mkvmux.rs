//! Joins a WebM video's picture and sound into one file, without ffmpeg.
//!
//! A video site's WebM qualities (VP9 or AV1 picture, Opus sound) come as
//! two WebM files, as its MP4 ones do (`mp4mux`). This reads each input's
//! tracks and blocks — the frames, with when each plays — and writes one
//! WebM: both tracks, the blocks of both in time order in clusters that
//! start at a picture's key frame, an index of those (`Cues`) so a player
//! can seek, and a `SeekHead` to find it. The frames are copied byte for
//! byte, laced or not: nothing is decoded or encoded.
//!
//! The inputs came from a server: nothing here indexes past what it
//! checked, and a file whose blocks point outside it is refused.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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
    /// Cluster time plus the block's own offset, in the input's ticks.
    ticks: i64,
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

impl Reader {
    fn read_at(&mut self, at: u64, len: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; len];
        self.file
            .read_into(at, &mut buf)
            .map_err(|e| e.to_string())?;
        Ok(buf)
    }

    /// The element header at `at`: ID, body start, body end (the end of the
    /// file, or of `within`, for an unknown size).
    fn header(&mut self, at: u64, within: u64) -> Result<(u32, u64, Option<u64>), String> {
        let take = (within.min(self.length).saturating_sub(at)).min(12) as usize;
        let head = self.read_at(at, take)?;
        let (id, id_len) = read_id(&head).ok_or("a damaged element")?;
        let (size, size_len) =
            read_size(head.get(id_len..).unwrap_or_default()).ok_or("a damaged element")?;
        let start = at + (id_len + size_len) as u64;
        Ok((id, start, size.map(|s| start.saturating_add(s))))
    }
}

fn read_input(path: &Path, input: usize) -> Result<Input, String> {
    let length = std::fs::metadata(path)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?
        .len();
    let mut r = Reader {
        file: Positioned::open(path)
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?,
        length,
    };
    let (id, _, end) = r.header(0, length)?;
    if id != EBML {
        return Err("it is not a WebM file".into());
    }
    let mut at = end.ok_or("it is not a WebM file")?;
    let mut segment = None;
    while at < length {
        let (id, start, end) = r.header(at, length)?;
        if id == SEGMENT {
            segment = Some((start, end.unwrap_or(length).min(length)));
            break;
        }
        at = end.ok_or("a damaged element")?;
    }
    let (mut at, segment_end) = segment.ok_or("it has no segment")?;
    let mut scale = SCALE_NS;
    let mut tracks = Vec::new();
    let mut blocks = Vec::new();
    while at + 2 <= segment_end {
        let (id, start, end) = r.header(at, segment_end)?;
        match id {
            INFO | TRACKS => {
                let end = end.ok_or("a damaged element")?.min(segment_end);
                let body = r.read_at(start, (end - start) as usize)?;
                if id == INFO {
                    for (child, value, _) in elements(&body) {
                        if child == TIMECODE_SCALE && uint(value) > 0 {
                            scale = uint(value);
                        }
                    }
                } else {
                    tracks = read_tracks(&body);
                }
                at = end;
            }
            CLUSTER => {
                at = read_cluster(&mut r, input, start, end, segment_end, &mut blocks)?;
            }
            _ => match end {
                Some(end) => at = end,
                None => break,
            },
        }
    }
    if tracks.is_empty() {
        return Err("it has no tracks".into());
    }
    Ok(Input {
        scale,
        tracks,
        blocks,
    })
}

fn read_tracks(body: &[u8]) -> Vec<TrackIn> {
    elements(body)
        .filter(|(id, _, _)| *id == TRACK_ENTRY)
        .map(|(_, entry, _)| {
            let mut track = TrackIn {
                number: 0,
                kind: 0,
                codec: String::new(),
                rest: Vec::new(),
            };
            for (id, value, whole) in elements(entry) {
                match id {
                    TRACK_NUMBER => track.number = uint(value),
                    TRACK_UID => {}
                    _ => {
                        if id == TRACK_TYPE {
                            track.kind = uint(value);
                        }
                        if id == CODEC_ID {
                            track.codec = String::from_utf8_lossy(value).into_owned();
                        }
                        track.rest.extend_from_slice(whole);
                    }
                }
            }
            track
        })
        .filter(|t| t.number > 0)
        .collect()
}

/// A block's header: track number, its offset in ticks, flags, length.
fn block_header(head: &[u8]) -> Option<(u64, i64, u8, usize)> {
    let (track, len) = read_size(head)?;
    let time = i16::from_be_bytes([*head.get(len)?, *head.get(len + 1)?]);
    let flags = *head.get(len + 2)?;
    Some((track?, i64::from(time), flags, len + 3))
}

fn read_cluster(
    r: &mut Reader,
    input: usize,
    start: u64,
    end: Option<u64>,
    segment_end: u64,
    blocks: &mut Vec<Block>,
) -> Result<u64, String> {
    let limit = end.unwrap_or(segment_end).min(segment_end);
    let mut cluster_time = 0i64;
    let mut at = start;
    while at + 2 <= limit {
        let (id, body, body_end) = r.header(at, limit)?;
        if end.is_none() && TOP_LEVEL.contains(&id) {
            // A cluster of unknown size ends where the next one starts.
            return Ok(at);
        }
        let body_end = body_end.ok_or("a damaged block")?;
        if body_end > r.length {
            return Err("it is cut short".into());
        }
        match id {
            TIMECODE => {
                cluster_time = uint(&r.read_at(body, (body_end - body) as usize)?) as i64;
            }
            SIMPLE_BLOCK => {
                let head = r.read_at(body, ((body_end - body) as usize).min(12))?;
                let (track, offset, flags, header) =
                    block_header(&head).ok_or("a damaged block")?;
                blocks.push(Block {
                    input,
                    track,
                    ticks: cluster_time + offset,
                    key: flags & 0x80 != 0,
                    flags,
                    data_at: body + header as u64,
                    data_len: (body_end - body).saturating_sub(header as u64),
                    group: None,
                });
            }
            BLOCK_GROUP => {
                let mut inner = body;
                let mut found = None;
                let mut extra = Vec::new();
                let mut referenced = false;
                while inner + 2 <= body_end {
                    let (child, child_body, child_end) = r.header(inner, body_end)?;
                    let child_end = child_end.ok_or("a damaged block")?.min(body_end);
                    if child == BLOCK {
                        let head =
                            r.read_at(child_body, ((child_end - child_body) as usize).min(12))?;
                        let (track, offset, flags, header) =
                            block_header(&head).ok_or("a damaged block")?;
                        found = Some((
                            track,
                            offset,
                            flags,
                            child_body + header as u64,
                            (child_end - child_body).saturating_sub(header as u64),
                        ));
                    } else {
                        if child == REFERENCE_BLOCK {
                            referenced = true;
                        }
                        extra.extend(r.read_at(inner, (child_end - inner) as usize)?);
                    }
                    inner = child_end;
                }
                if let Some((track, offset, flags, data_at, data_len)) = found {
                    blocks.push(Block {
                        input,
                        track,
                        ticks: cluster_time + offset,
                        key: !referenced,
                        flags,
                        data_at,
                        data_len,
                        group: Some(extra),
                    });
                }
            }
            _ => {}
        }
        at = body_end;
    }
    Ok(limit)
}

// ---- writing ------------------------------------------------------------

/// A block as written: its new track, its time in milliseconds.
struct Placed<'a> {
    block: &'a Block,
    track: u64,
    ms: i64,
    video: bool,
}

/// Joins the WebM files `inputs` into one at `out`. Returns its length.
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
            blocks.push(Block {
                input,
                track: number,
                ticks: i64::try_from(cue.start).unwrap_or(i64::MAX),
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
    }
}

/// [`join`], with `texts` carried as subtitle tracks, one per language.
pub fn join_with_text(
    inputs: &[PathBuf],
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
) -> Result<u64, String> {
    let mut read = Vec::new();
    for (i, path) in inputs.iter().enumerate() {
        let input = read_input(path, i)
            .map_err(|e| format!("{} is not a WebM dm can join: {e}", path.display()))?;
        let length = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        if input.blocks.iter().any(|b| b.data_at + b.data_len > length) {
            return Err(format!("{} is cut short", path.display()));
        }
        read.push(input);
    }
    let mut scratch_name = out.as_os_str().to_owned();
    scratch_name.push(".text");
    let scratch = PathBuf::from(scratch_name);
    let mut sources = inputs.to_vec();
    if !texts.is_empty() {
        let mut data = Vec::new();
        read.push(text_input(texts, inputs.len(), &mut data));
        std::fs::write(&scratch, &data).map_err(|e| format!("subtitles: {e}"))?;
        sources.push(scratch.clone());
    }
    let joined = join_read(&read, &sources, out);
    let _ = std::fs::remove_file(&scratch);
    joined
}

fn join_read(read: &[Input], inputs: &[PathBuf], out: &Path) -> Result<u64, String> {
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
            let ns = i128::from(block.ticks) * i128::from(input.scale);
            placed.push(Placed {
                block,
                track,
                ms: (ns / i128::from(SCALE_NS)) as i64,
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
    placed.sort_by_key(|p| (p.ms, !p.video));
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
    write(out, &order, &placed, &mut files, webm, has_video)
}

/// [`join`], off the async runtime.
pub async fn join_async(inputs: Vec<PathBuf>, out: PathBuf) -> Result<u64, String> {
    join_with_text_async(inputs, Vec::new(), out).await
}

/// [`join_with_text`], off the async runtime.
pub async fn join_with_text_async(
    inputs: Vec<PathBuf>,
    texts: Vec<crate::subtitles::Subtitle>,
    out: PathBuf,
) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join_with_text(&inputs, &texts, &out))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}

fn write(
    out: &Path,
    order: &[(usize, &TrackIn)],
    placed: &[Placed],
    files: &mut [Positioned],
    webm: bool,
    has_video: bool,
) -> Result<u64, String> {
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
            element(MUXING_APP, b"dm"),
            element(WRITING_APP, b"dm"),
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

    // Clusters: a new one at a picture's key frame a second or more in, or
    // before a block's offset would outgrow its 16 bits.
    let mut cues = Vec::new();
    let mut i = 0;
    let mut buffer = vec![0u8; 1 << 20];
    while let Some(first) = placed.get(i) {
        let start = first.ms.max(0);
        let mut end = i + 1;
        while let Some(p) = placed.get(end) {
            let span = p.ms - start;
            let cut = if has_video {
                p.video && p.block.key && span >= 1000
            } else {
                span >= 5000
            };
            if cut || span >= CLUSTER_MS {
                break;
            }
            end += 1;
        }
        let blocks = placed.get(i..end).unwrap_or_default();
        if (has_video && first.video && first.block.key) || !has_video {
            cues.push((start, first.track, at));
        }
        // The cluster's length, then it, block by block.
        let block_len = |p: &Placed| -> u64 {
            let simple = 4 + p.block.data_len;
            match &p.block.group {
                None => 1 + 8 + simple,
                Some(extra) => 1 + 8 + (1 + 8 + simple) + extra.len() as u64,
            }
        };
        let timecode = uint_element(TIMECODE, start as u64);
        let body: u64 = timecode.len() as u64 + blocks.iter().map(block_len).sum::<u64>();
        w.write_all(&id_bytes(CLUSTER)).map_err(wrote)?;
        w.write_all(&size8(body)).map_err(wrote)?;
        w.write_all(&timecode).map_err(wrote)?;
        for p in blocks {
            let offset = i16::try_from(p.ms - start).map_err(|_| "a block out of order")?;
            let mut head = vec![0x80 | p.track as u8];
            head.extend_from_slice(&offset.to_be_bytes());
            head.push(p.block.flags);
            match &p.block.group {
                None => {
                    w.write_all(&[SIMPLE_BLOCK as u8]).map_err(wrote)?;
                    w.write_all(&size8(4 + p.block.data_len)).map_err(wrote)?;
                }
                Some(extra) => {
                    w.write_all(&[BLOCK_GROUP as u8]).map_err(wrote)?;
                    w.write_all(&size8(1 + 8 + 4 + p.block.data_len + extra.len() as u64))
                        .map_err(wrote)?;
                    w.write_all(&[BLOCK as u8]).map_err(wrote)?;
                    w.write_all(&size8(4 + p.block.data_len)).map_err(wrote)?;
                }
            }
            w.write_all(&head).map_err(wrote)?;
            let file = files
                .get_mut(p.block.input)
                .ok_or("an input went missing")?;
            let mut left = p.block.data_len;
            let mut from = p.block.data_at;
            while left > 0 {
                let take = left.min(buffer.len() as u64) as usize;
                let part = buffer.get_mut(..take).ok_or("a read went wrong")?;
                file.read_into(from, part)
                    .map_err(|e| format!("an input could not be read: {e}"))?;
                from += take as u64;
                w.write_all(part).map_err(wrote)?;
                left -= take as u64;
            }
            // A group's other elements after its block, which comes first.
            if let Some(extra) = &p.block.group {
                w.write_all(extra).map_err(wrote)?;
            }
        }
        at += 4 + 8 + body;
        i = end;
    }

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

// ---- a WebM's tracks for an MP4 ------------------------------------------

/// Whether the file at `path` starts as an EBML (WebM, Matroska) file.
pub(crate) fn is_webm(path: &Path) -> bool {
    let mut head = [0u8; 4];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && u32::from_be_bytes(head) == EBML
}

/// An element's body among `elements`, by its ID.
fn find(elements: &[u8], id: u32) -> Option<&[u8]> {
    self::elements(elements)
        .find(|(i, _, _)| *i == id)
        .map(|(_, body, _)| body)
}

/// A VP9 key frame's profile, bit depth and chroma subsampling, from its
/// uncompressed header, for the `vpcC` an MP4 names them in.
fn vp9_shape(frame: &[u8]) -> Option<(u8, u8, u8)> {
    let mut bits = BitReader { data: frame, at: 0 };
    if bits.take(2)? != 2 {
        return None;
    }
    let low = bits.take(1)?;
    let high = bits.take(1)?;
    let profile = (high << 1 | low) as u8;
    if profile == 3 {
        bits.take(1)?;
    }
    if bits.take(1)? == 1 {
        return None; // shows an earlier frame: no header here
    }
    if bits.take(1)? != 0 {
        return None; // not a key frame
    }
    bits.take(2)?; // show_frame, error_resilient_mode
    if bits.take(24)? != 0x49_8342 {
        return None;
    }
    let depth = if profile >= 2 {
        if bits.take(1)? == 1 {
            12
        } else {
            10
        }
    } else {
        8
    };
    let space = bits.take(3)?;
    // 1 is 4:2:0 with chroma beside the first luma sample, what VP9 uses.
    let mut chroma = 1;
    if space != 7 {
        bits.take(1)?; // colour range
        if profile == 1 || profile == 3 {
            let x = bits.take(1)?;
            let y = bits.take(1)?;
            chroma = match (x, y) {
                (1, 0) => 2,
                (0, 0) => 3,
                _ => 1,
            };
        }
    } else {
        chroma = 3;
    }
    Some((profile, depth, chroma))
}

struct BitReader<'a> {
    data: &'a [u8],
    at: usize,
}

impl BitReader<'_> {
    fn take(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.data.get(self.at / 8)?;
            v = (v << 1) | u32::from((byte >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Some(v)
    }
}

/// An Opus packet's length in 48 kHz samples, from its table of contents.
fn opus_samples(packet: &[u8]) -> u32 {
    let Some(&toc) = packet.first() else {
        return 960;
    };
    let config = toc >> 3;
    // In tenths of a millisecond.
    let tenths: u32 = match config {
        0..=11 => [100, 200, 400, 600]
            .get(usize::from(config % 4))
            .copied()
            .unwrap_or(200),
        12..=15 => [100, 200]
            .get(usize::from(config % 2))
            .copied()
            .unwrap_or(200),
        _ => [25, 50, 100, 200]
            .get(usize::from(config % 4))
            .copied()
            .unwrap_or(200),
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => u32::from(packet.get(1).copied().unwrap_or(1) & 0x3f).max(1),
    };
    tenths * 48 / 10 * frames
}

/// The tracks of the WebM at `path` as MP4 tracks whose samples are its
/// frames, where they are: VP9 (`vp09` with `vpcC`), AV1 (`av01` with the
/// `av1C` WebM keeps as its private data) and Opus (`Opus` with `dOps`).
/// What ffmpeg did when a video's tallest picture came only as WebM and was
/// joined into an MP4. Another codec, or frames laced together, is refused.
pub(crate) fn mp4_tracks(path: &Path, input: usize) -> Result<Vec<crate::mp4mux::Track>, String> {
    use crate::mp4mux::{full, Sample, Track};
    let read = read_input(path, input)?;
    let mut tracks = Vec::new();
    for track in &read.tracks {
        let blocks: Vec<&Block> = read
            .blocks
            .iter()
            .filter(|b| b.track == track.number)
            .collect();
        if blocks.is_empty() {
            continue;
        }
        if blocks.iter().any(|b| b.flags & 0x06 != 0) {
            return Err("its frames are laced together".into());
        }
        let private = find(&track.rest, 0x63A2).unwrap_or_default();
        let ms = |b: &Block| (i128::from(b.ticks) * i128::from(read.scale) / 1_000_000) as i64;
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let mut first = |b: &Block, n: usize| -> Vec<u8> {
            let mut buf = vec![0u8; (b.data_len as usize).min(n)];
            let ok = file
                .seek(SeekFrom::Start(b.data_at))
                .and_then(|_| file.read_exact(&mut buf));
            if ok.is_ok() {
                buf
            } else {
                Vec::new()
            }
        };
        match track.codec.as_str() {
            "V_VP9" | "V_AV1" => {
                let video = find(&track.rest, 0xE0).unwrap_or_default();
                let width = find(video, 0xB0).map_or(0, uint) as u32;
                let height = find(video, 0xBA).map_or(0, uint) as u32;
                let entry = if track.codec == "V_VP9" {
                    let key = blocks.iter().find(|b| b.key).copied();
                    let (profile, depth, chroma) = key
                        .and_then(|b| vp9_shape(&first(b, 64)))
                        .unwrap_or((0, 8, 1));
                    // Level unknown (0 says so); colour unspecified (2).
                    let mut vpcc = vec![profile, 0, (depth << 4) | (chroma << 1), 2, 2, 2];
                    vpcc.extend_from_slice(&0u16.to_be_bytes());
                    crate::fmp4::visual(b"vp09", &full(b"vpcC", 1, 0, &vpcc), width, height)
                } else {
                    if private.len() < 4 {
                        return Err("its AV1 has no configuration".into());
                    }
                    crate::fmp4::visual(
                        b"av01",
                        &crate::mp4mux::boxed(b"av1C", &[private]),
                        width,
                        height,
                    )
                };
                let mut samples = Vec::with_capacity(blocks.len());
                for (i, b) in blocks.iter().enumerate() {
                    let next = blocks.get(i + 1).map(|n| ms(n));
                    let duration = next.map_or(0, |n| (n - ms(b)).max(0)) as u32;
                    samples.push(Sample {
                        offset: b.data_at,
                        size: b.data_len as u32,
                        duration,
                        cts: 0,
                        sync: b.key,
                    });
                }
                // The last lasts as long as the one before it.
                let usual = samples
                    .iter()
                    .rev()
                    .nth(1)
                    .map_or(33, |s| s.duration.max(1));
                if let Some(last) = samples.last_mut() {
                    last.duration = usual;
                }
                tracks.push(Track {
                    input,
                    id: track.number as u32,
                    tkhd_tail: crate::tsmux::tkhd_tail(0, width, height),
                    timescale: 1000,
                    language: 0x55c4,
                    hdlr: crate::tsmux::hdlr(b"vide", "VideoHandler"),
                    media_header: full(b"vmhd", 0, 1, &[0u8; 8]),
                    stsd: full(b"stsd", 0, 0, &[&1u32.to_be_bytes()[..], &entry].concat()),
                    edits: Vec::new(),
                    edits_timescale: 1000,
                    samples,
                    start: blocks.first().map(|b| ms(b).max(0) as u64),
                });
            }
            "A_OPUS" => {
                // OpusHead, little-endian, as WebM keeps it; dOps is the same
                // fields big-endian, without the magic.
                let head = private
                    .get(8..)
                    .filter(|h| h.len() >= 11)
                    .ok_or("its Opus has no header")?;
                let byte = |i: usize| head.get(i).copied().unwrap_or(0);
                let channels = byte(1);
                let pre_skip = u16::from_le_bytes([byte(2), byte(3)]);
                let rate = u32::from_le_bytes([byte(4), byte(5), byte(6), byte(7)]);
                let gain = i16::from_le_bytes([byte(8), byte(9)]);
                let mut dops = vec![0, channels];
                dops.extend_from_slice(&pre_skip.to_be_bytes());
                dops.extend_from_slice(&rate.to_be_bytes());
                dops.extend_from_slice(&gain.to_be_bytes());
                dops.extend_from_slice(head.get(10..).unwrap_or_default());
                let mut entry = vec![0u8; 6];
                entry.extend_from_slice(&1u16.to_be_bytes());
                entry.extend_from_slice(&[0u8; 8]);
                entry.extend_from_slice(&u16::from(channels.max(1)).to_be_bytes());
                entry.extend_from_slice(&16u16.to_be_bytes());
                entry.extend_from_slice(&[0u8; 4]);
                entry.extend_from_slice(&(48_000u32 << 16).to_be_bytes());
                let opus = crate::mp4mux::boxed(
                    b"Opus",
                    &[&entry, &crate::mp4mux::boxed(b"dOps", &[&dops])],
                );
                let mut samples = Vec::with_capacity(blocks.len());
                for b in &blocks {
                    samples.push(Sample {
                        offset: b.data_at,
                        size: b.data_len as u32,
                        duration: opus_samples(&first(b, 2)),
                        cts: 0,
                        sync: true,
                    });
                }
                tracks.push(Track {
                    input,
                    id: track.number as u32,
                    tkhd_tail: crate::tsmux::tkhd_tail(0x0100, 0, 0),
                    timescale: 48_000,
                    language: 0x55c4,
                    hdlr: crate::tsmux::hdlr(b"soun", "SoundHandler"),
                    media_header: full(b"smhd", 0, 0, &[0u8; 4]),
                    stsd: full(b"stsd", 0, 0, &[&1u32.to_be_bytes()[..], &opus].concat()),
                    // The encoder's delay skipped, as dOps says.
                    edits: vec![crate::mp4mux::Edit {
                        duration: 0,
                        media_time: i64::from(pre_skip),
                    }],
                    edits_timescale: 1000,
                    samples,
                    start: blocks.first().map(|b| (ms(b).max(0) as u64) * 48),
                });
            }
            other => return Err(format!("dm does not put {other} into an MP4")),
        }
    }
    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let _ = join(&[path], &dir.path().join("out.webm"));
        }
    }
}
