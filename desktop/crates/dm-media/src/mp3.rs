// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An MP3 encoder of dm's own: MPEG-1 Layer III, constant bit rate, for a
//! video's sound saved as MP3.
//!
//! What it does, in the standard's order (ISO/IEC 11172-3): the polyphase
//! analysis filterbank splits the sound into 32 bands, an MDCT of 18 lines
//! each (long blocks only) with the aliasing between bands taken out, then
//! each granule's 576 lines are quantized at the finest global gain whose
//! Huffman coding fits the frame, and written with the side information a
//! decoder reads. There is no psychoacoustic model and no bit reservoir:
//! every frame carries its own bits, at a rate (192 kbit/s for stereo
//! unless another of the standard's is asked for, [`KBPS`]) where that is
//! transparent enough for speech and music from a video. Its tables are in
//! `mp3/tables.rs`. The sound comes from AAC (an M4A, an MP4) or from Opus
//! (a WebM, where the system decodes Opus: [`takes`]).

pub mod decode;
mod huffman;
mod tables;

use crate::Error;
use huffman::{quantize, write_granule};

/// Whether dm can make an MP3 here: a decoder dm reads AAC with
/// (AudioToolbox on macOS; on Windows Media Foundation's, which a Windows
/// "N" edition lacks until the Media Feature Pack is added; elsewhere the
/// built-in one, `crate::aac`).
pub fn possible() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(windows)]
    {
        crate::mf::aac_decoder()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        true
    }
}

