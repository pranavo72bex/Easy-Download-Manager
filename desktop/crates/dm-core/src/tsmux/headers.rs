//! Reading a picture's size from its parameter sets, and the boxes both muxers share.

use super::*;

/// Reads an H.264 sequence parameter set's picture size.
pub(crate) fn picture_size(sps: &[u8]) -> Option<(u32, u32)> {
    // Past the one-byte NAL header, emulation prevention bytes out.
    let data = rbsp(sps.get(1..)?);
    let mut bits = Bits { data: &data, at: 0 };
    let profile = bits.take(8)?;
    bits.take(16)?; // constraints, level
    bits.ue()?; // seq_parameter_set_id
    let mut chroma = 1;
    let mut separate = false;
    if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].contains(&profile) {
        chroma = bits.ue()?;
        if chroma == 3 {
            separate = bits.take(1)? == 1;
        }
        bits.ue()?;
        bits.ue()?;
        bits.take(1)?;
        if bits.take(1)? == 1 {
            for i in 0..if chroma == 3 { 12 } else { 8 } {
                if bits.take(1)? == 1 {
                    let size = if i < 6 { 16 } else { 64 };
                    let (mut last, mut next) = (8i64, 8i64);
                    for _ in 0..size {
                        if next != 0 {
                            next = (last + bits.se()? + 256) % 256;
                        }
                        if next != 0 {
                            last = next;
                        }
                    }
                }
            }
        }
    }
    bits.ue()?; // log2_max_frame_num
    match bits.ue()? {
        0 => {
            bits.ue()?;
        }
        1 => {
            bits.take(1)?;
            bits.se()?;
            bits.se()?;
            for _ in 0..bits.ue()?.min(256) {
                bits.se()?;
            }
        }
        _ => {}
    }
    bits.ue()?; // max_num_ref_frames
    bits.take(1)?;
    let width_mbs = bits.ue()? + 1;
    let height_units = bits.ue()? + 1;
    let frame_only = bits.take(1)?;
    if frame_only == 0 {
        bits.take(1)?;
    }
    bits.take(1)?;
    let (mut left, mut right, mut top, mut bottom) = (0, 0, 0, 0);
    if bits.take(1)? == 1 {
        left = bits.ue()?;
        right = bits.ue()?;
        top = bits.ue()?;
        bottom = bits.ue()?;
    }
    let array_type = if separate { 0 } else { chroma };
    let (sub_w, sub_h) = match array_type {
        1 => (2, 2),
        2 => (2, 1),
        _ => (1, 1),
    };
    let (unit_x, unit_y) = if array_type == 0 {
        (1, 2 - frame_only)
    } else {
        (sub_w, sub_h * (2 - frame_only))
    };
    let width = (width_mbs * 16).checked_sub((left + right) * unit_x)?;
    let height = ((2 - frame_only) * height_units * 16).checked_sub((top + bottom) * unit_y)?;
    Some((width, height))
}

pub(super) struct Bits<'a> {
    pub(super) data: &'a [u8],
    pub(super) at: usize,
}

impl Bits<'_> {
    pub(super) fn take(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.data.get(self.at / 8)?;
            v = (v << 1) | u32::from((byte >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Some(v)
    }

    pub(super) fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.take(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1u32 << zeros) - 1 + self.take(zeros)?)
    }

    pub(super) fn se(&mut self) -> Option<i64> {
        let v = i64::from(self.ue()?);
        Some(if v % 2 == 1 { (v + 1) / 2 } else { -(v / 2) })
    }
}

pub(crate) fn hdlr(kind: &[u8; 4], name: &str) -> Vec<u8> {
    let mut body = vec![0u8; 4];
    body.extend_from_slice(kind);
    body.extend_from_slice(&[0u8; 12]);
    body.extend_from_slice(name.as_bytes());
    body.push(0);
    full(b"hdlr", 0, 0, &body)
}

pub(crate) fn tkhd_tail(volume: u16, width: u32, height: u32) -> Vec<u8> {
    let mut tail = vec![0u8; 8];
    tail.extend_from_slice(&0u16.to_be_bytes());
    tail.extend_from_slice(&0u16.to_be_bytes());
    tail.extend_from_slice(&volume.to_be_bytes());
    tail.extend_from_slice(&0u16.to_be_bytes());
    for m in MATRIX {
        tail.extend_from_slice(&m.to_be_bytes());
    }
    tail.extend_from_slice(&(width << 16).to_be_bytes());
    tail.extend_from_slice(&(height << 16).to_be_bytes());
    tail
}

/// An RBSP: a NAL unit's payload with its emulation prevention bytes out.
pub(super) fn rbsp(unit: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(unit.len());
    let mut zeros = 0;
    for b in unit {
        if zeros >= 2 && *b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if *b == 0 { zeros + 1 } else { 0 };
        out.push(*b);
    }
    out
}

/// What an HEVC sequence parameter set says that its `hvcC` repeats: the
/// profile, tier and level bytes as they are, the sub-layers, the chroma
/// format and bit depths, and the picture's size.
pub(crate) struct HevcSps {
    pub(crate) profile: Vec<u8>,
    pub(crate) sub_layers: u32,
    pub(crate) nested: bool,
    pub(crate) chroma: u32,
    pub(crate) luma_bits: u32,
    pub(crate) chroma_bits: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn hevc_sps(sps: &[u8]) -> Option<HevcSps> {
    // Past the two-byte NAL header.
    let data = rbsp(sps.get(2..)?);
    let mut bits = Bits { data: &data, at: 0 };
    bits.take(4)?;
    let max_sub_layers = bits.take(3)?;
    let nested = bits.take(1)? == 1;
    // general_profile_space … general_level_idc: twelve whole bytes.
    let profile = data.get(1..13)?.to_vec();
    bits.at = 13 * 8;
    let mut present = Vec::new();
    for _ in 0..max_sub_layers {
        present.push((bits.take(1)?, bits.take(1)?));
    }
    if max_sub_layers > 0 {
        for _ in max_sub_layers..8 {
            bits.take(2)?;
        }
    }
    for (profile_present, level_present) in present {
        if profile_present == 1 {
            bits.at += 88;
        }
        if level_present == 1 {
            bits.at += 8;
        }
    }
    bits.ue()?;
    let chroma = bits.ue()?;
    if chroma == 3 {
        bits.take(1)?;
    }
    let mut width = bits.ue()?;
    let mut height = bits.ue()?;
    if bits.take(1)? == 1 {
        let (sub_w, sub_h) = match chroma {
            1 => (2, 2),
            2 => (2, 1),
            _ => (1, 1),
        };
        let (left, right, top, bottom) = (bits.ue()?, bits.ue()?, bits.ue()?, bits.ue()?);
        width = width.checked_sub((left + right) * sub_w)?;
        height = height.checked_sub((top + bottom) * sub_h)?;
    }
    let luma_bits = bits.ue()? + 8;
    let chroma_bits = bits.ue()? + 8;
    Some(HevcSps {
        profile,
        sub_layers: max_sub_layers + 1,
        nested,
        chroma,
        luma_bits,
        chroma_bits,
        width,
        height,
    })
}
