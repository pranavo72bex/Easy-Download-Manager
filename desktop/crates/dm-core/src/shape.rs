// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A video made vertical (9:16) or square (1:1) without re-encoding it.
//!
//! An H.264 stream says, in its sequence parameter set (SPS), which
//! rectangle of the coded picture to show: the "frame cropping" every
//! decoder honours, the way a 1080-line video is coded as 1088 lines and
//! shown as 1080. dm rewrites that rectangle to the middle 9:16 or 1:1 of
//! the picture, and the sizes the MP4 states beside it, so every player
//! shows the shape asked for. Nothing is decoded and nothing is lost; the
//! file keeps the whole coded picture, so it is as large as the original.
//!
//! Only an H.264 picture (`avc1`, `avc3`) can be shaped this way: VP9 and
//! AV1 are refused in words.

use crate::mp4mux::Track;

/// The shape a video is made into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// Nine wide by sixteen tall, as a phone holds it.
    Vertical,
    /// As wide as it is tall.
    Square,
}

impl Shape {
    /// The shape a stored or typed word names: "vertical" or "square".
    pub fn named(word: &str) -> Option<Shape> {
        match word.trim().to_ascii_lowercase().as_str() {
            "vertical" => Some(Shape::Vertical),
            "square" => Some(Shape::Square),
            _ => None,
        }
    }

    /// The word [`Shape::named`] reads back.
    pub fn name(self) -> &'static str {
        match self {
            Shape::Vertical => "vertical",
            Shape::Square => "square",
        }
    }

    /// Width and height of the ratio: 9:16 or 1:1.
    fn ratio(self) -> (u64, u64) {
        match self {
            Shape::Vertical => (9, 16),
            Shape::Square => (1, 1),
        }
    }
}

/// Shapes `track` where it is a picture: its SPS's crop, its sample
/// entry's size and its header's size. A sound or text track is left as it
/// is; a picture that is not H.264 is refused.
pub(crate) fn apply(track: &mut Track, shape: Shape) -> Result<(), String> {
    // The handler box whole: its type after size, kind, version, predefined.
    if track.hdlr.get(16..20) != Some(b"vide") {
        return Ok(());
    }
    let (stsd, (width, height)) = shaped_stsd(&track.stsd, shape)?;
    track.stsd = stsd;
    // The header's size, 16.16 fixed point, at the end of its tail.
    if track.tkhd_tail.len() >= 60 {
        track.tkhd_tail[52..56].copy_from_slice(&((width as u32) << 16).to_be_bytes());
        track.tkhd_tail[56..60].copy_from_slice(&((height as u32) << 16).to_be_bytes());
    }
    Ok(())
}

/// `stsd` (a whole box) with its first H.264 entry's SPS shaped, and the
/// size it shows now.
fn shaped_stsd(stsd: &[u8], shape: Shape) -> Result<(Vec<u8>, (u64, u64)), String> {
    // A full box: size, kind, version and flags, entry count; then entries.
    let entries = stsd
        .get(16..)
        .ok_or("the sample description is cut short")?;
    let size = u32_at(entries, 0).ok_or("the sample description is cut short")? as usize;
    let entry = entries
        .get(..size)
        .ok_or("the sample description is cut short")?;
    let kind = entry
        .get(4..8)
        .ok_or("the sample description is cut short")?;
    if kind != b"avc1" && kind != b"avc3" {
        return Err(format!(
            "only an H.264 video can be made {}; this one is {}",
            shape.name(),
            String::from_utf8_lossy(kind)
        ));
    }
    // A visual sample entry: 8 of header, 78 of fields, then its boxes.
    let fields = entry
        .get(..86)
        .ok_or("the video's description is cut short")?;
    let mut children = Vec::new();
    let mut size_shown = None;
    let mut at = 86;
    while at + 8 <= entry.len() {
        let len = u32_at(entry, at).ok_or("a box is cut short")? as usize;
        let child = entry
            .get(at..at + len)
            .filter(|_| len >= 8)
            .ok_or("a box is cut short")?;
        if child.get(4..8) == Some(b"avcC") {
            let (avcc, shown) = shaped_avcc(child.get(8..).unwrap_or_default(), shape)?;
            children.extend_from_slice(&boxed(b"avcC", &avcc));
            size_shown = Some(shown);
        } else {
            children.extend_from_slice(child);
        }
        at += len;
    }
    let (width, height) = size_shown.ok_or("the video has no H.264 settings (avcC)")?;
    let mut fields = fields.to_vec();
    fields[32..34].copy_from_slice(&(width as u16).to_be_bytes());
    fields[34..36].copy_from_slice(&(height as u16).to_be_bytes());
    let new_entry = [&fields[8..], &children[..]].concat();
    let mut out_entries = boxed(&kind.try_into().unwrap_or(*b"avc1"), &new_entry);
    out_entries.extend_from_slice(entries.get(size..).unwrap_or_default());
    let body = [stsd.get(8..16).unwrap_or_default(), &out_entries[..]].concat();
    Ok((boxed(b"stsd", &body), (width, height)))
}