/// Whether dm can make an MP3 from Opus sound here (a WebM's, as YouTube serves its sound):
/// on Windows with Media Foundation's Opus decoder, which Windows 10 and 11 carry. macOS
/// (AudioToolbox reads no WebM) and Linux (no Opus decoder in dm) not yet (gaps row Y25).
pub fn opus_possible() -> bool {
    #[cfg(windows)]
    {
        crate::mf::opus_decoder()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Whether the finished file at `path` is a sound dm makes an MP3 from: AAC in an M4A, an MP4
/// or an ADTS file (by its name, as always), or Opus in a WebM where [`opus_possible`].
pub fn takes(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "m4a" | "mp4" | "aac" => true,
        "webm" | "weba" | "mka" => {
            opus_possible() && crate::mkvmux::sound_codec(path).as_deref() == Some("A_OPUS")
        }
        _ => false,
    }
}

/// The sound of `source` (an M4A, an MP4's sound, or a WebM's Opus) as an MP3's bytes at `kbps`
/// kbit/s (`None`: [`default_kbps`]): read by the system's decoder ([`decode::read`]) and
/// encoded here, its tags first. dm-core's `mp3::convert` writes them to the file.
pub fn convert_bytes(source: &std::path::Path, kbps: Option<u32>) -> Result<Vec<u8>, Error> {
    if crate::mkvmux::is_webm(source) {
        match crate::mkvmux::sound_codec(source).as_deref() {
            Some("A_OPUS") if opus_possible() => {}
            Some("A_OPUS") => {
                return Err("this system has no Opus decoder dm can make an MP3 with".into())
            }
            Some(other) => {
                return Err(format!(
                    "an MP3 is made from AAC or Opus sound, not {}",
                    other.trim_start_matches("A_")
                )
                .into())
            }
            None => return Err("the file has no sound to save as MP3".into()),
        }
    }
    // A wrong rate is refused before the sound is read.
    if let Some(kbps) = kbps {
        Encoder::at(2, 44100, Some(kbps))?;
    }
    let pcm = decode::read(source)?;
    if pcm.samples.is_empty() {
        return Err("the file has no sound to save as MP3".into());
    }
    let mut bytes = id3(&crate::mp4mux::tags_of(source));
    bytes.extend(encode_all(&pcm.samples, pcm.channels, pcm.rate, kbps)?);
    Ok(bytes)
}

/// The whole of `samples` as an MP3 at `kbps` kbit/s (`None`: [`default_kbps`]), on every
/// core: the same bytes as one [`Encoder`] fed them all. Each frame carries its own bits (no
/// reservoir), so the frames are cut into runs, one a core; each run's
/// encoder first takes the two frames before it, whose output is dropped,
/// so its filterbank and MDCT carry what one long run's would, and it
/// counts frames from where its run starts, so the padding falls where
/// one run puts it.
pub fn encode_all(
    samples: &[i16],
    channels: usize,
    rate: u32,
    kbps: Option<u32>,
) -> Result<Vec<u8>, Error> {
    let probe = Encoder::at(channels, rate, kbps)?;
    let whole = FRAME * channels;
    let frames = samples.len().div_ceil(whole);
    let cores = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8);
    if cores < 2 || frames < 64 {
        let mut encoder = probe;
        let mut out = encoder.push(samples);
        out.extend(encoder.finish());
        return Ok(out);
    }
    const LEAD: usize = 2;
    let per = frames.div_ceil(cores);
    let runs: Vec<(usize, usize)> = (0..frames)
        .step_by(per)
        .map(|first| (first, (first + per).min(frames)))
        .collect();
    let slots = 144 * u64::from(probe.bitrate) * 1000;
    let rate_hz = u64::from(probe.rate);
    let pieces: Vec<Result<Vec<u8>, Error>> = std::thread::scope(|scope| {
        let handles: Vec<_> = runs
            .iter()
            .map(|&(first, last)| {
                scope.spawn(move || -> Result<Vec<u8>, Error> {
                    let mut encoder = Encoder::at(channels, rate, kbps)?;
                    let lead = LEAD.min(first);
                    let from = first - lead;
                    encoder.frames = from as u64;
                    encoder.written = from as u64 * slots / rate_hz;
                    let mut out = Vec::new();
                    let mut frame = vec![0i16; whole];
                    for index in from..last {
                        let at = index * whole;
                        let have = samples.len().saturating_sub(at).min(whole);
                        frame[..have].copy_from_slice(&samples[at..at + have]);
                        frame[have..].fill(0);
                        let bytes = encoder.frame(&frame);
                        if index >= first {
                            out.extend(bytes);
                        }
                    }
                    // The filterbank's delay, after the last run.
                    if last == frames {
                        out.extend(encoder.frame(&vec![0; whole]));
                    }
                    Ok(out)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("the MP3 encoder stopped".into()))
            })
            .collect()
    });
    let mut out = Vec::new();
    for piece in pieces {
        out.extend(piece?);
    }
    Ok(out)
}

/// An ID3v2.4 header with `tags`' words (title `TIT2`, artist `TPE1`, date `TDRC`, and where it
/// came from `WOAS`), as every player reads one before an MP3's frames (gaps row Y25); nothing
/// where there are no words.
pub fn id3(tags: &crate::mp4mux::Tags) -> Vec<u8> {
    // A size in seven bits a byte, as ID3v2.4 writes every size.
    fn synchsafe(n: usize) -> [u8; 4] {
        let n = n as u32;
        [
            ((n >> 21) & 0x7f) as u8,
            ((n >> 14) & 0x7f) as u8,
            ((n >> 7) & 0x7f) as u8,
            (n & 0x7f) as u8,
        ]
    }
    let mut frames = Vec::new();
    let mut frame = |id: &[u8; 4], body: Vec<u8>| {
        frames.extend_from_slice(id);
        frames.extend_from_slice(&synchsafe(body.len()));
        frames.extend_from_slice(&[0, 0]);
        frames.extend(body);
    };
    for (id, words) in [
        (b"TIT2", &tags.title),
        (b"TPE1", &tags.artist),
        (b"TDRC", &tags.date),
    ] {
        let words = words.trim();
        if !words.is_empty() {
            // Text in UTF-8 (encoding 3).
            frame(id, [&[3u8][..], words.as_bytes()].concat());
        }
    }
    let source = tags.source.trim();
    if source.starts_with("https://") || source.starts_with("http://") {
        frame(b"WOAS", source.as_bytes().to_vec());
    }
    // Its picture, as the front cover (type 3), where it is a JPEG or a PNG (gaps row Y11).
    if let Some(kind) = crate::mp4mux::picture_kind(&tags.cover) {
        let mut body = vec![0u8]; // the description's encoding: Latin-1, and it is empty
        body.extend_from_slice(kind.as_bytes());
        body.extend_from_slice(&[0, 3, 0]);
        body.extend_from_slice(&tags.cover);
        frame(b"APIC", body);
    }
    if frames.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(10 + frames.len());
    out.extend_from_slice(b"ID3\x04\x00\x00");
    out.extend_from_slice(&synchsafe(frames.len()));
    out.extend(frames);
    out
}

use std::f64::consts::PI;

/// MPEG-1 Layer III bit rates, kbit/s, by their index (0 is "free format", never written).
const BITRATES: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];

