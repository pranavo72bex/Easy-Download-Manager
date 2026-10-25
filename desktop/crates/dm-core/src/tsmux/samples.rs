//! Cutting pictures into samples.

use super::*;

/// The NAL units in an Annex B byte stream.
pub(super) fn nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data.get(i..i + 3) == Some(&[0, 0, 1]) {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut units = Vec::new();
    for (n, start) in starts.iter().enumerate() {
        let mut end = starts.get(n + 1).map_or(data.len(), |next| next - 3);
        // A four-byte start code's leading zero, and trailing zeros.
        while end > *start && data.get(end - 1) == Some(&0) {
            end -= 1;
        }
        if let Some(unit) = data.get(*start..end).filter(|u| !u.is_empty()) {
            units.push(unit);
        }
    }
    units
}

pub(super) fn take_picture(
    stream: &mut Elementary,
    data: &[u8],
    pts: Option<i64>,
    dts: Option<i64>,
) -> Result<(), String> {
    let mut sample = Vec::with_capacity(data.len() + 16);
    let mut key = false;
    let hevc = stream.kind == HEVC;
    for unit in nal_units(data) {
        let Some(head) = unit.first() else {
            continue;
        };
        /* What each unit is: a parameter set (kept for the sample
         * description, not the samples), a delimiter or filler (dropped),
         * or part of the picture, a key frame's or not. */
        let (parameter, dropped, keyframe) = if hevc {
            let kind = (head >> 1) & 0x3f;
            let parameter = match kind {
                32 => Some(&mut stream.vps),
                33 => Some(&mut stream.sps),
                34 => Some(&mut stream.pps),
                _ => None,
            };
            (
                parameter,
                matches!(kind, 35 | 38),
                (16..=21).contains(&kind),
            )
        } else {
            let kind = head & 0x1f;
            let parameter = match kind {
                7 => Some(&mut stream.sps),
                8 => Some(&mut stream.pps),
                _ => None,
            };
            (parameter, matches!(kind, 9 | 12), kind == 5)
        };
        if let Some(slot) = parameter {
            slot.get_or_insert_with(|| unit.to_vec());
            continue;
        }
        if dropped {
            continue;
        }
        key |= keyframe;
        sample.extend_from_slice(&(unit.len() as u32).to_be_bytes());
        sample.extend_from_slice(unit);
    }
    if sample.is_empty() {
        return Ok(());
    }
    let Some(pts) = pts else {
        // A picture with no time is the rest of the one before it.
        return append_to_last(stream, &sample, key);
    };
    write(stream, &sample)?;
    let offset = stream.written - sample.len() as u64;
    stream.first_pts = Some(stream.first_pts.map_or(pts, |f| f.min(pts)));
    stream
        .samples
        .push((offset, sample.len() as u32, pts, dts.unwrap_or(pts), key));
    Ok(())
}

pub(super) fn append_to_last(
    stream: &mut Elementary,
    sample: &[u8],
    key: bool,
) -> Result<(), String> {
    if stream.samples.is_empty() {
        return Ok(());
    }
    write(stream, sample)?;
    if let Some(last) = stream.samples.last_mut() {
        last.1 += sample.len() as u32;
        last.4 |= key;
    }
    Ok(())
}

pub(super) fn write(stream: &mut Elementary, bytes: &[u8]) -> Result<(), String> {
    let out = stream.out.as_mut().ok_or("a scratch file went missing")?;
    out.write_all(bytes)
        .map_err(|e| format!("a scratch file could not be written: {e}"))?;
    stream.written += bytes.len() as u64;
    Ok(())
}
