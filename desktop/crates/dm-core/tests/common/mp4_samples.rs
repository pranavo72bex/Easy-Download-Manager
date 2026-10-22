//! A plain MP4 read back sample by sample, for judging what dm recorded.
//!
//! Written for the tests from the format's own boxes (`stsd`, `stts`,
//! `ctts`, `stss`, `stsz`, `stsc`, `stco`/`co64`), and not shared with
//! dm-core's own reader, so a mistake there cannot hide one in the writer.

#![allow(dead_code)] // each test binary uses a different part of this

/// One sample: its bytes and when it is decoded and shown.
#[derive(Debug, Clone)]
pub struct Sample {
    pub data: Vec<u8>,
    /// Decoding time, in the track's timescale.
    pub dts: u64,
    /// Presentation time minus decoding time.
    pub offset: i64,
    pub duration: u32,
    pub sync: bool,
}

/// One track.
#[derive(Debug, Clone)]
pub struct Track {
    /// `vide` or `soun`.
    pub handler: [u8; 4],
    /// The sample entry's type: `avc1`, `mp4a`, ...
    pub codec: [u8; 4],
    /// The decoder's setup: the `avcC` record, or the AAC
    /// AudioSpecificConfig inside `esds`.
    pub config: Vec<u8>,
    pub timescale: u32,
    pub width: u16,
    pub height: u16,
    pub samples: Vec<Sample>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(b[at..at + 8].try_into().unwrap())
}

/// The boxes laid one after another in `data`: (type, payload).
pub fn boxes(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = u32_at(data, at) as usize;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().unwrap();
        let (header, size) = match size {
            1 => (16, u64_at(data, at + 8) as usize),
            0 => (8, data.len() - at),
            n => (8, n),
        };
        assert!(
            size >= header && at + size <= data.len(),
            "box {} runs past its parent",
            String::from_utf8_lossy(&kind)
        );
        out.push((kind, &data[at + header..at + size]));
        at += size;
    }
    assert_eq!(at, data.len(), "trailing bytes after the last box");
    out
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data)
        .into_iter()
        .find(|(k, _)| k == kind)
        .map(|(_, b)| b)
}

fn path<'a>(data: &'a [u8], kinds: &[&[u8; 4]]) -> &'a [u8] {
    kinds.iter().fold(data, |inside, kind| {
        child(inside, kind)
            .unwrap_or_else(|| panic!("no {} box", String::from_utf8_lossy(&kind[..])))
    })
}

/// The entries of a full box's table: the count after version and flags.
fn table(full: &[u8], width: usize) -> impl Iterator<Item = &[u8]> {
    let count = u32_at(full, 4) as usize;
    assert!(8 + count * width <= full.len(), "a table runs past its box");
    full[8..8 + count * width].chunks(width)
}

/// AudioSpecificConfig out of an `esds` box's descriptors.
fn esds_config(esds: &[u8]) -> Vec<u8> {
    fn descriptor(b: &[u8], at: &mut usize) -> (u8, usize) {
        let tag = b[*at];
        *at += 1;
        let mut len = 0usize;
        loop {
            let byte = b[*at];
            *at += 1;
            len = (len << 7) | usize::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                break;
            }
        }
        (tag, len)
    }
    let mut at = 4; // version and flags
    let (tag, _) = descriptor(esds, &mut at);
    assert_eq!(tag, 0x03, "no ES descriptor");
    let flags = esds[at + 2];
    at += 3;
    if flags & 0x80 != 0 {
        at += 2;
    }
    if flags & 0x40 != 0 {
        at += 1 + usize::from(esds[at]);
    }
    if flags & 0x20 != 0 {
        at += 2;
    }
    let (tag, _) = descriptor(esds, &mut at);
    assert_eq!(tag, 0x04, "no decoder config descriptor");
    assert_eq!(esds[at], 0x40, "not MPEG-4 audio");
    at += 13;
    let (tag, len) = descriptor(esds, &mut at);
    assert_eq!(tag, 0x05, "no decoder specific info");
    esds[at..at + len].to_vec()
}

