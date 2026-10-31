//! An MP3 encoder of dm's own: MPEG-1 Layer III, constant bit rate, for a
//! video's sound saved as MP3.
//!
//! What it does, in the standard's order (ISO/IEC 11172-3): the polyphase
//! analysis filterbank splits the sound into 32 bands, an MDCT of 18 lines
//! each (long blocks only) with the aliasing between bands taken out, then
//! each granule's 576 lines are quantized at the finest global gain whose
//! Huffman coding fits the frame, and written with the side information a
//! decoder reads. There is no psychoacoustic model and no bit reservoir:
//! every frame carries its own bits, at a rate (192 kbit/s for stereo)
//! where that is transparent enough for speech and music from a video.
//! Its tables are in `mp3/tables.rs`.

pub mod decode;
mod tables;

/// Whether dm can make an MP3 here: the system has a decoder dm reads
/// AAC with (macOS for now).
pub fn possible() -> bool {
    cfg!(target_os = "macos")
}

/// The list line for the sound as MP3, offered after an M4A's: its size
/// about half as much again (192 kbit/s against AAC's usual 128).
pub fn choice_after(m4a_size: Option<u64>) -> Option<crate::ytdlp::Choice> {
    possible().then(|| crate::ytdlp::Choice {
        label: "MP3 audio".into(),
        quality: "audio/mp3".into(),
        size: m4a_size.map(|s| s * 3 / 2),
    })
}

/// Saves the sound of `source` (an M4A, or an MP4's sound) as an MP3 at
/// `target`: read by the system's decoder ([`decode::read`]) and encoded
/// here. Returns the MP3's length.
pub fn convert(source: &std::path::Path, target: &std::path::Path) -> Result<u64, String> {
    let pcm = decode::read(source)?;
    if pcm.samples.is_empty() {
        return Err("the file has no sound to save as MP3".into());
    }
    let bytes = encode_all(&pcm.samples, pcm.channels, pcm.rate)?;
    crate::write_atomic(target, &bytes)
        .map_err(|e| format!("the MP3 could not be written: {e}"))?;
    Ok(bytes.len() as u64)
}

