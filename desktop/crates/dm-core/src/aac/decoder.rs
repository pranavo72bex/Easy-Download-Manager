//! One AAC-LC frame (a `raw_data_block`) to 1024 samples per channel.
//!
//! Supported: single and channel-pair elements, long and eight-short windows with their start
//! and stop shapes, section and scale-factor data, pulses, mid/side stereo, TNS with plain
//! coefficients. Refused with a clear error rather than decoded wrongly: intensity stereo and
//! noise substitution (books 13-15), main-profile prediction, gain control, coupling channels,
//! program configs, TNS with compressed coefficients.

use super::bits::Bits;
use super::huff::Book;
use super::tables;
use super::transform::{self, EIGHT_SHORT};

const ZERO_BOOK: u8 = 0;
const FIRST_RESERVED: u8 = 12;

/// What a frame says about its window.
#[derive(Clone)]
struct Info {
    sequence: u8,
    shape: u8,
    max_sfb: usize,
    /// Windows in each group (one group of one window for a long block).
    groups: Vec<usize>,
    edges: &'static [u16],
}

impl Info {
    fn short(&self) -> bool {
        self.sequence == EIGHT_SHORT
    }
}

struct Books {
    scalefactor: Book,
    spectral: Vec<Option<Book>>,
}

pub(super) struct Decoder {
    rate_index: usize,
    channels: usize,
    books: Books,
    shape: Vec<u8>,
    held: Vec<Vec<f64>>,
}

impl Decoder {
    /// A decoder for `channels` (1 or 2) at the sample-rate index, or why not.
    pub(super) fn new(rate_index: usize, channels: usize) -> Result<Decoder, String> {
        if !tables::COMPLETE {
            return Err("dm's AAC decoder has no Huffman tables yet".into());
        }
        if channels == 0 || channels > 2 {
            return Err("only mono and stereo sound is converted to MP3".into());
        }
        let scalefactor = tables::scalefactor_book().ok_or("no scale-factor table")?;
        let spectral = (1..=11).map(tables::spectral_book).collect();
        Ok(Decoder {
            rate_index,
            channels,
            books: Books {
                scalefactor,
                spectral,
            },
            shape: vec![0; channels],
            held: vec![Vec::new(); channels],
        })
    }

    /// The frame's samples, one vector of 1024 per channel.
    pub(super) fn frame(&mut self, data: &[u8]) -> Result<Vec<Vec<f64>>, String> {
        let mut bits = Bits::new(data);
        let mut spectra: Vec<(Vec<f64>, Info)> = Vec::new();
        loop {
            match bits.bits(3)? {
                0 | 3 => {
                    bits.skip(4)?;
                    spectra.push(self.channel(&mut bits, None)?);
                }
                1 => {
                    bits.skip(4)?;
                    let common = bits.flag()?;
                    let (first, second) = if common {
                        let info = self.info(&mut bits)?;
                        let mask = self.mask(&mut bits, &info)?;
                        let mut a = self.channel(&mut bits, Some(info.clone()))?;
                        let mut b = self.channel(&mut bits, Some(info.clone()))?;
                        if let Some(mask) = mask {
                            mid_side(&mut a.0, &mut b.0, &info, &mask);
                        }
                        (a, b)
                    } else {
                        let a = self.channel(&mut bits, None)?;
                        let b = self.channel(&mut bits, None)?;
                        (a, b)
                    };
                    spectra.push(first);
                    spectra.push(second);
                }
                2 => return Err("a coupling channel in AAC data".into()),
                4 => {
                    bits.skip(4)?;
                    let align = bits.flag()?;
                    let mut count = bits.bits(8)? as usize;
                    if count == 255 {
                        count += bits.bits(8)? as usize;
                    }
                    if align {
                        bits.align();
                    }
                    bits.skip(count * 8)?;
                }
                5 => return Err("a program config in the middle of AAC data".into()),
                6 => {
                    let mut count = bits.bits(4)? as usize;
                    if count == 15 {
                        count += (bits.bits(8)? as usize).saturating_sub(1);
                    }
                    bits.skip(count * 8)?;
                }
                _ => break,
            }
        }
        if spectra.len() != self.channels {
            return Err("a frame with a different number of channels than the file's".into());
        }
        let mut out = Vec::new();
        for (index, (lines, info)) in spectra.into_iter().enumerate() {
            let previous = self.shape.get(index).copied().unwrap_or(0);
            let frame = transform::frame_samples(&lines, info.sequence, previous, info.shape);
            if let Some(slot) = self.shape.get_mut(index) {
                *slot = info.shape;
            }
            let held = self.held.get_mut(index).ok_or("a channel out of range")?;
            out.push(transform::overlap(&frame, held));
        }
        Ok(out)
    }

