//! An MP4 written as a live stream arrives, so the part file plays however
//! far it got.
//!
//! A live stream (RTMP, RTSP) has no end known ahead and may be cut short
//! by its server, a pause or a cancel. An MP4's index normally comes from
//! knowing every sample; this writes the fragmented form instead: an empty
//! index (`moov` with `mvex`) once the streams' shapes are known, then a
//! fragment (`moof` and `mdat`) every second or so of what arrived. Every
//! fragment written is playable. When the stream ends the caller has
//! `mp4mux` rewrite it as a plain MP4, which reads fragments.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::mp4mux::{boxed, full, MATRIX};

/// What a track is, for its index entry.
#[derive(Debug, Clone)]
pub struct Spec {
    /// The whole sample entry: `avc1` with its `avcC`, `mp4a` with its
    /// `esds`, and so on.
    pub entry: Vec<u8>,
    pub timescale: u32,
    pub video: bool,
    pub width: u32,
    pub height: u32,
}

struct Pending {
    data: Vec<u8>,
    dts: i64,
    cts: i32,
    key: bool,
    /// Its length where the source says it (a sound frame's samples);
    /// otherwise the gap to the next one's time.
    duration: Option<u32>,
}

struct TrackState {
    spec: Spec,
    pending: Vec<Pending>,
    /// Where the next fragment starts, in its timescale.
    next_dts: Option<i64>,
    last_duration: u32,
}

/// A fragmented MP4 being written.
pub struct Writer {
    out: BufWriter<File>,
    tracks: Vec<TrackState>,
    sequence: u32,
    /// The earliest time of any track, which all start from.
    origin: Option<i64>,
    pub written: u64,
}

const SYNC: u32 = 0x0200_0000;
const NOT_SYNC: u32 = 0x0101_0000;

impl Writer {
    /// Starts the file: its type and an index of `tracks`.
    pub fn create(path: &Path, tracks: Vec<Spec>) -> std::io::Result<Writer> {
        let mut out = BufWriter::new(File::create(path)?);
        let ftyp = boxed(
            b"ftyp",
            &[b"isom", &0x200u32.to_be_bytes(), b"isomiso2iso6avc1mp41"],
        );
        let moov = index(&tracks);
        out.write_all(&ftyp)?;
        out.write_all(&moov)?;
        out.flush()?;
        Ok(Writer {
            out,
            written: (ftyp.len() + moov.len()) as u64,
            tracks: tracks
                .into_iter()
                .map(|spec| TrackState {
                    spec,
                    pending: Vec::new(),
                    next_dts: None,
                    last_duration: 0,
                })
                .collect(),
            sequence: 0,
            origin: None,
        })
    }

    /// One sample of track `track`, decoded at `dts` and shown `cts` later
    /// (both in its timescale).
    pub fn push(
        &mut self,
        track: usize,
        data: Vec<u8>,
        dts: i64,
        cts: i32,
        key: bool,
        duration: Option<u32>,
    ) -> std::io::Result<()> {
        let Some(state) = self.tracks.get_mut(track) else {
            return Ok(());
        };
        // In milliseconds, to compare across timescales.
        let ms = dts * 1000 / i64::from(state.spec.timescale.max(1));
        self.origin = Some(self.origin.map_or(ms, |o| o.min(ms)));
        state.pending.push(Pending {
            data,
            dts,
            cts,
            key,
            duration,
        });
        // A fragment about every second of picture (at a key frame), or of
        // sound where there is no picture.
        let video = self.tracks.iter().any(|t| t.spec.video);
        let due = self.tracks.iter().any(|t| {
            let span = match (t.pending.first(), t.pending.last()) {
                (Some(a), Some(b)) => (b.dts - a.dts) * 1000 / i64::from(t.spec.timescale.max(1)),
                _ => 0,
            };
            if video {
                t.spec.video && span >= 1000 && t.pending.last().is_some_and(|p| p.key)
            } else {
                span >= 1000
            }
        });
        if due {
            self.fragment(false)?;
        }
        Ok(())
    }

