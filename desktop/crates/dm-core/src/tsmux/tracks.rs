//! Making the MP4 tracks from what was read, and writing the file.

use super::*;

/// An HEVC picture's sample entry: `hvc1`, its parameter sets in `hvcC`
/// (and none in the samples), and its size.
pub(super) fn hevc_entry(stream: &Elementary) -> Result<(Vec<u8>, u32, u32), String> {
    let (Some(vps), Some(sps), Some(pps)) = (&stream.vps, &stream.sps, &stream.pps) else {
        return Err("its pictures have no parameter sets".into());
    };
    let info = hevc_sps(sps).ok_or("its sequence parameter set could not be read")?;
    let mut hvcc = vec![1];
    hvcc.extend_from_slice(&info.profile);
    hvcc.extend_from_slice(&[0xf0, 0x00, 0xfc]);
    hvcc.push(0xfc | (info.chroma & 3) as u8);
    hvcc.push(0xf8 | (info.luma_bits.saturating_sub(8) & 7) as u8);
    hvcc.push(0xf8 | (info.chroma_bits.saturating_sub(8) & 7) as u8);
    hvcc.extend_from_slice(&[0, 0]);
    hvcc.push((((info.sub_layers & 7) << 3) as u8) | (u8::from(info.nested) << 2) | 3);
    hvcc.push(3);
    for (kind, unit) in [(32u8, vps), (33, sps), (34, pps)] {
        hvcc.push(0x80 | kind);
        hvcc.extend_from_slice(&1u16.to_be_bytes());
        hvcc.extend_from_slice(&(unit.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(unit);
    }
    Ok((boxed(b"hvcC", &[&hvcc]), info.width, info.height))
}

pub(super) fn video_track(stream: &Elementary, input: usize, lead: i64) -> Result<Track, String> {
    let (config, codec, width, height) = if stream.kind == HEVC {
        let (hvcc, width, height) = hevc_entry(stream)?;
        (hvcc, *b"hvc1", width, height)
    } else {
        let (avcc, width, height) = avc_entry(stream)?;
        (avcc, *b"avc1", width, height)
    };
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
    let sample_entry = boxed(&codec, &[&entry, &config]);
    let stsd = full(
        b"stsd",
        0,
        0,
        &[&1u32.to_be_bytes()[..], &sample_entry].concat(),
    );
    picture_track(stream, input, lead, stsd, width, height)
}

/// An H.264 picture's `avcC` and size.
pub(super) fn avc_entry(stream: &Elementary) -> Result<(Vec<u8>, u32, u32), String> {
    let (Some(sps), Some(pps)) = (&stream.sps, &stream.pps) else {
        return Err("its pictures have no parameter sets".into());
    };
    let (width, height) = picture_size(sps).unwrap_or((0, 0));
    let mut avcc = vec![
        1,
        sps.get(1).copied().unwrap_or(0),
        sps.get(2).copied().unwrap_or(0),
        sps.get(3).copied().unwrap_or(0),
        0xff,
        0xe1,
    ];
    avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    avcc.extend_from_slice(sps);
    avcc.push(1);
    avcc.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    avcc.extend_from_slice(pps);
    Ok((boxed(b"avcC", &[&avcc]), width, height))
}

pub(super) fn picture_track(
    stream: &Elementary,
    input: usize,
    lead: i64,
    stsd: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<Track, String> {
    let first_dts = stream.samples.first().map_or(0, |s| s.3);
    let mut durations: Vec<u32> = stream
        .samples
        .windows(2)
        .map(|w| match w {
            [a, b] => u32::try_from(b.3 - a.3).unwrap_or(0),
            _ => 0,
        })
        .collect();
    // A picture's length where its timestamps do not say (the last, or a
    // stamp out of step): the most common one.
    let usual = {
        let mut sorted = durations.clone();
        sorted.sort_unstable();
        sorted
            .get(sorted.len() / 2)
            .copied()
            .filter(|d| *d > 0)
            .unwrap_or(3000)
    };
    for d in &mut durations {
        if *d == 0 || *d > usual * 10 {
            *d = usual;
        }
    }
    durations.push(usual);
    let samples = stream
        .samples
        .iter()
        .zip(durations)
        .map(|(s, duration)| Sample {
            offset: s.0,
            size: s.1,
            duration,
            cts: i32::try_from(s.2 - s.3).unwrap_or(0),
            sync: s.4,
        })
        .collect();
    let shown = stream.first_pts.unwrap_or(first_dts) - first_dts;
    let mut edits = Vec::new();
    if lead > 0 {
        edits.push(Edit {
            duration: lead as u64,
            media_time: -1,
        });
    }
    edits.push(Edit {
        duration: 0,
        media_time: shown.max(0),
    });
    Ok(Track {
        input,
        id: 1,
        tkhd_tail: tkhd_tail(0, width, height),
        timescale: TIMESCALE,
        language: 0x55c4,
        hdlr: hdlr(b"vide", "VideoHandler"),
        media_header: full(b"vmhd", 0, 1, &[0u8; 8]),
        stsd,
        edits,
        edits_timescale: TIMESCALE,
        samples,
        start: None,
    })
}

pub(super) const RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

pub(super) fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag, body.len() as u8];
    out.extend_from_slice(body);
    out
}

pub(super) fn audio_track(stream: &Elementary, input: usize, lead: i64) -> Result<Track, String> {
    let shape = stream.sound.ok_or("its sound has no frames")?;
    let es = |object: u8, specific: Option<&[u8]>| {
        let mut decoder = vec![object, 0x15, 0, 0, 0];
        decoder.extend_from_slice(&0u32.to_be_bytes());
        decoder.extend_from_slice(&0u32.to_be_bytes());
        if let Some(specific) = specific {
            decoder.extend(descriptor(5, specific));
        }
        let mut es = vec![0, 0, 0];
        es.extend(descriptor(4, &decoder));
        es.extend(descriptor(6, &[2]));
        full(b"esds", 0, 0, &descriptor(3, &es))
    };
    let (codec, config, rate, channels) = match shape {
        Sound::Aac {
            profile,
            rate_index,
            channels,
        } => {
            let rate = *RATES
                .get(usize::from(rate_index))
                .ok_or("its sound has an unknown sample rate")?;
            let specific = (u16::from(profile + 1) << 11)
                | (u16::from(rate_index) << 7)
                | (u16::from(channels) << 3);
            (
                *b"mp4a",
                es(0x40, Some(&specific.to_be_bytes())),
                rate,
                u16::from(channels),
            )
        }
        // MPEG-1 audio is object type 0x6B; MPEG-2's (and 2.5's) 0x69.
        Sound::Mpeg {
            mpeg1,
            rate,
            channels,
            ..
        } => (
            *b"mp4a",
            es(if mpeg1 { 0x6b } else { 0x69 }, None),
            rate,
            channels,
        ),
        Sound::Ac3 {
            dac3,
            rate,
            channels,
        } => (*b"ac-3", boxed(b"dac3", &[&dac3]), rate, channels),
        Sound::Eac3 {
            dec3,
            rate,
            channels,
            ..
        } => (*b"ec-3", boxed(b"dec3", &[&dec3]), rate, channels),
    };
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 8]);
    entry.extend_from_slice(&channels.clamp(1, 2).to_be_bytes());
    entry.extend_from_slice(&16u16.to_be_bytes());
    entry.extend_from_slice(&[0u8; 4]);
    entry.extend_from_slice(&(rate << 16).to_be_bytes());
    let sample_entry = boxed(&codec, &[&entry, &config]);
    let stsd = full(
        b"stsd",
        0,
        0,
        &[&1u32.to_be_bytes()[..], &sample_entry].concat(),
    );
    let per_frame = shape.per_frame();
    let samples = stream
        .samples
        .iter()
        .map(|s| Sample {
            offset: s.0,
            size: s.1,
            duration: per_frame,
            cts: 0,
            sync: true,
        })
        .collect();
    let mut edits = Vec::new();
    if lead > 0 {
        edits.push(Edit {
            duration: lead as u64,
            media_time: -1,
        });
    }
    edits.push(Edit {
        duration: 0,
        media_time: 0,
    });
    Ok(Track {
        input,
        id: 2,
        tkhd_tail: tkhd_tail(0x0100, 0, 0),
        timescale: rate,
        language: 0x55c4,
        hdlr: hdlr(b"soun", "SoundHandler"),
        media_header: full(b"smhd", 0, 0, &[0u8; 4]),
        stsd,
        edits,
        edits_timescale: TIMESCALE,
        samples,
        start: None,
    })
}

