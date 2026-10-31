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