/// An `avcC`'s contents with each SPS shaped; the size the first shows.
fn shaped_avcc(avcc: &[u8], shape: Shape) -> Result<(Vec<u8>, (u64, u64)), String> {
    let head = avcc.get(..5).ok_or("the H.264 settings are cut short")?;
    let count = usize::from(*avcc.get(5).ok_or("the H.264 settings are cut short")? & 0x1f);
    let mut out = head.to_vec();
    out.push(avcc[5]);
    let mut at = 6;
    let mut shown = None;
    for _ in 0..count {
        let len = usize::from(u16::from_be_bytes([
            *avcc.get(at).ok_or("an SPS is cut short")?,
            *avcc.get(at + 1).ok_or("an SPS is cut short")?,
        ]));
        let nal = avcc
            .get(at + 2..at + 2 + len)
            .ok_or("an SPS is cut short")?;
        let (sps, size) = shaped_sps(nal, shape)?;
        shown.get_or_insert(size);
        out.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        out.extend_from_slice(&sps);
        at += 2 + len;
    }
    // The picture parameter sets and anything after them, as they were.
    out.extend_from_slice(avcc.get(at..).unwrap_or_default());
    Ok((out, shown.ok_or("the video has no SPS")?))
}

/// One SPS NAL unit with its crop set to the middle of the picture in
/// `shape`; and the size it then shows.
fn shaped_sps(nal: &[u8], shape: Shape) -> Result<(Vec<u8>, (u64, u64)), String> {
    let header = *nal.first().ok_or("an empty SPS")?;
    let rbsp = unescape(nal.get(1..).unwrap_or_default());
    let mut r = Bits::new(&rbsp);
    let mut w = Writer::default();
    let short = || "the SPS is cut short".to_string();
    let profile = r.copy(&mut w, 8).ok_or_else(short)?;
    r.copy(&mut w, 16).ok_or_else(short)?; // constraints, level
    r.copy_ue(&mut w).ok_or_else(short)?; // its id
    let mut chroma = 1;
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma = r.copy_ue(&mut w).ok_or_else(short)?;
        let mut separate = 0;
        if chroma == 3 {
            separate = r.copy(&mut w, 1).ok_or_else(short)?;
        }
        if separate == 1 {
            chroma = 0;
        }
        r.copy_ue(&mut w).ok_or_else(short)?; // luma bit depth
        r.copy_ue(&mut w).ok_or_else(short)?; // chroma bit depth
        r.copy(&mut w, 1).ok_or_else(short)?; // lossless bypass
        if r.copy(&mut w, 1).ok_or_else(short)? == 1 {
            let lists = if chroma == 3 { 12 } else { 8 };
            for i in 0..lists {
                if r.copy(&mut w, 1).ok_or_else(short)? == 1 {
                    let size = if i < 6 { 16 } else { 64 };
                    let (mut last, mut next) = (8i64, 8i64);
                    for _ in 0..size {
                        if next != 0 {
                            let delta = r.copy_se(&mut w).ok_or_else(short)?;
                            next = (last + delta + 256) % 256;
                        }
                        if next != 0 {
                            last = next;
                        }
                    }
                }
            }
        }
    }
    r.copy_ue(&mut w).ok_or_else(short)?; // log2 max frame num
    match r.copy_ue(&mut w).ok_or_else(short)? {
        0 => {
            r.copy_ue(&mut w).ok_or_else(short)?;
        }
        1 => {
            r.copy(&mut w, 1).ok_or_else(short)?;
            r.copy_se(&mut w).ok_or_else(short)?;
            r.copy_se(&mut w).ok_or_else(short)?;
            let cycle = r.copy_ue(&mut w).ok_or_else(short)?;
            for _ in 0..cycle.min(255) {
                r.copy_se(&mut w).ok_or_else(short)?;
            }
        }
        _ => {}
    }
    r.copy_ue(&mut w).ok_or_else(short)?; // reference frames
    r.copy(&mut w, 1).ok_or_else(short)?; // gaps allowed
    let width_mbs = r.copy_ue(&mut w).ok_or_else(short)? + 1;
    let height_units = r.copy_ue(&mut w).ok_or_else(short)? + 1;
    let frames_only = r.copy(&mut w, 1).ok_or_else(short)?;
    if frames_only == 0 {
        r.copy(&mut w, 1).ok_or_else(short)?;
    }
    r.copy(&mut w, 1).ok_or_else(short)?; // direct 8x8
                                          // The crop as it is (a 1088-line picture shown as 1080, say).
    let mut crop = [0u64; 4]; // left, right, top, bottom
    if r.read(1).ok_or_else(short)? == 1 {
        for side in &mut crop {
            *side = r.ue().ok_or_else(short)?;
        }
    }
    let (unit_x, unit_y) = match chroma {
        0 => (1, 2 - frames_only),
        1 => (2, 2 * (2 - frames_only)),
        2 => (2, 2 - frames_only),
        _ => (1, 2 - frames_only),
    };
    let coded_w = width_mbs * 16;
    let coded_h = height_units * 16 * (2 - frames_only);
    let shown_w = coded_w.saturating_sub(unit_x * (crop[0] + crop[1]));
    let shown_h = coded_h.saturating_sub(unit_y * (crop[2] + crop[3]));
    if shown_w == 0 || shown_h == 0 {
        return Err("the video's picture has no size".into());
    }
    // The middle of the shown picture in the ratio, in whole crop units.
    let (rw, rh) = shape.ratio();
    let (mut new_w, mut new_h) = (shown_w, shown_h);
    if shown_w * rh > shown_h * rw {
        new_w = (shown_h * rw / rh) / unit_x * unit_x;
    } else {
        new_h = (shown_w * rh / rw) / unit_y * unit_y;
    }
    let (cut_x, cut_y) = ((shown_w - new_w) / unit_x, (shown_h - new_h) / unit_y);
    crop[0] += cut_x / 2;
    crop[1] += cut_x - cut_x / 2;
    crop[2] += cut_y / 2;
    crop[3] += cut_y - cut_y / 2;
    w.put(1, 1);
    for side in crop {
        w.put_ue(side);
    }
    // The rest (the video usability information) as it was, up to the
    // stop bit, which is written again.
    let rest = r.remaining_before_stop();
    for _ in 0..rest {
        let bit = r.read(1).ok_or_else(short)?;
        w.put(bit, 1);
    }
    w.put(1, 1);
    let rbsp = w.finish();
    let mut out = vec![header];
    out.extend_from_slice(&escape(&rbsp));
    Ok((out, (new_w, new_h)))
}