pub(super) fn build(streams: &mut [Elementary], out: &Path) -> Result<u64, String> {
    let video = streams
        .iter()
        .position(|s| PICTURES.contains(&s.kind) && !s.samples.is_empty());
    let audio = streams
        .iter()
        .position(|s| SOUNDS.contains(&s.kind) && !s.samples.is_empty());
    if video.is_none() && audio.is_none() {
        return Err("it holds no picture or sound dm can put into an MP4".into());
    }
    // Picture and sound start together: whichever starts later waits.
    let start = |i: Option<usize>| i.and_then(|i| streams.get(i)).and_then(|s| s.first_pts);
    let earliest = [start(video), start(audio)]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(0);
    let mut tracks = Vec::new();
    let mut files = Vec::new();
    for (index, make) in [
        (
            video,
            video_track as fn(&Elementary, usize, i64) -> Result<Track, String>,
        ),
        (audio, audio_track),
    ] {
        let Some(stream) = index.and_then(|i| streams.get(i)) else {
            continue;
        };
        let lead = stream.first_pts.unwrap_or(earliest) - earliest;
        tracks.push(make(stream, files.len(), lead)?);
        files.push(
            File::open(&stream.path)
                .map_err(|e| format!("a scratch file could not be read: {e}"))?,
        );
    }
    crate::mp4mux::write_output(&tracks, &mut files, out)
}
