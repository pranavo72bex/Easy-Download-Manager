//! Reading a transport stream: its packets, tables and timestamps.

use super::*;

pub(super) fn read_ts(
    input: &Path,
    scratch: &dyn Fn(&str) -> PathBuf,
) -> Result<Vec<Elementary>, String> {
    let file = File::open(input).map_err(|e| format!("the stream could not be read: {e}"))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut packet = [0u8; PACKET];
    let mut pmt_pids: Vec<u16> = Vec::new();
    // PID → stream, in the order the tables named them.
    let mut streams: Vec<(u16, Elementary)> = Vec::new();
    let mut lost_sync = 0u32;
    loop {
        match reader.read_exact(&mut packet) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("the stream could not be read: {e}")),
        }
        if packet[0] != 0x47 {
            // Out of step: find the next sync byte and carry on from it.
            lost_sync += 1;
            if lost_sync > 1000 {
                return Err("it is not a transport stream".into());
            }
            let Some(at) = packet.iter().skip(1).position(|b| *b == 0x47) else {
                continue;
            };
            let mut rest = vec![0u8; at + 1];
            reader
                .read_exact(&mut rest)
                .map_err(|_| "the stream ends mid-packet".to_string())?;
            let mut shifted = packet.get(at + 1..).unwrap_or_default().to_vec();
            shifted.extend_from_slice(&rest);
            packet.copy_from_slice(&shifted);
            if packet[0] != 0x47 {
                continue;
            }
        }
        let start = packet[1] & 0x40 != 0;
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let control = (packet[3] >> 4) & 3;
        let mut at = 4usize;
        if control & 2 != 0 {
            at += 1 + usize::from(packet[4]);
        }
        if control & 1 == 0 {
            continue;
        }
        let Some(payload) = packet.get(at..).filter(|p| !p.is_empty()) else {
            continue;
        };
        if pid == 0 && start {
            pmt_pids = read_pat(payload);
        } else if pmt_pids.contains(&pid) && start {
            for (kind, es_pid) in read_pmt(payload) {
                if OTHERS.contains(&kind) {
                    for (_, stream) in &streams {
                        let _ = std::fs::remove_file(&stream.path);
                    }
                    return Err(format!(
                        "it carries a kind of picture or sound (stream type {kind:#04x}) dm does \
                         not put into an MP4"
                    ));
                }
                let picture = PICTURES.contains(&kind);
                if !streams.iter().any(|(p, _)| *p == es_pid) && (picture || SOUNDS.contains(&kind))
                {
                    let has_kind = streams
                        .iter()
                        .any(|(_, s)| PICTURES.contains(&s.kind) == picture);
                    // One picture and one sound: the first of each.
                    if !has_kind {
                        let path = scratch(if picture { "video" } else { "audio" });
                        let out = File::create(&path)
                            .map_err(|e| format!("a scratch file could not be made: {e}"))?;
                        streams.push((
                            es_pid,
                            Elementary {
                                kind,
                                out: Some(BufWriter::with_capacity(1 << 20, out)),
                                path,
                                ..Elementary::default()
                            },
                        ));
                    }
                }
            }
        } else if let Some((_, stream)) = streams.iter_mut().find(|(p, _)| *p == pid) {
            if start && !stream.pes.is_empty() {
                let pes = std::mem::take(&mut stream.pes);
                take_pes(stream, &pes)?;
            }
            if start || !stream.pes.is_empty() {
                stream.pes.extend_from_slice(payload);
            }
        }
    }
    let mut out = Vec::new();
    for (_, mut stream) in streams {
        if !stream.pes.is_empty() {
            let pes = std::mem::take(&mut stream.pes);
            take_pes(&mut stream, &pes)?;
        }
        if let Some(mut writer) = stream.out.take() {
            writer
                .flush()
                .map_err(|e| format!("a scratch file could not be written: {e}"))?;
        }
        out.push(stream);
    }
    Ok(out)
}

