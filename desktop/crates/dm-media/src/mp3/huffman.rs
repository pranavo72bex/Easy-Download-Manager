// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A granule's lines quantized to the finest global gain that fits, and coded: the Huffman tables
//! chosen by region and their costs, and the granule written.

use super::*;

/// The finest quantization of `lines` whose coding fits `budget` bits:
/// searched from `hint` (the channel's last gain) where there is one, a
/// step or two each way, else by halves over the whole range.
pub(super) fn quantize(
    lines: &[f64; 576],
    budget: usize,
    rate: usize,
    hint: Option<u32>,
) -> Granule {
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
pub(super) fn write_granule(w: &mut Bits, g: &Granule) {
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