    fn info(&self, bits: &mut Bits) -> Result<Info, String> {
        bits.skip(1)?;
        let sequence = bits.bits(2)? as u8;
        let shape = bits.bits(1)? as u8;
        let short = sequence == EIGHT_SHORT;
        let edges = tables::band_edges(self.rate_index, short)
            .ok_or("a sample rate whose AAC band table is not in dm yet")?;
        let (max_sfb, groups) = if short {
            let max_sfb = bits.bits(4)? as usize;
            let grouping = bits.bits(7)?;
            let mut groups = vec![1usize];
            for i in 0..7 {
                if grouping & (1 << (6 - i)) == 0 {
                    groups.push(1);
                } else if let Some(last) = groups.last_mut() {
                    *last += 1;
                }
            }
            (max_sfb, groups)
        } else {
            let max_sfb = bits.bits(6)? as usize;
            if bits.flag()? {
                return Err("AAC main-profile prediction is not supported".into());
            }
            (max_sfb, vec![1])
        };
        if max_sfb + 1 > edges.len() {
            return Err("more scale-factor bands than the window has".into());
        }
        Ok(Info {
            sequence,
            shape,
            max_sfb,
            groups,
            edges,
        })
    }

    /// The mid/side flags of a pair: per group and band, or None when off.
    fn mask(&self, bits: &mut Bits, info: &Info) -> Result<Option<Vec<Vec<bool>>>, String> {
        let kind = bits.bits(2)?;
        match kind {
            0 => Ok(None),
            1 | 2 => {
                let mut flags = Vec::new();
                for _ in &info.groups {
                    let mut row = Vec::new();
                    for _ in 0..info.max_sfb {
                        row.push(if kind == 1 { bits.flag()? } else { true });
                    }
                    flags.push(row);
                }
                Ok(Some(flags))
            }
            _ => Err("a reserved mid/side mask in AAC data".into()),
        }
    }

    /// One individual channel stream: its dequantized lines (windows after one another) and info.
    fn channel(&self, bits: &mut Bits, shared: Option<Info>) -> Result<(Vec<f64>, Info), String> {
        let global_gain = bits.bits(8)? as i32;
        let info = match shared {
            Some(info) => info,
            None => self.info(bits)?,
        };
        let short = info.short();
        let (length_bits, escape) = if short { (3, 7) } else { (5, 31) };
        // Sections: the codebook of each (group, band).
        let mut books = vec![vec![ZERO_BOOK; info.max_sfb]; info.groups.len()];
        for row in books.iter_mut() {
            let mut band = 0;
            while band < info.max_sfb {
                let book = bits.bits(4)? as u8;
                if book >= FIRST_RESERVED {
                    return Err(if book == 12 {
                        "a reserved AAC codebook".into()
                    } else {
                        "intensity stereo and noise substitution in AAC are not supported".into()
                    });
                }
                let mut length = 0;
                loop {
                    let step = bits.bits(length_bits)? as usize;
                    length += step;
                    if step != escape {
                        break;
                    }
                }
                if length == 0 || band + length > info.max_sfb {
                    return Err("an AAC section that runs past the bands".into());
                }
                for slot in row.iter_mut().skip(band).take(length) {
                    *slot = book;
                }
                band += length;
            }
        }
        // Scale factors.
        let mut factors = vec![vec![0i32; info.max_sfb]; info.groups.len()];
        let mut current = global_gain;
        for (row, book_row) in factors.iter_mut().zip(&books) {
            for (slot, &book) in row.iter_mut().zip(book_row) {
                if book != ZERO_BOOK {
                    current += i32::from(self.books.scalefactor.decode(bits)?) - 60;
                    *slot = current;
                }
            }
        }
        let pulse = if bits.flag()? {
            if short {
                return Err("an AAC pulse in a short window".into());
            }
            let count = bits.bits(2)? as usize + 1;
            let start = bits.bits(6)? as usize;
            let mut pulses = Vec::new();
            for _ in 0..count {
                pulses.push((bits.bits(5)? as usize, bits.bits(4)? as i32));
            }
            Some((start, pulses))
        } else {
            None
        };
        let tns = if bits.flag()? {
            Some(read_tns(bits, short)?)
        } else {
            None
        };
        if bits.flag()? {
            return Err("AAC gain control is not supported".into());
        }
        // Spectral data, quantized.
        let windows: usize = info.groups.iter().sum();
        let mut quantized =
            vec![0i32; windows * (info.edges.last().copied().unwrap_or(0) as usize)];
        let width = info.edges.last().copied().unwrap_or(0) as usize;
        let mut first_window = 0;
        for (group, &count) in info.groups.iter().enumerate() {
            for band in 0..info.max_sfb {
                let lo = usize::from(info.edges[band]);
                let size = usize::from(info.edges[band + 1]) - lo;
                let book = books[group][band];
                if book == ZERO_BOOK {
                    continue;
                }
                let values = self.spectral(bits, book, count * size)?;
                for (j, value) in values.into_iter().enumerate() {
                    let window = first_window + j / size;
                    let at = window * width + lo + j % size;
                    if let Some(slot) = quantized.get_mut(at) {
                        *slot = value;
                    }
                }
            }
            first_window += count;
        }
        if let Some((start, pulses)) = pulse {
            let mut at = usize::from(
                *info
                    .edges
                    .get(start)
                    .ok_or("an AAC pulse start past the bands")?,
            );
            for (offset, amplitude) in pulses {
                at += offset;
                let slot = quantized.get_mut(at).ok_or("an AAC pulse past the lines")?;
                *slot += if *slot > 0 { amplitude } else { -amplitude };
            }
        }
        // Dequantize.
        let mut lines = vec![0.0f64; quantized.len()];
        let mut first_window = 0;
        for (group, &count) in info.groups.iter().enumerate() {
            for band in 0..info.max_sfb {
                let gain = 2f64.powf(0.25 * f64::from(factors[group][band] - 100));
                for window in first_window..first_window + count {
                    for line in usize::from(info.edges[band])..usize::from(info.edges[band + 1]) {
                        let at = window * width + line;
                        let q = quantized[at];
                        lines[at] =
                            f64::from(q.signum()) * f64::from(q.abs()).powf(4.0 / 3.0) * gain;
                    }
                }
            }
            first_window += count;
        }
        if let Some(filters) = tns {
            apply_tns(&mut lines, &info, &filters)?;
        }
        Ok((lines, info))
    }