/// An NAL unit's payload without its emulation-prevention bytes.
fn unescape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut zeros = 0;
    for &byte in data {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        out.push(byte);
    }
    out
}

/// A payload with emulation-prevention bytes put back where three bytes
/// would read as a start code.
fn escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 4);
    let mut zeros = 0;
    for &byte in data {
        if zeros >= 2 && byte <= 3 {
            out.push(3);
            zeros = 0;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        out.push(byte);
    }
    out
}

/// A box of `kind` around `body`.
fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// Reads bits, most significant first.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits { data, at: 0 }
    }

    fn read(&mut self, n: u32) -> Option<u64> {
        let mut value = 0u64;
        for _ in 0..n {
            let byte = *self.data.get(self.at / 8)?;
            let bit = (byte >> (7 - self.at % 8)) & 1;
            value = (value << 1) | u64::from(bit);
            self.at += 1;
        }
        Some(value)
    }

    /// An unsigned Exp-Golomb number.
    fn ue(&mut self) -> Option<u64> {
        let mut zeros = 0;
        while self.read(1)? == 0 {
            zeros += 1;
            if zeros > 32 {
                return None;
            }
        }
        Some((1u64 << zeros) - 1 + self.read(zeros)?)
    }

    /// `n` bits read and written as they were.
    fn copy(&mut self, w: &mut Writer, n: u32) -> Option<u64> {
        let value = self.read(n)?;
        w.put(value, n);
        Some(value)
    }

    fn copy_ue(&mut self, w: &mut Writer) -> Option<u64> {
        let value = self.ue()?;
        w.put_ue(value);
        Some(value)
    }

    fn copy_se(&mut self, w: &mut Writer) -> Option<i64> {
        let code = self.copy_ue(w)?;
        let magnitude = code.div_ceil(2) as i64;
        Some(if code % 2 == 1 { magnitude } else { -magnitude })
    }

    /// Bits left before the RBSP's stop bit (the last 1 in the data).
    fn remaining_before_stop(&self) -> usize {
        let total = self.data.len() * 8;
        let mut last = total;
        while last > self.at {
            let i = last - 1;
            if (self.data[i / 8] >> (7 - i % 8)) & 1 == 1 {
                return i - self.at;
            }
            last -= 1;
        }
        0
    }
}

/// Writes bits, most significant first.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    bits: usize,
}