/// The section a PSI packet's payload carries, past its pointer field.
pub(super) fn section(payload: &[u8]) -> Option<&[u8]> {
    let pointer = usize::from(*payload.first()?);
    let section = payload.get(1 + pointer..)?;
    let length = usize::from(u16::from_be_bytes([*section.get(1)?, *section.get(2)?]) & 0x0fff);
    section.get(..3 + length)
}

pub(super) fn read_pat(payload: &[u8]) -> Vec<u16> {
    let Some(section) = section(payload) else {
        return Vec::new();
    };
    // Programs from byte 8 to the CRC.
    let end = section.len().saturating_sub(4);
    section
        .get(8..end)
        .unwrap_or_default()
        .as_chunks::<4>()
        .0
        .iter()
        // Program 0 names the network table, not a program's.
        .filter(|[a, b, _, _]| *a != 0 || *b != 0)
        .map(|[_, _, high, low]| (u16::from(high & 0x1f) << 8) | u16::from(*low))
        .collect()
}

pub(super) fn read_pmt(payload: &[u8]) -> Vec<(u8, u16)> {
    let Some(section) = section(payload) else {
        return Vec::new();
    };
    let Some(info) = section
        .get(10..12)
        .and_then(|b| b.try_into().ok())
        .map(|b: [u8; 2]| usize::from(u16::from_be_bytes(b) & 0x0fff))
    else {
        return Vec::new();
    };
    let end = section.len().saturating_sub(4);
    let mut at = 12 + info;
    let mut found = Vec::new();
    while at + 5 <= end {
        let (Some(kind), Some(p1), Some(p2), Some(l1), Some(l2)) = (
            section.get(at),
            section.get(at + 1),
            section.get(at + 2),
            section.get(at + 3),
            section.get(at + 4),
        ) else {
            break;
        };
        found.push((*kind, (u16::from(p1 & 0x1f) << 8) | u16::from(*p2)));
        at += 5 + usize::from((u16::from(l1 & 0x0f) << 8) | u16::from(*l2));
    }
    found
}

pub(super) fn timestamp(b: &[u8]) -> Option<i64> {
    let b: &[u8; 5] = b.get(..5)?.try_into().ok()?;
    Some(
        (i64::from(b[0] >> 1) & 7) << 30
            | i64::from(b[1]) << 22
            | i64::from(b[2] >> 1) << 15
            | i64::from(b[3]) << 7
            | i64::from(b[4] >> 1),
    )
}

/// A timestamp carried past a wrap, so it keeps rising.
pub(super) fn unwrap(value: i64, last: Option<i64>) -> i64 {
    let Some(last) = last else {
        return value;
    };
    let mut v = value + (last / WRAP) * WRAP;
    if v < last - WRAP / 2 {
        v += WRAP;
    } else if v > last + WRAP / 2 {
        v -= WRAP;
    }
    v
}

/// One whole PES packet: its timestamps, and its payload as samples.
pub(super) fn take_pes(stream: &mut Elementary, pes: &[u8]) -> Result<(), String> {
    if pes.get(..3) != Some(&[0, 0, 1]) {
        return Ok(());
    }
    let flags = pes.get(7).copied().unwrap_or(0) >> 6;
    let header = usize::from(pes.get(8).copied().unwrap_or(0));
    let pts = if flags & 2 != 0 {
        pes.get(9..).and_then(timestamp)
    } else {
        None
    };
    let dts = if flags == 3 {
        pes.get(14..).and_then(timestamp)
    } else {
        None
    };
    let Some(data) = pes.get(9 + header..) else {
        return Ok(());
    };
    let pts = pts.map(|p| unwrap(p, stream.last_pts));
    let dts = dts.map(|d| unwrap(d, pts.or(stream.last_pts)));
    if let Some(p) = pts {
        stream.last_pts = Some(p);
    }
    if PICTURES.contains(&stream.kind) {
        take_picture(stream, data, pts, dts)
    } else {
        take_sound(stream, data, pts)
    }
}