    /// `count` quantized values from codebook `book`.
    fn spectral(&self, bits: &mut Bits, book: u8, count: usize) -> Result<Vec<i32>, String> {
        let table = self
            .books
            .spectral
            .get(usize::from(book) - 1)
            .and_then(|b| b.as_ref())
            .ok_or("an AAC spectral codebook that dm has no table for")?;
        let (dimension, signed, base): (usize, bool, usize) = match book {
            1 | 2 => (4, true, 3),
            3 | 4 => (4, false, 3),
            5 | 6 => (2, true, 9),
            7 | 8 => (2, false, 8),
            9 | 10 => (2, false, 13),
            _ => (2, false, 17),
        };
        let mut values = Vec::with_capacity(count);
        while values.len() < count {
            let mut index = usize::from(table.decode(bits)?);
            let mut chunk = vec![0i32; dimension];
            for slot in chunk.iter_mut().rev() {
                let digit = (index % base) as i32;
                index /= base;
                *slot = if signed {
                    digit - (base as i32 - 1) / 2
                } else {
                    digit
                };
            }
            if !signed {
                for slot in chunk.iter_mut() {
                    if *slot != 0 && bits.flag()? {
                        *slot = -*slot;
                    }
                }
            }
            if book == 11 {
                for slot in chunk.iter_mut() {
                    if slot.abs() == 16 {
                        let negative = *slot < 0;
                        let mut width = 4;
                        while bits.flag()? {
                            width += 1;
                            if width > 15 {
                                return Err("an AAC escape sequence that never ends".into());
                            }
                        }
                        let word = bits.bits(width)? as i32;
                        let magnitude = (1 << width) + word;
                        *slot = if negative { -magnitude } else { magnitude };
                    }
                }
            }
            values.extend(chunk);
        }
        values.truncate(count);
        Ok(values)
    }
}

/// Mid/side to left/right on the bands the mask names.
fn mid_side(left: &mut [f64], right: &mut [f64], info: &Info, mask: &[Vec<bool>]) {
    let width = info.edges.last().copied().unwrap_or(0) as usize;
    let first_window = 0;
    for (group, &count) in info.groups.iter().enumerate() {
        for band in 0..info.max_sfb {
            if !mask
                .get(group)
                .and_then(|r| r.get(band))
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            for window in first_window..first_window + count {
                for line in usize::from(info.edges[band])..usize::from(info.edges[band + 1]) {
                    let at = window * width + line;
                    if let (Some(l), Some(r)) = (left.get_mut(at), right.get_mut(at)) {
                        let (m, s) = (*l, *r);
                        *l = m + s;
                        *r = m - s;
                    }
                }
            }
        }
    }
}

struct Filter {
    window: usize,
    length: usize,
    backwards: bool,
    /// Reflection coefficients.
    parcor: Vec<f64>,
}