/// The whole of `samples` as an MP3, on every core: the same bytes as one
/// [`Encoder`] fed them all. Each frame carries its own bits (no
/// reservoir), so the frames are cut into runs, one a core; each run's
/// encoder first takes the two frames before it, whose output is dropped,
/// so its filterbank and MDCT carry what one long run's would, and it
/// counts frames from where its run starts, so the padding falls where
/// one run puts it.
pub fn encode_all(samples: &[i16], channels: usize, rate: u32) -> Result<Vec<u8>, String> {
    let probe = Encoder::new(channels, rate)?;
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
    let pieces: Vec<Result<Vec<u8>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = runs
            .iter()
            .map(|&(first, last)| {
                scope.spawn(move || -> Result<Vec<u8>, String> {
                    let mut encoder = Encoder::new(channels, rate)?;
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

/// [`convert`], off the async runtime.
pub async fn convert_async(
    source: std::path::PathBuf,
    target: std::path::PathBuf,
) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || convert(&source, &target))
        .await
        .map_err(|e| format!("making the MP3 stopped: {e}"))?
}

use std::f64::consts::PI;

/// MPEG-1 Layer III bit rates, kbit/s, by their index.
const BITRATES: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
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
    /// An encoder for `channels` (1 or 2) at `rate` (32000, 44100 or 48000).
    pub fn new(channels: usize, rate: u32) -> Result<Encoder, String> {
        let rate_index = match rate {
            44100 => 0,
            48000 => 1,
            32000 => 2,
            _ => return Err(format!("MP3 is made at 32, 44.1 or 48 kHz, not {rate} Hz")),
        };
        if !(1..=2).contains(&channels) {
            return Err(format!("MP3 holds one or two channels, not {channels}"));
        }
        let bitrate = if channels == 2 { 192 } else { 128 };
        let bitrate_index = BITRATES.iter().position(|b| *b == bitrate).unwrap_or(11) as u32;
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

/// The finest quantization of `lines` whose coding fits `budget` bits:
/// searched from `hint` (the channel's last gain) where there is one, a
/// step or two each way, else by halves over the whole range.
fn quantize(lines: &[f64; 576], budget: usize, rate: usize, hint: Option<u32>) -> Granule {
    let budget = budget.min(4095);
    // Each line's magnitude to the 3/4, once: each gain tried only scales it.
    let mut powered = [0f64; 576];
    for (p, x) in powered.iter_mut().zip(lines) {
        *p = x.abs().powf(0.75);
    }
    let fits = |gain: u32| -> Option<Granule> {
        let step = 2f64.powf(-0.1875 * (f64::from(gain) - 210.0));
        let mut values = [0i32; 576];
        for ((v, x), p) in values.iter_mut().zip(lines).zip(&powered) {
            let q = (p * step - 0.0946 + 0.5).floor().max(0.0);
            if q > f64::from(MAX_VALUE) {
                return None;
            }
            let q = q as i32;
            *v = if *x < 0.0 { -q } else { q };
        }
        // A fixed split while searching: the best split codes in no more
        // bits, so a gain that fits this way fits after it too.
        let g = coded(values, gain, rate, true);
        (g.bits as usize <= budget).then_some(g)
    };
    // The smallest gain (finest step) that fits: bits fall as gain rises.
    if let Some(hint) = hint {
        let mut found = None;
        if let Some(g) = fits(hint) {
            // Finer while it still fits.
            found = Some(g);
            let mut gain = hint;
            while gain > 0 {
                match fits(gain - 1) {
                    Some(g) => {
                        found = Some(g);
                        gain -= 1;
                    }
                    None => break,
                }
            }
        } else {
            // Coarser until it fits.
            for gain in hint + 1..=255 {
                if let Some(g) = fits(gain) {
                    found = Some(g);
                    break;
                }
            }
        }
        if let Some(best) = found {
            return coded(best.values, best.global_gain, rate, false);
        }
    }
    let (mut low, mut high) = (0u32, 255u32);
    let mut best = fits(255).unwrap_or_else(|| coded([0; 576], 255, rate, true));
    while low < high {
        let mid = (low + high) / 2;
        match fits(mid) {
            Some(g) => {
                best = g;
                high = mid;
            }
            None => low = mid + 1,
        }
    }
    // The regions split where they cost fewest, at the gain found.
    coded(best.values, best.global_gain, rate, false)
}

/// A split into regions: the tables, region0 and region1 counts, the bits
/// and where regions 1 and 2 start.
type Split = ([u32; 3], u32, u32, u32, (usize, usize));

/// `values` laid out in regions, with the tables that code them in the
/// fewest bits.
fn coded(values: [i32; 576], global_gain: u32, rate: usize, quick: bool) -> Granule {
    let bands = &tables::BANDS[rate];
    // Zeros at the top, then quadruples of 0 and ±1 (count1), then pairs.
    let mut end = 576;
    while end >= 2 && values[end - 1] == 0 && values[end - 2] == 0 {
        end -= 2;
    }
    let mut big = end;
    while big >= 4 && values[big - 4..big].iter().all(|v| v.abs() <= 1) {
        big -= 4;
    }
    let count1 = (end - big) / 4;
    let (count1_table, count1_bits) = count1_cost(&values[big..end]);
    // Each table's bits for each scalefactor band's pairs, worked out once;
    // a split's cost is then sums of them (it was every pair under every
    // table for each of 128 splits).
    let costs = BandCosts::new(&values, bands, big);
    // Regions: the split at band edges with the fewest bits.
    let mut best: Option<Split> = None;
    let (r0s, r1s) = if quick { (7..8, 7..8) } else { (0..16, 0..8) };
    for r0 in r0s {
        for r1 in r1s.clone() {
            if r0 + r1 + 2 > 22 {
                continue;
            }
            let a = usize::from(bands[r0 + 1]).min(big);
            let b = usize::from(bands[r0 + r1 + 2]).min(big);
            let (t0, c0) = costs.best(0, r0 + 1);
            let (t1, c1) = costs.best(r0 + 1, r0 + r1 + 2);
            let (t2, c2) = costs.best(r0 + r1 + 2, 22);
            let cost = c0 + c1 + c2;
            if best.is_none_or(|(_, _, _, c, _)| cost < c) {
                best = Some(([t0, t1, t2], r0 as u32, r1 as u32, cost, (a, b)));
            }
            // A split past the pairs changes nothing more.
            if b >= big && a >= big {
                break;
            }
        }
    }
    let (tables, region0, region1, pair_bits, edges) = best.unwrap_or(([0; 3], 0, 0, 0, (0, 0)));
    Granule {
        values,
        global_gain,
        big_values: (big / 2) as u32,
        count1,
        tables,
        region0,
        region1,
        count1_table,
        edges,
        bits: pair_bits + count1_bits,
    }
}

/// The tables with no linbits a region can be coded with.
const SMALL: [u32; 13] = [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15];
/// The two families with linbits, by their first table: 16 to 23 share
/// table 16's codes, 24 to 31 table 24's, and differ only in linbits.
const FAMILIES: [u32; 2] = [16, 24];

/// Bits from band 0 up to each band, so a run of bands costs two lookups:
/// for each small table (with how many bands it cannot code), for each
/// family the codes' bits (values over 15 coded as 15, sign bits counted)
/// and how many values need linbits; and each band's largest value.
struct BandCosts {
    small: Vec<[(u64, u32); 23]>,
    family: [[(u64, u64); 23]; 2],
    most: [i32; 23],
}

impl BandCosts {
    fn new(values: &[i32; 576], bands: &[u16; 23], big: usize) -> BandCosts {
        let mut small = vec![[(0u64, 0u32); 23]; SMALL.len()];
        let mut family = [[(0u64, 0u64); 23]; 2];
        let mut most = [0i32; 23];
        for band in 0..22 {
            let lo = usize::from(bands[band]).min(big);
            let hi = usize::from(bands[band + 1]).min(big);
            let pairs = &values[lo..hi];
            let top = pairs.iter().map(|v| v.abs()).max().unwrap_or(0);
            most[band + 1] = top;
            let pair = |p: &[i32]| (p[0], p.get(1).copied().unwrap_or(0));
            for (i, table) in SMALL.iter().enumerate() {
                let (bits, bad) = small[i][band];
                // Zeros cost bits too under a real table: only a region of
                // zeros alone is free (table 0, in `best`).
                small[i][band + 1] = if (table_size(*table) as i32) > top {
                    let cost: u32 = pairs
                        .chunks(2)
                        .map(|p| {
                            let (x, y) = pair(p);
                            pair_bits(*table, x, y)
                        })
                        .sum();
                    (bits + u64::from(cost), bad)
                } else {
                    (bits, bad + 1)
                };
            }
            for (f, first) in FAMILIES.iter().enumerate() {
                let mut codes = 0u64;
                let mut long = 0u64;
                for p in pairs.chunks(2) {
                    let (x, y) = pair(p);
                    // Without linbits: the code and its signs.
                    codes += u64::from(pair_code(*first, x, y).1)
                        + u64::from(x != 0)
                        + u64::from(y != 0);
                    long += u64::from(x.abs() >= 15) + u64::from(y.abs() >= 15);
                }
                let (c, l) = family[f][band];
                family[f][band + 1] = (c + codes, l + long);
            }
        }
        BandCosts {
            small,
            family,
            most,
        }
    }

    /// The table that codes bands `from..to` in the fewest bits, and those.
    fn best(&self, from: usize, to: usize) -> (u32, u32) {
        let to = to.min(22);
        let top = self.most[from + 1..=to].iter().copied().max().unwrap_or(0);
        if from >= to || top == 0 {
            return (0, 0);
        }
        let mut best = (0, u32::MAX);
        for (i, table) in SMALL.iter().enumerate() {
            let bits = self.small[i][to].0 - self.small[i][from].0;
            let bad = self.small[i][to].1 - self.small[i][from].1;
            if bad == 0 && (bits as u32) < best.1 {
                best = (*table, bits as u32);
            }
        }
        for (f, first) in FAMILIES.iter().enumerate() {
            // The fewest linbits that hold the largest value.
            let Some(table) =
                (*first..*first + 8).find(|t| 15 + (1i32 << tables::LINBITS[*t as usize]) > top)
            else {
                continue;
            };
            let linbits = u64::from(tables::LINBITS[table as usize]);
            let codes = self.family[f][to].0 - self.family[f][from].0;
            let long = self.family[f][to].1 - self.family[f][from].1;
            let bits = codes + linbits * long;
            if (bits as u32) < best.1 {
                best = (table, bits as u32);
            }
        }
        best
    }
}

fn table_size(table: u32) -> usize {
    match table {
        1 => 2,
        2 | 3 => 3,
        5 | 6 => 4,
        7..=9 => 6,
        10..=12 => 8,
        _ => 16,
    }
}

/// A table's codes, by its number (16 to 23 share 16's, 24 to 31 24's).
fn codes(table: u32) -> &'static [(u32, u8)] {
    match table {
        1 => &tables::T1,
        2 => &tables::T2,
        3 => &tables::T3,
        5 => &tables::T5,
        6 => &tables::T6,
        7 => &tables::T7,
        8 => &tables::T8,
        9 => &tables::T9,
        10 => &tables::T10,
        11 => &tables::T11,
        12 => &tables::T12,
        13 => &tables::T13,
        15 => &tables::T15,
        16..=23 => &tables::T16,
        _ => &tables::T24,
    }
}

/// The Huffman code for a pair (values over 15 coded as 15, their rest in
/// linbits), and its length.
fn pair_code(table: u32, x: i32, y: i32) -> (u32, u8) {
    let size = table_size(table);
    let (ax, ay) = (x.unsigned_abs() as usize, y.unsigned_abs() as usize);
    let (cx, cy) = (ax.min(15), ay.min(15));
    codes(table)[cx * size + cy]
}

fn pair_bits(table: u32, x: i32, y: i32) -> u32 {
    if table == 0 {
        return 0;
    }
    let linbits = u32::from(tables::LINBITS[table as usize]);
    let (_, len) = pair_code(table, x, y);
    let mut bits = u32::from(len);
    for v in [x, y] {
        if v != 0 {
            bits += 1;
        }
        if linbits > 0 && v.abs() >= 15 {
            bits += linbits;
        }
    }
    bits
}

/// The count1 table (A or B) with the fewer bits for `values`, and those bits.
fn count1_cost(values: &[i32]) -> (u32, u32) {
    let mut cost = [0u32; 2];
    for quad in values.chunks(4) {
        let index = quad_index(quad);
        let signs = quad.iter().filter(|v| **v != 0).count() as u32;
        cost[0] += u32::from(tables::COUNT1_A[index].1) + signs;
        cost[1] += u32::from(tables::COUNT1_B[index].1) + signs;
    }
    if cost[1] < cost[0] {
        (1, cost[1])
    } else {
        (0, cost[0])
    }
}

fn quad_index(quad: &[i32]) -> usize {
    let bit = |i: usize| usize::from(quad.get(i).copied().unwrap_or(0) != 0);
    (bit(0) << 3) | (bit(1) << 2) | (bit(2) << 1) | bit(3)
}

/// A granule's Huffman-coded lines.
fn write_granule(w: &mut Bits, g: &Granule) {
    let start = w.len();
    let big = g.big_values as usize * 2;
    for (i, pair) in g.values[..big].chunks(2).enumerate() {
        let at = i * 2;
        let table = g.region_table(at);
        if table == 0 {
            continue;
        }
        let (x, y) = (pair[0], pair[1]);
        let (code, len) = pair_code(table, x, y);
        w.put(code, u32::from(len));
        let linbits = u32::from(tables::LINBITS[table as usize]);
        for v in [x, y] {
            if linbits > 0 && v.abs() >= 15 {
                w.put((v.abs() - 15) as u32, linbits);
            }
            if v != 0 {
                w.put(u32::from(v < 0), 1);
            }
        }
    }
    let quads = &g.values[big..big + g.count1 * 4];
    for quad in quads.chunks(4) {
        let (code, len) = if g.count1_table == 1 {
            tables::COUNT1_B[quad_index(quad)]
        } else {
            tables::COUNT1_A[quad_index(quad)]
        };
        w.put(code, u32::from(len));
        for v in quad {
            if *v != 0 {
                w.put(u32::from(*v < 0), 1);
            }
        }
    }
    debug_assert_eq!((w.len() - start) as u32, g.bits);
}

impl Granule {
    /// The table the line at `at` is coded with.
    fn region_table(&self, at: usize) -> u32 {
        let (a, b) = self.edges;
        if at < a {
            self.tables[0]
        } else if at < b {
            self.tables[1]
        } else {
            self.tables[2]
        }
    }
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
mod tests {
    use super::*;

    /// Two seconds of a tone in each channel (440 Hz left, 1 kHz right).
    fn tones(rate: u32) -> Vec<i16> {
        let n = rate as usize * 2;
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f64 / f64::from(rate);
            out.push(((2.0 * PI * 440.0 * t).sin() * 12000.0) as i16);
            out.push(((2.0 * PI * 1000.0 * t).sin() * 8000.0) as i16);
        }
        out
    }

    fn encode(samples: &[i16], channels: usize, rate: u32) -> Vec<u8> {
        let mut e = Encoder::new(channels, rate).unwrap();
        let mut out = e.push(samples);
        out.extend(e.finish());
        out
    }

    #[test]
    fn every_frame_starts_with_its_sync_and_has_its_size() {
        let mp3 = encode(&tones(44100), 2, 44100);
        let mut at = 0;
        let mut frames = 0;
        while at + 4 <= mp3.len() {
            assert_eq!(mp3[at], 0xFF, "no sync at {at}");
            assert_eq!(mp3[at + 1] & 0xE0, 0xE0, "no sync at {at}");
            let padding = usize::from((mp3[at + 2] >> 1) & 1);
            at += 144 * 192_000 / 44100 + padding;
            frames += 1;
        }
        assert_eq!(at, mp3.len(), "frames do not tile the file");
        // Two seconds, the last part frame, and the flush.
        assert_eq!(frames, 88200 / FRAME + 2);
    }

    #[test]
    fn rates_and_channels_it_cannot_make_are_refused_in_words() {
        assert!(Encoder::new(2, 22050).err().unwrap().contains("44.1"));
        assert!(Encoder::new(6, 44100).is_err());
    }

    /// Decoded by the system's own MP3 decoder, the sound is the sound
    /// that went in: aligned, its signal-to-noise ratio is high.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_decoder_plays_back_what_went_in() {
        let dir = tempfile::tempdir().unwrap();
        for (channels, rate) in [(2usize, 44100u32), (1, 48000), (2, 32000)] {
            let input: Vec<i16> = if channels == 2 {
                tones(rate)
            } else {
                tones(rate).chunks(2).map(|p| p[0]).collect()
            };
            let mp3 = dir.path().join(format!("t{channels}{rate}.mp3"));
            let wav = dir.path().join(format!("t{channels}{rate}.wav"));
            std::fs::write(&mp3, encode(&input, channels, rate)).unwrap();
            let ok = std::process::Command::new("/usr/bin/afconvert")
                .arg(&mp3)
                .arg(&wav)
                .args(["-f", "WAVE", "-d", "LEI16"])
                .status()
                .unwrap()
                .success();
            assert!(ok, "the system would not decode it");
            let bytes = std::fs::read(&wav).unwrap();
            let data = bytes
                .windows(4)
                .position(|w| w == b"data")
                .expect("no data in the WAV")
                + 8;
            let out: Vec<i16> = bytes[data..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect();
            // The encoder and decoder's delay: the shift that matches best.
            let left = |s: &[i16], i: usize| f64::from(s[i * channels]);
            let span = rate as usize / 2;
            let start = rate as usize / 4;
            let mut best = (0usize, f64::MAX);
            for shift in 0..3000 {
                if (start + span + shift) * channels > out.len() {
                    break;
                }
                let err: f64 = (start..start + span)
                    .map(|i| (left(&out, i + shift) - left(&input, i)).powi(2))
                    .sum();
                if err < best.1 {
                    best = (shift, err);
                }
            }
            let signal: f64 = (start..start + span).map(|i| left(&input, i).powi(2)).sum();
            let snr = 10.0 * (signal / best.1.max(1e-9)).log10();
            assert!(
                snr > 30.0,
                "{channels} ch at {rate} Hz: {snr:.1} dB at a delay of {}",
                best.0
            );
        }
    }

    /// An AAC sound (made by the system from a tone) saved as MP3 through
    /// `convert`: the system decoder reads the AAC, dm encodes it, and the
    /// MP3 decodes back to the tone.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_aac_sound_is_saved_as_an_mp3_that_plays_it() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("tone.wav");
        let m4a = dir.path().join("tone.m4a");
        let mp3 = dir.path().join("tone.mp3");
        // A 440 Hz stereo tone as a WAV, then AAC by the system.
        let samples = tones(44100);
        let mut file = Vec::new();
        let data = (samples.len() * 2) as u32;
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&(36 + data).to_le_bytes());
        file.extend_from_slice(b"WAVEfmt ");
        file.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 2] {
            file.extend_from_slice(&v.to_le_bytes());
        }
        for v in [44100u32, 44100 * 4] {
            file.extend_from_slice(&v.to_le_bytes());
        }
        for v in [4u16, 16] {
            file.extend_from_slice(&v.to_le_bytes());
        }
        file.extend_from_slice(b"data");
        file.extend_from_slice(&data.to_le_bytes());
        for s in &samples {
            file.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(&wav, file).unwrap();
        let made = std::process::Command::new("/usr/bin/afconvert")
            .arg(&wav)
            .arg(&m4a)
            .args(["-f", "m4af", "-d", "aac"])
            .status()
            .unwrap()
            .success();
        assert!(made, "the system would not make the AAC");
        let length = convert(&m4a, &mp3).unwrap();
        assert!(length > 40_000, "{length}");
        let pcm = decode::read(&mp3).unwrap();
        assert_eq!((pcm.channels, pcm.rate), (2, 44100));
        // The tone is there at about the level it went in.
        let seg = &pcm.samples[40000..80000];
        let (mut c, mut s) = (0.0f64, 0.0f64);
        for (i, v) in seg.chunks(2).enumerate() {
            let ph = 2.0 * PI * 440.0 * i as f64 / 44100.0;
            c += f64::from(v[0]) * ph.cos();
            s += f64::from(v[0]) * ph.sin();
        }
        let level = 2.0 * (c * c + s * s).sqrt() / (seg.len() / 2) as f64;
        assert!((10_000.0..14_000.0).contains(&level), "{level}");
    }

    #[test]
    fn encoding_on_every_core_gives_the_bytes_one_run_gives() {
        // Long enough to be cut into runs: ten seconds of the tones.
        let mut samples = tones(44100);
        samples.extend(tones(44100));
        samples.extend(tones(44100));
        samples.extend(tones(44100));
        samples.extend(tones(44100));
        assert_eq!(
            encode_all(&samples, 2, 44100).unwrap(),
            encode(&samples, 2, 44100)
        );
    }
}