fn track(file: &[u8], trak: &[u8]) -> Track {
    let mdia = path(trak, &[b"mdia"]);
    let mdhd = path(mdia, &[b"mdhd"]);
    let timescale = if mdhd[0] == 1 {
        u32_at(mdhd, 20)
    } else {
        u32_at(mdhd, 12)
    };
    let handler: [u8; 4] = path(mdia, &[b"hdlr"])[8..12].try_into().unwrap();
    let stbl = path(mdia, &[b"minf", b"stbl"]);

    let stsd = path(stbl, &[b"stsd"]);
    let entries = boxes(&stsd[8..]);
    assert_eq!(entries.len(), 1, "one sample entry");
    let (codec, entry) = entries[0];
    let (fixed, width, height) = match &handler {
        b"vide" => (78, u16_at(entry, 24), u16_at(entry, 26)),
        b"soun" => (28, 0, 0),
        other => panic!("a {} track", String::from_utf8_lossy(other)),
    };
    let inner = boxes(&entry[fixed..]);
    let config = match &codec {
        b"avc1" | b"avc3" => inner
            .iter()
            .find(|(k, _)| k == b"avcC")
            .expect("avcC")
            .1
            .to_vec(),
        b"mp4a" => esds_config(inner.iter().find(|(k, _)| k == b"esds").expect("esds").1),
        _ => Vec::new(),
    };

    let sizes: Vec<usize> = {
        let stsz = path(stbl, &[b"stsz"]);
        let fixed = u32_at(stsz, 4) as usize;
        let count = u32_at(stsz, 8) as usize;
        if fixed != 0 {
            vec![fixed; count]
        } else {
            (0..count)
                .map(|i| u32_at(stsz, 12 + 4 * i) as usize)
                .collect()
        }
    };
    let chunk_offsets: Vec<u64> = match child(stbl, b"stco") {
        Some(stco) => table(stco, 4).map(|e| u64::from(u32_at(e, 0))).collect(),
        None => table(path(stbl, &[b"co64"]), 8)
            .map(|e| u64_at(e, 0))
            .collect(),
    };
    let runs: Vec<(u32, u32)> = table(path(stbl, &[b"stsc"]), 12)
        .map(|e| (u32_at(e, 0), u32_at(e, 4)))
        .collect();
    let durations: Vec<u32> = table(path(stbl, &[b"stts"]), 8)
        .flat_map(|e| std::iter::repeat_n(u32_at(e, 4), u32_at(e, 0) as usize))
        .collect();
    let offsets: Vec<i64> = match child(stbl, b"ctts") {
        Some(ctts) => {
            let signed = ctts[0] == 1;
            table(ctts, 8)
                .flat_map(|e| {
                    let raw = u32_at(e, 4);
                    let v = if signed {
                        i64::from(raw as i32)
                    } else {
                        i64::from(raw)
                    };
                    std::iter::repeat_n(v, u32_at(e, 0) as usize)
                })
                .collect()
        }
        None => vec![0; sizes.len()],
    };
    let syncs: Option<Vec<u32>> =
        child(stbl, b"stss").map(|stss| table(stss, 4).map(|e| u32_at(e, 0)).collect());
    assert_eq!(durations.len(), sizes.len(), "stts and stsz disagree");
    assert_eq!(offsets.len(), sizes.len(), "ctts and stsz disagree");

    // Each chunk's sample count, from the runs of stsc.
    let mut samples = Vec::with_capacity(sizes.len());
    let mut n = 0usize;
    let mut dts = 0u64;
    for (chunk, &offset) in chunk_offsets.iter().enumerate() {
        let number = chunk as u32 + 1;
        let per = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= number)
            .expect("stsc covers every chunk")
            .1;
        let mut at = offset as usize;
        for _ in 0..per {
            let size = sizes[n];
            samples.push(Sample {
                data: file[at..at + size].to_vec(),
                dts,
                offset: offsets[n],
                duration: durations[n],
                sync: syncs.as_ref().is_none_or(|s| s.contains(&(n as u32 + 1))),
            });
            dts += u64::from(durations[n]);
            at += size;
            n += 1;
        }
    }
    assert_eq!(
        n,
        sizes.len(),
        "the chunks hold a different number of samples"
    );
    Track {
        handler,
        codec,
        config,
        timescale,
        width,
        height,
        samples,
    }
}

/// Every track of the plain (unfragmented) MP4 `file`.
pub fn tracks(file: &[u8]) -> Vec<Track> {
    let top = boxes(file);
    assert_eq!(top.first().map(|b| b.0), Some(*b"ftyp"), "no ftyp first");
    assert!(
        !top.iter().any(|(k, _)| k == b"moof"),
        "left fragmented, not rewritten plain"
    );
    let moov = top.iter().find(|(k, _)| k == b"moov").expect("no moov").1;
    boxes(moov)
        .into_iter()
        .filter(|(k, _)| k == b"trak")
        .map(|(_, trak)| track(file, trak))
        .collect()
}

/// The NAL units of a sample in length-prefixed form.
pub fn length_prefixed_units(sample: &[u8], length_size: usize) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < sample.len() {
        let len = sample[at..at + length_size]
            .iter()
            .fold(0usize, |n, &b| (n << 8) | usize::from(b));
        at += length_size;
        out.push(&sample[at..at + len]);
        at += len;
    }
    out
}