fn read_tns(bits: &mut Bits, short: bool) -> Result<Vec<Filter>, String> {
    let windows = if short { 8 } else { 1 };
    let (count_bits, length_bits, order_bits) = if short { (1, 4, 3) } else { (2, 6, 5) };
    let mut filters = Vec::new();
    for window in 0..windows {
        let count = bits.bits(count_bits)? as usize;
        if count == 0 {
            continue;
        }
        let high_resolution = bits.flag()?;
        for _ in 0..count {
            let length = bits.bits(length_bits)? as usize;
            let order = bits.bits(order_bits)? as usize;
            let mut backwards = false;
            let mut parcor = Vec::new();
            if order > 0 {
                backwards = bits.flag()?;
                if bits.flag()? {
                    return Err("AAC TNS with compressed coefficients is not supported".into());
                }
                let size = if high_resolution { 4 } else { 3 };
                for _ in 0..order {
                    let raw = bits.bits(size)? as i32;
                    let index = if raw >= 1 << (size - 1) {
                        raw - (1 << size)
                    } else {
                        raw
                    };
                    let half = f64::from(1 << (size - 1));
                    let scale = if index >= 0 { half - 0.5 } else { half + 0.5 };
                    parcor.push((f64::from(index) * (std::f64::consts::FRAC_PI_2 / scale)).sin());
                }
            }
            filters.push(Filter {
                window,
                length,
                backwards,
                parcor,
            });
        }
    }
    Ok(filters)
}

/// Reflection coefficients to the direct-form predictor (a[0] = 1).
fn predictor(parcor: &[f64]) -> Vec<f64> {
    let mut a = vec![1.0];
    for (m, &k) in parcor.iter().enumerate() {
        let mut next = a.clone();
        next.push(0.0);
        for i in 1..=m + 1 {
            let mirror = a.get(m + 1 - i).copied().unwrap_or(0.0);
            if let Some(slot) = next.get_mut(i) {
                *slot = a.get(i).copied().unwrap_or(0.0) + k * mirror;
            }
        }
        a = next;
    }
    a
}

fn apply_tns(lines: &mut [f64], info: &Info, filters: &[Filter]) -> Result<(), String> {
    let width = info.edges.last().copied().unwrap_or(0) as usize;
    let bands = info.edges.len() - 1;
    let mut top = bands;
    let mut window_seen = usize::MAX;
    for filter in filters {
        if filter.window != window_seen {
            window_seen = filter.window;
            top = bands;
        }
        let bottom = top.saturating_sub(filter.length);
        let start = usize::from(info.edges[bottom.min(info.max_sfb)]);
        let end = usize::from(info.edges[top.min(info.max_sfb)]);
        top = bottom;
        if filter.parcor.is_empty() || end <= start {
            continue;
        }
        let a = predictor(&filter.parcor);
        let base = filter.window * width;
        let mut run: Vec<usize> = (start..end).collect();
        if filter.backwards {
            run.reverse();
        }
        for (n, &line) in run.iter().enumerate() {
            let mut sum = 0.0;
            for (i, coefficient) in a.iter().enumerate().skip(1) {
                if let Some(&earlier) = n.checked_sub(i).and_then(|p| run.get(p)) {
                    sum += coefficient * lines.get(base + earlier).copied().unwrap_or(0.0);
                }
            }
            if let Some(slot) = lines.get_mut(base + line) {
                *slot -= sum;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decoder_is_refused_while_the_tables_are_missing() {
        assert!(Decoder::new(4, 2).is_err());
    }

    #[test]
    fn the_predictor_of_one_reflection_coefficient_is_one_and_k() {
        let a = predictor(&[0.5]);
        assert_eq!(a, vec![1.0, 0.5]);
        let b = predictor(&[0.5, 0.25]);
        assert_eq!(b.len(), 3);
        assert!((b[1] - (0.5 + 0.25 * 0.5)).abs() < 1e-12);
        assert!((b[2] - 0.25).abs() < 1e-12);
    }

    #[test]
    fn mid_side_turns_mid_and_side_into_left_and_right_on_flagged_bands_only() {
        let edges: &'static [u16] = &[0, 2, 4];
        let info = Info {
            sequence: transform::ONLY_LONG,
            shape: 0,
            max_sfb: 2,
            groups: vec![1],
            edges,
        };
        let (mut l, mut r) = (vec![3.0, 3.0, 5.0, 5.0], vec![1.0, 1.0, 2.0, 2.0]);
        mid_side(&mut l, &mut r, &info, &[vec![true, false]]);
        assert_eq!(l, vec![4.0, 4.0, 5.0, 5.0]);
        assert_eq!(r, vec![2.0, 2.0, 2.0, 2.0]);
    }
}