/// The bit rates an MP3 can be asked for, kbit/s: every one MPEG-1 Layer III has
/// (ISO/IEC 11172-3), for one channel or two.
pub const KBPS: [u32; 14] = [
    32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];

/// The bit rate an MP3 is made at when none is asked for, as dm always made it: 192 kbit/s
/// for two channels, 128 for one.
pub fn default_kbps(channels: usize) -> u32 {
    if channels == 2 {
        192
    } else {
        128
    }
}
/// The largest value a Huffman table can carry: 15 and 13 linbits.
const MAX_VALUE: i32 = 15 + 8191;

/// Samples a frame holds, per channel.
pub const FRAME: usize = 1152;

/// An encoder for one stream of 16-bit samples.
pub struct Encoder {
    channels: usize,
    rate_index: u32,
    bitrate_index: u32,
    rate: u32,
    bitrate: u32,
    /// Each channel's filterbank input, newest first.
    history: Vec<[f64; 512]>,
    /// Each channel's last granule of band samples, for the MDCT's overlap.
    overlap: Vec<[[f64; 18]; 32]>,
    /// Each channel's last global gain, where the next granule's search
    /// starts.
    gains: Vec<Option<u32>>,
    /// Bytes written so far, to keep the padding even.
    written: u64,
    frames: u64,
    /// Samples waiting for a whole frame, interleaved.
    pending: Vec<i16>,
    /// The analysis filterbank's cosine matrix.
    matrix: Box<[[f64; 64]; 32]>,
    /// The MDCT's sine window times cosine, 18 by 36.
    mdct: Box<[[f64; 36]; 18]>,
}

impl Encoder {
    /// An encoder for `channels` (1 or 2) at `rate` (32000, 44100 or 48000), at the bit rate
    /// dm makes an MP3 at when none is asked for ([`default_kbps`]).
    pub fn new(channels: usize, rate: u32) -> Result<Encoder, Error> {
        Encoder::at(channels, rate, None)
    }

    /// [`Encoder::new`] at `kbps` kbit/s, one of [`KBPS`] (`None`: [`default_kbps`]).
    pub fn at(channels: usize, rate: u32, kbps: Option<u32>) -> Result<Encoder, Error> {
        let rate_index = match rate {
            44100 => 0,
            48000 => 1,
            32000 => 2,
            _ => return Err(format!("MP3 is made at 32, 44.1 or 48 kHz, not {rate} Hz").into()),
        };
        if !(1..=2).contains(&channels) {
            return Err(format!("MP3 holds one or two channels, not {channels}").into());
        }
        let bitrate = kbps.unwrap_or(default_kbps(channels));
        let Some(bitrate_index) = BITRATES.iter().skip(1).position(|b| *b == bitrate) else {
            return Err(format!(
                "an MP3 is made at 32 to 320 kbit/s ({}), not {bitrate}",
                KBPS.map(|k| k.to_string()).join(", ")
            )
            .into());
        };
        let bitrate_index = bitrate_index as u32 + 1;
        let mut matrix = Box::new([[0.0; 64]; 32]);
        for (k, row) in matrix.iter_mut().enumerate() {
            for (i, m) in row.iter_mut().enumerate() {
                *m = ((2 * k + 1) as f64 * (i as f64 - 16.0) * PI / 64.0).cos();
            }
        }
        let mut mdct = Box::new([[0.0; 36]; 18]);
        for (m, row) in mdct.iter_mut().enumerate() {
            for (i, v) in row.iter_mut().enumerate() {
                let window = (PI / 36.0 * (i as f64 + 0.5)).sin();
                // Over 9 (36 / 4): the decoder's inverse MDCT is not
                // normalized, and the pair would otherwise play 9 times
                // as loud (measured with the system's decoder).
                *v = window
                    * (PI / 72.0 * (2.0 * i as f64 + 1.0 + 18.0) * (2.0 * m as f64 + 1.0)).cos()
                    / 9.0;
            }
        }
        Ok(Encoder {
            channels,
            rate_index,
            bitrate_index,
            rate,
            bitrate,
            history: vec![[0.0; 512]; channels],
            overlap: vec![[[0.0; 18]; 32]; channels],
            gains: vec![None; channels],
            written: 0,
            frames: 0,
            pending: Vec::new(),
            matrix,
            mdct,
        })
    }