    /// Writes what has arrived as one fragment. Each track's last sample
    /// is held back (its length is the gap to the next) unless `all`.
    fn fragment(&mut self, all: bool) -> std::io::Result<()> {
        let origin = self.origin.unwrap_or(0);
        let mut trafs = Vec::new();
        let mut datas: Vec<Vec<u8>> = Vec::new();
        for (index, state) in self.tracks.iter_mut().enumerate() {
            let keep_last = !all && state.pending.iter().any(|p| p.duration.is_none());
            let take = if keep_last {
                state.pending.len().saturating_sub(1)
            } else {
                state.pending.len()
            };
            if take == 0 {
                continue;
            }
            let held: Vec<Pending> = state.pending.split_off(take);
            let samples = std::mem::replace(&mut state.pending, held);
            let next_time = state.pending.first().map(|p| p.dts);
            let mut entries = Vec::new();
            let mut data = Vec::new();
            let scale = i64::from(state.spec.timescale.max(1));
            let start = state
                .next_dts
                .unwrap_or_else(|| samples.first().map_or(0, |p| p.dts) - origin * scale / 1000);
            for (i, sample) in samples.iter().enumerate() {
                let following = samples.get(i + 1).map(|p| p.dts).or(next_time);
                let duration = sample.duration.unwrap_or_else(|| {
                    following
                        .and_then(|n| u32::try_from(n - sample.dts).ok())
                        .filter(|d| *d > 0)
                        .unwrap_or(state.last_duration)
                });
                state.last_duration = duration;
                entries.push((duration, sample.data.len() as u32, sample.key, sample.cts));
                data.extend_from_slice(&sample.data);
            }
            let total: i64 = entries.iter().map(|e| i64::from(e.0)).sum();
            state.next_dts = Some(start + total);
            trafs.push((index as u32 + 1, start.max(0) as u64, entries));
            datas.push(data);
        }
        if trafs.is_empty() {
            return Ok(());
        }
        self.sequence += 1;
        let build = |offsets: &[u32]| {
            let mut parts = vec![full(b"mfhd", 0, 0, &self.sequence.to_be_bytes())];
            for ((id, start, entries), offset) in trafs.iter().zip(offsets) {
                let tfhd = full(b"tfhd", 0, 0x2_0000, &id.to_be_bytes());
                let tfdt = full(b"tfdt", 1, 0, &start.to_be_bytes());
                let mut trun = (entries.len() as u32).to_be_bytes().to_vec();
                trun.extend_from_slice(&offset.to_be_bytes());
                for (duration, size, key, cts) in entries {
                    trun.extend_from_slice(&duration.to_be_bytes());
                    trun.extend_from_slice(&size.to_be_bytes());
                    trun.extend_from_slice(&(if *key { SYNC } else { NOT_SYNC }).to_be_bytes());
                    trun.extend_from_slice(&cts.to_be_bytes());
                }
                let trun = full(b"trun", 1, 0xF01, &trun);
                parts.push(boxed(b"traf", &[&tfhd, &tfdt, &trun]));
            }
            let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
            boxed(b"moof", &refs)
        };
        // Data offsets count from the moof's start, which is as long with
        // any offsets as with none.
        let moof_len = build(&vec![0; trafs.len()]).len() as u32;
        let mut offsets = Vec::new();
        let mut at = moof_len + 8;
        for data in &datas {
            offsets.push(at);
            at += data.len() as u32;
        }
        let moof = build(&offsets);
        let mdat_len: usize = datas.iter().map(Vec::len).sum::<usize>() + 8;
        self.out.write_all(&moof)?;
        self.out.write_all(&(mdat_len as u32).to_be_bytes())?;
        self.out.write_all(b"mdat")?;
        for data in &datas {
            self.out.write_all(data)?;
        }
        // Out to the file now: what is written is what plays if dm stops.
        self.out.flush()?;
        self.written += (moof.len() + mdat_len) as u64;
        Ok(())
    }

    /// Writes everything still held and closes the file.
    pub fn finish(mut self) -> std::io::Result<u64> {
        self.fragment(true)?;
        self.out.flush()?;
        self.out.get_ref().sync_all()?;
        Ok(self.written)
    }
}