impl Writer {
    fn put(&mut self, value: u64, n: u32) {
        for i in (0..n).rev() {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                if let Some(last) = self.bytes.last_mut() {
                    *last |= 1 << (7 - self.bits % 8);
                }
            }
            self.bits += 1;
        }
    }

    fn put_ue(&mut self, value: u64) {
        let coded = value + 1;
        let len = 64 - coded.leading_zeros();
        self.put(0, len - 1);
        self.put(coded, len);
    }

    /// The bytes, the last padded with zero bits after the stop bit.
    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Main-profile SPS for 1920x1080 (coded as 1088, cropped by 8 rows),
    /// with a VUI that must survive the rewrite.
    fn sps_1080() -> Vec<u8> {
        let mut w = Writer::default();
        w.put(77, 8); // profile: Main
        w.put(0, 8); // constraints
        w.put(40, 8); // level 4.0
        w.put_ue(0); // id
        w.put_ue(0); // log2 max frame num - 4
        w.put_ue(0); // poc type 0
        w.put_ue(2); // log2 max poc lsb - 4
        w.put_ue(4); // reference frames
        w.put(0, 1); // gaps
        w.put_ue(119); // 120 macroblocks wide: 1920
        w.put_ue(67); // 68 tall: 1088
        w.put(1, 1); // frames only
        w.put(1, 1); // direct 8x8
        w.put(1, 1); // cropping
        for side in [0, 0, 0, 4] {
            w.put_ue(side); // 4 units of 2 rows at the bottom
        }
        w.put(1, 1); // VUI present
        w.put(1, 1); // aspect ratio info present
        w.put(1, 8); // 1:1
        w.put(0, 6); // the rest of the VUI's flags, off
        w.put(1, 1); // stop bit
        let mut nal = vec![0x67];
        nal.extend_from_slice(&escape(&w.finish()));
        nal
    }

    /// The SPS's coded crop and what it shows, read back.
    fn shown(nal: &[u8]) -> (u64, u64) {
        let rbsp = unescape(&nal[1..]);
        let mut r = Bits::new(&rbsp);
        r.read(24).unwrap();
        for _ in 0..4 {
            r.ue().unwrap();
        }
        r.ue().unwrap();
        r.read(1).unwrap();
        let w = (r.ue().unwrap() + 1) * 16;
        let h = (r.ue().unwrap() + 1) * 16;
        r.read(2).unwrap();
        assert_eq!(r.read(1).unwrap(), 1, "cropping off");
        let c: Vec<u64> = (0..4).map(|_| r.ue().unwrap()).collect();
        // The VUI is still there after it, as it was.
        assert_eq!(r.read(2).unwrap(), 0b11, "the VUI was lost");
        assert_eq!(r.read(8).unwrap(), 1, "the aspect ratio was lost");
        (w - 2 * (c[0] + c[1]), h - 2 * (c[2] + c[3]))
    }

    #[test]
    fn a_wide_picture_is_cut_to_its_middle_nine_by_sixteen_or_square() {
        let (vertical, size) = shaped_sps(&sps_1080(), Shape::Vertical).unwrap();
        assert_eq!(size, (606, 1080));
        assert_eq!(shown(&vertical), (606, 1080));
        let (square, size) = shaped_sps(&sps_1080(), Shape::Square).unwrap();
        assert_eq!(size, (1080, 1080));
        assert_eq!(shown(&square), (1080, 1080));
    }

    #[test]
    fn a_tall_picture_is_cut_top_and_bottom_for_a_square() {
        // Rewritten as vertical first, then made square: 606 wide, so 606 tall.
        let (vertical, _) = shaped_sps(&sps_1080(), Shape::Vertical).unwrap();
        let (square, size) = shaped_sps(&vertical, Shape::Square).unwrap();
        assert_eq!(size, (606, 606));
        assert_eq!(shown(&square), (606, 606));
    }

    #[test]
    fn escaping_round_trips_and_guards_start_codes() {
        let raw = [0u8, 0, 1, 0, 0, 0, 0, 0, 3, 5];
        let escaped = escape(&raw);
        assert!(!escaped.windows(3).any(|w| w == [0, 0, 1]), "{escaped:?}");
        assert_eq!(unescape(&escaped), raw);
    }

    #[test]
    fn words_name_shapes_both_ways() {
        for shape in [Shape::Vertical, Shape::Square] {
            assert_eq!(Shape::named(shape.name()), Some(shape));
        }
        assert_eq!(Shape::named("round"), None);
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn media(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/media/mp4mux")
            .join(name)
    }

    /// What an SPS says, for the builder below.
    #[derive(Clone, Copy)]
    struct Sps {
        profile: u64,
        /// `chroma_format_idc`, and the separate-planes flag where it is 3.
        chroma: Option<(u64, u64)>,
        scaling: bool,
        poc: u64,
        frames_only: u64,
        width_mbs: u64,
        height_units: u64,
        crop: Option<[u64; 4]>,
        vui: bool,
    }

    const PLAIN: Sps = Sps {
        profile: 77,
        chroma: None,
        scaling: false,
        poc: 0,
        frames_only: 1,
        width_mbs: 20,
        height_units: 10,
        crop: None,
        vui: false,
    };

    fn put_se(w: &mut Writer, v: i64) {
        w.put_ue(if v > 0 {
            2 * v as u64 - 1
        } else {
            (-2 * v) as u64
        });
    }

    /// The SPS NAL unit `s` describes, written bit by bit as H.264 lays it
    /// out (7.3.2.1.1).
    fn sps(s: Sps) -> Vec<u8> {
        let mut w = Writer::default();
        w.put(s.profile, 8);
        w.put(0, 8);
        w.put(30, 8);
        w.put_ue(0); // id
        if let Some((chroma, separate)) = s.chroma {
            w.put_ue(chroma);
            if chroma == 3 {
                w.put(separate, 1);
            }
            w.put_ue(0); // luma bit depth - 8
            w.put_ue(0); // chroma bit depth - 8
            w.put(0, 1); // lossless bypass
            w.put(u64::from(s.scaling), 1);
            if s.scaling {
                let lists = if chroma == 3 { 12 } else { 8 };
                for i in 0..lists {
                    // Every other list present: the first of them given in
                    // full (a delta of 0 sixteen or sixty-four times), the
                    // rest ended at once (a delta to 0, "use the default").
                    let present = i % 2 == 0;
                    w.put(u64::from(present), 1);
                    if present {
                        if i == 0 {
                            for _ in 0..16 {
                                put_se(&mut w, 0);
                            }
                        } else {
                            put_se(&mut w, -8);
                        }
                    }
                }
            }
        }
        w.put_ue(0); // log2 max frame num - 4
        w.put_ue(s.poc);
        match s.poc {
            0 => w.put_ue(2),
            1 => {
                w.put(1, 1); // delta pic order always zero
                put_se(&mut w, -3); // offset for non-reference pictures
                put_se(&mut w, 5); // offset top to bottom
                w.put_ue(2); // two frames in the cycle
                put_se(&mut w, 1);
                put_se(&mut w, -1);
            }
            _ => {}
        }
        w.put_ue(1); // reference frames
        w.put(0, 1); // gaps
        w.put_ue(s.width_mbs - 1);
        w.put_ue(s.height_units - 1);
        w.put(s.frames_only, 1);
        if s.frames_only == 0 {
            w.put(1, 1); // adaptive frame/field
        }
        w.put(1, 1); // direct 8x8
        match s.crop {
            Some(crop) => {
                w.put(1, 1);
                for side in crop {
                    w.put_ue(side);
                }
            }
            None => w.put(0, 1),
        }
        w.put(u64::from(s.vui), 1);
        if s.vui {
            w.put(1, 1); // aspect ratio info
            w.put(255, 8); // extended
            w.put(4, 16);
            w.put(3, 16);
            w.put(0, 6);
        }
        w.put(1, 1); // stop bit
        let mut nal = vec![0x67];
        nal.extend_from_slice(&escape(&w.finish()));
        nal
    }

    /// Shaped, the SPS shows `size`; shaped again the same way it reads
    /// back whole (the crop written, and every field after it), and where
    /// the size was the ratio exactly nothing changes.
    fn shapes_to(s: Sps, shape: Shape, size: (u64, u64)) -> Vec<u8> {
        let (out, shown) = shaped_sps(&sps(s), shape).unwrap();
        assert_eq!(shown, size, "{}", shape.name());
        let (again, shown_again) = shaped_sps(&out, shape).unwrap();
        assert!(
            shown_again.0 <= size.0 && shown_again.1 <= size.1,
            "shaped twice it grew: {shown_again:?}"
        );
        if shown_again == size {
            assert_eq!(again, out, "shaping a shaped SPS changed it");
        }
        out
    }

    /// Every chroma layout and both kinds of picture order count give the
    /// size they should, the crop in that layout's own units.
    #[test]
    fn every_chroma_layout_and_order_count_is_shaped_in_its_own_units() {
        // Monochrome: crop units of one pixel. 320 by 160 made 9:16 is 90 wide.
        let mono = Sps {
            profile: 100,
            chroma: Some((0, 0)),
            ..PLAIN
        };
        shapes_to(mono, Shape::Vertical, (90, 160));
        // 4:2:2, interlaced (each height unit two macroblock rows): 160 by
        // 128, square by cutting its sides in units of two pixels.
        let field = Sps {
            profile: 122,
            chroma: Some((2, 0)),
            frames_only: 0,
            width_mbs: 10,
            height_units: 4,
            vui: true,
            ..PLAIN
        };
        shapes_to(field, Shape::Square, (128, 128));
        // 4:4:4 coded as separate planes reads as monochrome: 256 by 144
        // made 9:16 is 81 wide, an odd number allowed in one-pixel units.
        let planes = Sps {
            profile: 244,
            chroma: Some((3, 1)),
            width_mbs: 16,
            height_units: 9,
            ..PLAIN
        };
        shapes_to(planes, Shape::Vertical, (81, 144));
        // 4:4:4 with its twelve scaling lists, some given, some defaulted.
        let scaled = Sps {
            profile: 244,
            chroma: Some((3, 0)),
            scaling: true,
            width_mbs: 16,
            height_units: 9,
            ..PLAIN
        };
        shapes_to(scaled, Shape::Square, (144, 144));
        // 4:2:0 High with eight scaling lists, and the order count of
        // type 1 (its cycle of offsets) and of type 2 (nothing more).
        let high = Sps {
            profile: 100,
            chroma: Some((1, 0)),
            scaling: true,
            poc: 1,
            vui: true,
            ..PLAIN
        };
        shapes_to(high, Shape::Vertical, (90, 160));
        shapes_to(Sps { poc: 2, ..PLAIN }, Shape::Square, (160, 160));
        // Interlaced 4:2:0: a height unit of four rows. 320 by 320 is made
        // 9:16 by cutting its sides: 180 wide.
        shapes_to(
            Sps {
                frames_only: 0,
                height_units: 10,
                ..PLAIN
            },
            Shape::Vertical,
            (180, 320),
        );
        // A picture already taller than 9:16 loses rows, in whole units.
        shapes_to(
            Sps {
                width_mbs: 4,
                height_units: 20,
                ..PLAIN
            },
            Shape::Vertical,
            (64, 112),
        );
    }

    /// The crop already in the SPS is kept and added to.
    #[test]
    fn a_crop_already_there_is_counted_before_the_new_one() {
        // 320 by 160 cropped to 300 by 150 (units of two pixels).
        let cropped = Sps {
            crop: Some([5, 5, 0, 5]),
            ..PLAIN
        };
        shapes_to(cropped, Shape::Square, (150, 150));
        let none_left = Sps {
            width_mbs: 1,
            height_units: 1,
            crop: Some([4, 4, 0, 0]),
            ..PLAIN
        };
        assert_eq!(
            shaped_sps(&sps(none_left), Shape::Square).unwrap_err(),
            "the video's picture has no size"
        );
    }

    /// An SPS that is empty, cut short anywhere, or with an Exp-Golomb
    /// number longer than any is refused, never read past its end.
    #[test]
    fn an_sps_cut_short_is_refused_wherever_it_ends() {
        assert_eq!(shaped_sps(&[], Shape::Square).unwrap_err(), "an empty SPS");
        for s in [
            PLAIN,
            Sps {
                profile: 100,
                chroma: Some((3, 0)),
                scaling: true,
                poc: 1,
                frames_only: 0,
                crop: Some([1, 1, 1, 1]),
                ..PLAIN
            },
        ] {
            // With no VUI after the crop, the last two bytes hold the crop's
            // end: cut anywhere before them, the SPS is cut short.
            let whole = sps(s);
            for len in 1..whole.len() - 2 {
                let said = shaped_sps(&whole[..len], Shape::Vertical).unwrap_err();
                assert_eq!(said, "the SPS is cut short", "cut to {len} bytes");
            }
        }
        // Forty zero bits where a number should begin.
        let mut long = vec![0x67, 77, 0, 30];
        long.extend_from_slice(&[0; 6]);
        assert_eq!(
            shaped_sps(&long, Shape::Vertical).unwrap_err(),
            "the SPS is cut short"
        );
    }

    /// An `avcC` with this SPS, and one picture parameter set.
    fn avcc(sps_nal: &[u8]) -> Vec<u8> {
        let mut out = vec![1, 77, 0, 30, 0xff, 0xe1];
        out.extend_from_slice(&(sps_nal.len() as u16).to_be_bytes());
        out.extend_from_slice(sps_nal);
        out.extend_from_slice(&[1, 0, 4, 0x68, 0xee, 0x3c, 0x80]);
        out
    }

    /// A visual sample entry of `kind`, `width` by `height`, with `children`.
    fn entry(kind: &[u8; 4], width: u16, height: u16, children: &[u8]) -> Vec<u8> {
        let mut fields = vec![0u8; 78];
        fields[6..8].copy_from_slice(&1u16.to_be_bytes()); // data reference
        fields[24..26].copy_from_slice(&width.to_be_bytes());
        fields[26..28].copy_from_slice(&height.to_be_bytes());
        fields.extend_from_slice(children);
        boxed(kind, &fields)
    }

    /// A whole `stsd` box holding `entries`.
    fn stsd(entries: &[&[u8]]) -> Vec<u8> {
        let mut body = vec![0, 0, 0, 0];
        body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for e in entries {
            body.extend_from_slice(e);
        }
        boxed(b"stsd", &body)
    }

    /// The width and height an `stsd`'s first entry states.
    fn stated(stsd: &[u8]) -> (u16, u16) {
        (
            u16::from_be_bytes([stsd[48], stsd[49]]),
            u16::from_be_bytes([stsd[50], stsd[51]]),
        )
    }

    /// The sample description is rewritten whole: its stated size, its
    /// `avcC`'s SPS, the boxes beside it and any later entry as they were;
    /// the SPS's picture parameter set after it, untouched.
    #[test]
    fn a_sample_description_is_shaped_and_keeps_everything_else() {
        let pasp = boxed(b"pasp", &[0, 0, 0, 1, 0, 0, 0, 1]);
        let first_sps = sps(PLAIN);
        let children = [pasp.clone(), boxed(b"avcC", &avcc(&first_sps))].concat();
        let first = entry(b"avc3", 320, 160, &children);
        let second = entry(b"avc1", 1, 1, &[]);
        let before = stsd(&[&first, &second]);
        let (after, size) = shaped_stsd(&before, Shape::Vertical).unwrap();
        assert_eq!(size, (90, 160));
        assert_eq!(stated(&after), (90, 160));
        assert_eq!(u32_at(&after, 0).unwrap() as usize, after.len());
        assert_eq!(&after[4..16], &before[4..16], "version and entry count");
        assert_eq!(&after[20..24], b"avc3", "the entry's kind");
        assert!(after.ends_with(&second), "the second entry was lost");
        let at = after.windows(4).position(|w| w == b"pasp").unwrap();
        assert_eq!(&after[at - 4..at - 4 + pasp.len()], &pasp[..]);
        let (shaped_sps_nal, _) = shaped_sps(&first_sps, Shape::Vertical).unwrap();
        let expected_avcc = avcc(&shaped_sps_nal);
        assert!(
            after
                .windows(expected_avcc.len())
                .any(|w| w == expected_avcc),
            "the avcC is not the shaped SPS and the PPS as it was"
        );
    }

    /// Each way a sample description can be broken is refused in words.
    #[test]
    fn a_broken_sample_description_is_refused_in_words() {
        let good_avcc = boxed(b"avcC", &avcc(&sps(PLAIN)));
        let cases: Vec<(Vec<u8>, &str)> = vec![
            (vec![0; 10], "the sample description is cut short"),
            (
                {
                    let mut s = stsd(&[&entry(b"avc1", 1, 1, &good_avcc)]);
                    s.truncate(s.len() - 1);
                    s
                },
                "the sample description is cut short",
            ),
            (
                stsd(&[&boxed(b"avc1", &[0; 20])]),
                "the video's description is cut short",
            ),
            (
                stsd(&[&[0, 0, 0, 6, b'a', b'v']]),
                "the sample description is cut short",
            ),
            (
                stsd(&[&entry(b"avc1", 1, 1, &[0, 0, 0, 4, 0, 0, 0, 0])]),
                "a box is cut short",
            ),
            (
                stsd(&[&entry(
                    b"avc1",
                    1,
                    1,
                    &[0, 0, 0, 40, b'f', b'r', b'e', b'e'],
                )]),
                "a box is cut short",
            ),
            (
                stsd(&[&entry(b"avc1", 1, 1, &boxed(b"pasp", &[0; 8]))]),
                "the video has no H.264 settings (avcC)",
            ),
        ];
        for (bytes, said) in cases {
            assert_eq!(shaped_stsd(&bytes, Shape::Square).unwrap_err(), said);
        }
        let hevc = stsd(&[&entry(b"hvc1", 1, 1, &[])]);
        assert_eq!(
            shaped_stsd(&hevc, Shape::Square).unwrap_err(),
            "only an H.264 video can be made square; this one is hvc1"
        );
    }

    /// An `avcC` cut short, or with no SPS, is refused in words.
    #[test]
    fn a_broken_avcc_is_refused_in_words() {
        let whole = avcc(&sps(PLAIN));
        assert_eq!(
            shaped_avcc(&whole[..4], Shape::Square).unwrap_err(),
            "the H.264 settings are cut short"
        );
        assert_eq!(
            shaped_avcc(&whole[..5], Shape::Square).unwrap_err(),
            "the H.264 settings are cut short"
        );
        assert_eq!(
            shaped_avcc(&whole[..7], Shape::Square).unwrap_err(),
            "an SPS is cut short"
        );
        assert_eq!(
            shaped_avcc(&whole[..12], Shape::Square).unwrap_err(),
            "an SPS is cut short"
        );
        let mut none = whole.clone();
        none[5] = 0xe0;
        assert_eq!(
            shaped_avcc(&none, Shape::Square).unwrap_err(),
            "the video has no SPS"
        );
        // Two SPSs: both shaped, the size the first shows.
        let mut two = vec![1, 77, 0, 30, 0xff, 0xe2];
        for s in [
            PLAIN,
            Sps {
                width_mbs: 40,
                ..PLAIN
            },
        ] {
            let nal = sps(s);
            two.extend_from_slice(&(nal.len() as u16).to_be_bytes());
            two.extend_from_slice(&nal);
        }
        let (_, size) = shaped_avcc(&two, Shape::Square).unwrap();
        assert_eq!(size, (160, 160));
    }

    /// `apply` shapes a picture's description and its header's size, and
    /// leaves a sound alone and a short header as it is.
    #[test]
    fn apply_shapes_a_picture_and_leaves_a_sound_alone() {
        let hdlr = |kind: &[u8; 4]| {
            let mut h = vec![0u8; 32];
            h[16..20].copy_from_slice(kind);
            h
        };
        let description = stsd(&[&entry(
            b"avc1",
            320,
            160,
            &boxed(b"avcC", &avcc(&sps(PLAIN))),
        )]);
        let mut picture = Track {
            hdlr: hdlr(b"vide"),
            stsd: description.clone(),
            tkhd_tail: vec![7; 60],
            ..Track::default()
        };
        apply(&mut picture, Shape::Vertical).unwrap();
        assert_eq!(stated(&picture.stsd), (90, 160));
        assert_eq!(&picture.tkhd_tail[..52], &[7; 52][..]);
        assert_eq!(u32_at(&picture.tkhd_tail, 52), Some(90 << 16));
        assert_eq!(u32_at(&picture.tkhd_tail, 56), Some(160 << 16));

        let mut short = Track {
            hdlr: hdlr(b"vide"),
            stsd: description.clone(),
            tkhd_tail: vec![7; 20],
            ..Track::default()
        };
        apply(&mut short, Shape::Square).unwrap();
        assert_eq!(stated(&short.stsd), (160, 160));
        assert_eq!(short.tkhd_tail, vec![7; 20]);

        let mut sound = Track {
            hdlr: hdlr(b"soun"),
            stsd: vec![1, 2, 3],
            tkhd_tail: vec![7; 60],
            ..Track::default()
        };
        apply(&mut sound, Shape::Vertical).unwrap();
        assert_eq!(sound.stsd, [1, 2, 3]);
        assert_eq!(sound.tkhd_tail, vec![7; 60]);
    }

    /// A real H.264 picture joined with its sound, made vertical and
    /// square: the picture states its new size in its description and its
    /// header, the sound is the same as in a plain join.
    #[test]
    fn a_joined_video_is_shaped_and_its_sound_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let inputs = [media("picture.mp4"), media("sound.m4a")];
        let plain = dir.path().join("plain.mp4");
        crate::mp4mux::join(&inputs, &plain).unwrap();
        let plain = crate::mp4mux::tracks_in_file(&plain).unwrap();
        let sound_of = |tracks: &[Track]| {
            tracks
                .iter()
                .find(|t| t.hdlr.get(16..20) == Some(b"soun"))
                .map(|t| (t.stsd.clone(), t.tkhd_tail.clone()))
                .unwrap()
        };
        assert_eq!(
            stated(
                &plain
                    .iter()
                    .find(|t| t.hdlr[16..20] == *b"vide")
                    .unwrap()
                    .stsd
            ),
            (160, 90)
        );
        for (shape, size) in [(Shape::Vertical, (50, 90)), (Shape::Square, (90, 90))] {
            let out = dir.path().join(format!("{}.mp4", shape.name()));
            crate::mp4mux::join_cut(&inputs, &[], &out, None, Some(shape)).unwrap();
            let tracks = crate::mp4mux::tracks_in_file(&out).unwrap();
            let video = tracks
                .iter()
                .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
                .unwrap();
            assert_eq!(stated(&video.stsd), size, "{}", shape.name());
            let tail = &video.tkhd_tail;
            assert_eq!(u32_at(tail, 52), Some(u32::from(size.0) << 16));
            assert_eq!(u32_at(tail, 56), Some(u32::from(size.1) << 16));
            assert_eq!(sound_of(&tracks), sound_of(&plain), "the sound changed");
        }
    }

    /// VP9 and AV1 pictures are refused in words, naming the shape and
    /// the kind of picture.
    #[test]
    fn a_vp9_or_av1_picture_is_refused_in_words() {
        let dir = tempfile::tempdir().unwrap();
        for (name, kind) in [("vp9.f313.webm", "vp09"), ("av1.f399.webm", "av01")] {
            let out = dir.path().join("out.mp4");
            let said =
                crate::mp4mux::join_cut(&[media(name)], &[], &out, None, Some(Shape::Vertical))
                    .unwrap_err();
            assert_eq!(
                said,
                format!("only an H.264 video can be made vertical; this one is {kind}")
            );
            assert!(!out.exists(), "a file was written for a refused shape");
        }
    }

    /// A shape is stored as its lowercase word and read back from it.
    #[test]
    fn a_shape_is_stored_as_its_word() {
        assert_eq!(
            serde_json::to_string(&Shape::Vertical).unwrap(),
            "\"vertical\""
        );
        assert_eq!(
            serde_json::from_str::<Shape>("\"square\"").unwrap(),
            Shape::Square
        );
        assert_eq!(Shape::named(" Vertical "), Some(Shape::Vertical));
        assert_eq!(Shape::named("SQUARE"), Some(Shape::Square));
    }

    /// Escaping puts a 3 before every 0, 1, 2 or 3 that follows two zeros,
    /// and unescaping takes exactly those out again.
    #[test]
    fn escaping_guards_every_byte_a_start_code_could_begin_with() {
        for last in 0..=3u8 {
            let raw = [0, 0, last, 9];
            let escaped = escape(&raw);
            assert_eq!(escaped, [0, 0, 3, last, 9], "{last}");
            assert_eq!(unescape(&escaped), raw);
        }
        assert_eq!(escape(&[0, 0, 4]), [0, 0, 4]);
        // No stop bit to be found: nothing is left before it.
        let zeros = [0u8; 2];
        let bits = Bits::new(&zeros);
        assert_eq!(bits.remaining_before_stop(), 0);
    }
}