    /// Takes interleaved samples; returns the frames they complete.
    pub fn push(&mut self, samples: &[i16]) -> Vec<u8> {
        self.pending.extend_from_slice(samples);
        let whole = FRAME * self.channels;
        let mut out = Vec::new();
        while self.pending.len() >= whole {
            let frame: Vec<i16> = self.pending.drain(..whole).collect();
            out.extend(self.frame(&frame));
        }
        out
    }

    /// The last frames: what is waiting, padded with silence, and one more
    /// frame of silence for the filterbank's delay.
    pub fn finish(&mut self) -> Vec<u8> {
        let whole = FRAME * self.channels;
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let mut frame = std::mem::take(&mut self.pending);
            frame.resize(whole, 0);
            out.extend(self.frame(&frame));
        }
        out.extend(self.frame(&vec![0; whole]));
        out
    }

    /// One frame: two granules of 576 samples per channel.
    fn frame(&mut self, samples: &[i16]) -> Vec<u8> {
        // Bytes this frame holds, with padding to keep the average exact.
        let slots = 144 * u64::from(self.bitrate) * 1000;
        let ideal = (self.frames + 1) * slots / u64::from(self.rate);
        let plain = slots / u64::from(self.rate);
        let size = (ideal - self.written).max(plain);
        let padding = size > plain;
        let side = if self.channels == 2 { 32 } else { 17 };
        let main_bits = ((size as usize) - 4 - side) * 8;
        let budget = main_bits / (2 * self.channels);

        let mut granules = Vec::with_capacity(2);
        for gr in 0..2 {
            let mut chans = Vec::with_capacity(self.channels);
            for ch in 0..self.channels {
                let lines = self.lines(samples, gr, ch);
                let g = quantize(&lines, budget, self.rate_index as usize, self.gains[ch]);
                self.gains[ch] = Some(g.global_gain);
                chans.push(g);
            }
            granules.push(chans);
        }

        let mut w = Bits::default();
        // Header.
        w.put(0xFFF, 12);
        w.put(1, 1); // MPEG-1
        w.put(1, 2); // Layer III
        w.put(1, 1); // no CRC
        w.put(self.bitrate_index, 4);
        w.put(self.rate_index, 2);
        w.put(u32::from(padding), 1);
        w.put(0, 1); // private
        w.put(if self.channels == 2 { 0 } else { 3 }, 2);
        w.put(0, 2); // mode extension
        w.put(0, 1); // copyright
        w.put(1, 1); // original
        w.put(0, 2); // emphasis
                     // Side information.
        w.put(0, 9); // main data begins in this frame
        w.put(0, if self.channels == 2 { 3 } else { 5 });
        w.put(0, 4 * self.channels as u32); // no scalefactor sharing
        for granule in &granules {
            for g in granule {
                w.put(g.bits, 12);
                w.put(g.big_values, 9);
                w.put(g.global_gain, 8);
                w.put(0, 4); // scalefac_compress: no scalefactors
                w.put(0, 1); // long blocks
                for table in g.tables {
                    w.put(table, 5);
                }
                w.put(g.region0, 4);
                w.put(g.region1, 3);
                w.put(0, 1); // preflag
                w.put(0, 1); // scalefac_scale
                w.put(g.count1_table, 1);
            }
        }
        // Main data.
        for granule in &granules {
            for g in granule {
                write_granule(&mut w, g);
            }
        }
        let mut bytes = w.finish();
        bytes.resize(size as usize, 0);
        self.written += size;
        self.frames += 1;
        bytes
    }

    /// Granule `gr`'s 576 frequency lines for channel `ch`.
    fn lines(&mut self, samples: &[i16], gr: usize, ch: usize) -> [f64; 576] {
        let channels = self.channels;
        let mut bands = [[0.0f64; 18]; 32];
        for slot in 0..18 {
            // 32 new samples into the filterbank, newest first.
            let history = &mut self.history[ch];
            history.copy_within(0..480, 32);
            for i in 0..32 {
                let at = (gr * 576 + slot * 32 + i) * channels + ch;
                let sample = f64::from(samples.get(at).copied().unwrap_or(0)) / 32768.0;
                history[31 - i] = sample;
            }
            let mut y = [0.0f64; 64];
            for (i, yi) in y.iter_mut().enumerate() {
                let mut sum = 0.0;
                for j in 0..8 {
                    let n = i + 64 * j;
                    sum += tables::WINDOW[n] / 32.0 * history[n];
                }
                *yi = sum;
            }
            for (k, band) in bands.iter_mut().enumerate() {
                let row = &self.matrix[k];
                band[slot] = row.iter().zip(&y).map(|(m, v)| m * v).sum();
            }
        }
        let mut lines = [0.0f64; 576];
        for (k, band) in bands.iter_mut().enumerate() {
            // The odd bands' odd samples inverted, as the decoder undoes.
            if k % 2 == 1 {
                for (slot, v) in band.iter_mut().enumerate() {
                    if slot % 2 == 1 {
                        *v = -*v;
                    }
                }
            }
            let mut input = [0.0f64; 36];
            input[..18].copy_from_slice(&self.overlap[ch][k]);
            input[18..].copy_from_slice(band);
            self.overlap[ch][k] = *band;
            for m in 0..18 {
                lines[k * 18 + m] = self.mdct[m].iter().zip(&input).map(|(c, v)| c * v).sum();
            }
        }
        // The aliasing between neighbouring bands, taken out as the decoder
        // puts it back.
        const CI: [f64; 8] = [
            -0.6, -0.535, -0.33, -0.185, -0.095, -0.041, -0.0142, -0.0037,
        ];
        for sb in 1..32 {
            for (i, c) in CI.iter().enumerate() {
                let cs = 1.0 / (1.0 + c * c).sqrt();
                let ca = c / (1.0 + c * c).sqrt();
                let lower = sb * 18 - 1 - i;
                let upper = sb * 18 + i;
                let (a, b) = (lines[lower], lines[upper]);
                lines[lower] = a * cs + b * ca;
                lines[upper] = b * cs - a * ca;
            }
        }
        lines
    }
}

/// One channel's granule, quantized and ready to write.
struct Granule {
    values: [i32; 576],
    global_gain: u32,
    big_values: u32,
    count1: usize,
    tables: [u32; 3],
    region0: u32,
    region1: u32,
    count1_table: u32,
    /// Where regions 1 and 2 start, in lines: band edges, at most the
    /// pairs' end, as a decoder works them out from region0 and region1.
    edges: (usize, usize),
    /// part2_3_length: the Huffman bits (no scalefactors).
    bits: u32,
}

/// Writes bits, most significant first.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    used: usize,
}

impl Bits {
    fn put(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                if let Some(last) = self.bytes.last_mut() {
                    *last |= 1 << (7 - self.used % 8);
                }
            }
            self.used += 1;
        }
    }

    fn len(&self) -> usize {
        self.used
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests;