/// The index of a fragmented MP4: each track with its sample entry and
/// empty tables, and `mvex` saying they come in fragments.
fn index(tracks: &[Spec]) -> Vec<u8> {
    let mut mvhd = Vec::new();
    mvhd.extend_from_slice(&[0u8; 8]);
    mvhd.extend_from_slice(&1000u32.to_be_bytes());
    mvhd.extend_from_slice(&0u32.to_be_bytes());
    mvhd.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    mvhd.extend_from_slice(&0x0100u16.to_be_bytes());
    mvhd.extend_from_slice(&[0u8; 10]);
    for m in MATRIX {
        mvhd.extend_from_slice(&m.to_be_bytes());
    }
    mvhd.extend_from_slice(&[0u8; 24]);
    mvhd.extend_from_slice(&(tracks.len() as u32 + 1).to_be_bytes());
    let mut parts = vec![full(b"mvhd", 0, 0, &mvhd)];
    let mut trexes = Vec::new();
    for (i, spec) in tracks.iter().enumerate() {
        let id = i as u32 + 1;
        let mut tkhd = vec![0u8; 8];
        tkhd.extend_from_slice(&id.to_be_bytes());
        tkhd.extend_from_slice(&[0u8; 8]);
        tkhd.extend_from_slice(&[0u8; 8]);
        tkhd.extend_from_slice(&0u16.to_be_bytes());
        tkhd.extend_from_slice(&0u16.to_be_bytes());
        tkhd.extend_from_slice(&(if spec.video { 0u16 } else { 0x0100 }).to_be_bytes());
        tkhd.extend_from_slice(&0u16.to_be_bytes());
        for m in MATRIX {
            tkhd.extend_from_slice(&m.to_be_bytes());
        }
        tkhd.extend_from_slice(&(spec.width << 16).to_be_bytes());
        tkhd.extend_from_slice(&(spec.height << 16).to_be_bytes());
        let mut mdhd = vec![0u8; 8];
        mdhd.extend_from_slice(&spec.timescale.to_be_bytes());
        mdhd.extend_from_slice(&0u32.to_be_bytes());
        mdhd.extend_from_slice(&0x55c4u16.to_be_bytes());
        mdhd.extend_from_slice(&0u16.to_be_bytes());
        let mut hdlr = vec![0u8; 4];
        hdlr.extend_from_slice(if spec.video { b"vide" } else { b"soun" });
        hdlr.extend_from_slice(&[0u8; 12]);
        hdlr.extend_from_slice(if spec.video {
            b"VideoHandler\0".as_slice()
        } else {
            b"SoundHandler\0".as_slice()
        });
        let media_header = if spec.video {
            full(b"vmhd", 0, 1, &[0u8; 8])
        } else {
            full(b"smhd", 0, 0, &[0u8; 4])
        };
        let url = full(b"url ", 0, 1, &[]);
        let dinf = boxed(
            b"dinf",
            &[&full(
                b"dref",
                0,
                0,
                &[&1u32.to_be_bytes()[..], &url].concat(),
            )],
        );
        let none = 0u32.to_be_bytes();
        let stbl = boxed(
            b"stbl",
            &[
                &full(
                    b"stsd",
                    0,
                    0,
                    &[&1u32.to_be_bytes()[..], &spec.entry].concat(),
                ),
                &full(b"stts", 0, 0, &none),
                &full(b"stsc", 0, 0, &none),
                &full(b"stsz", 0, 0, &[0u8; 8]),
                &full(b"stco", 0, 0, &none),
            ],
        );
        let minf = boxed(b"minf", &[&media_header, &dinf, &stbl]);
        let mdia = boxed(
            b"mdia",
            &[
                &full(b"mdhd", 0, 0, &mdhd),
                &full(b"hdlr", 0, 0, &hdlr),
                &minf,
            ],
        );
        parts.push(boxed(b"trak", &[&full(b"tkhd", 0, 3, &tkhd), &mdia]));
        let mut trex = id.to_be_bytes().to_vec();
        trex.extend_from_slice(&1u32.to_be_bytes());
        trex.extend_from_slice(&[0u8; 12]);
        trexes.push(full(b"trex", 0, 0, &trex));
    }
    let trex_refs: Vec<&[u8]> = trexes.iter().map(Vec::as_slice).collect();
    parts.push(boxed(b"mvex", &trex_refs));
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    boxed(b"moov", &refs)
}

/// A picture's `avc1` sample entry around its `avcC` record.
pub fn avc1(avcc: &[u8], width: u32, height: u32) -> Vec<u8> {
    visual(b"avc1", &boxed(b"avcC", &[avcc]), width, height)
}

/// A picture's `hvc1` sample entry around its `hvcC` record.
pub fn hvc1(hvcc: &[u8], width: u32, height: u32) -> Vec<u8> {
    visual(b"hvc1", &boxed(b"hvcC", &[hvcc]), width, height)
}

fn visual(kind: &[u8; 4], config: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 16]);
    entry.extend_from_slice(&(width as u16).to_be_bytes());
    entry.extend_from_slice(&(height as u16).to_be_bytes());
    entry.extend_from_slice(&0x0048_0000u32.to_be_bytes());
    entry.extend_from_slice(&0x0048_0000u32.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 32]);
    entry.extend_from_slice(&0x0018u16.to_be_bytes());
    entry.extend_from_slice(&0xffffu16.to_be_bytes());
    boxed(kind, &[&entry, config])
}

/// A sound's `mp4a` sample entry: AAC with its AudioSpecificConfig, or
/// MPEG audio (object type 0x6B) with none.
pub fn mp4a(object: u8, specific: Option<&[u8]>, rate: u32, channels: u16) -> Vec<u8> {
    let descriptor = |tag: u8, body: &[u8]| {
        let mut out = vec![tag, body.len() as u8];
        out.extend_from_slice(body);
        out
    };
    let mut decoder = vec![object, 0x15, 0, 0, 0];
    decoder.extend_from_slice(&[0u8; 8]);
    if let Some(specific) = specific {
        decoder.extend(descriptor(5, specific));
    }
    let mut es = vec![0, 0, 0];
    es.extend(descriptor(4, &decoder));
    es.extend(descriptor(6, &[2]));
    let esds = full(b"esds", 0, 0, &descriptor(3, &es));
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 8]);
    entry.extend_from_slice(&channels.clamp(1, 2).to_be_bytes());
    entry.extend_from_slice(&16u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 4]);
    entry.extend_from_slice(&(rate.min(65535) << 16).to_be_bytes());
    boxed(b"mp4a", &[&entry, &esds])
}

/// An AAC AudioSpecificConfig's sample rate and channels.
pub fn aac_shape(config: &[u8]) -> (u32, u16) {
    const RATES: [u32; 13] = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ];
    let (Some(a), Some(b)) = (config.first(), config.get(1)) else {
        return (44100, 2);
    };
    let index = usize::from(((a & 0x07) << 1) | (b >> 7));
    let channels = u16::from((b >> 3) & 0x0f);
    (RATES.get(index).copied().unwrap_or(44100), channels.max(1))
}
