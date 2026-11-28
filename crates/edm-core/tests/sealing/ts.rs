// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A transport stream's PES packets read out, scrambled as Apple's sample encryption of H.264
//! and AAC scrambles them (SAMPLE-AES), and written again as a segment (gaps row Y16).

use std::collections::HashMap;

use super::seal_at;

/// Emulation prevention put on: a `03` after two zeros before any byte up to 3.
pub fn escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 8);
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

/// One NAL unit (its emulation prevention in place) scrambled as Apple's sample encryption has
/// it: a coded slice over 48 bytes, 32 in the clear, then one block in ten, emulation prevention
/// put on again over the whole.
pub fn seal_nal(unit: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    let kind = unit[0] & 0x1f;
    if !(kind == 1 || kind == 5) || unit.len() <= 48 {
        return unit.to_vec();
    }
    let mut data = unit.to_vec();
    let mut starts = Vec::new();
    let mut at = 32;
    while at < data.len() {
        if data.len() - at > 16 {
            starts.push(at);
            at += 16;
        }
        at += (data.len() - at).min(144);
    }
    seal_at(key, iv, &mut data, &starts);
    escape(&data)
}

/// The NAL units of an Annex B stream, each with the start code before it.
pub fn units(data: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &start)| {
            let mut end = starts.get(n + 1).map_or(data.len(), |next| next - 3);
            while end > start && data[end - 1] == 0 {
                end -= 1;
            }
            (start, end)
        })
        .collect()
}

/// A PES's payload scrambled: each picture's slices, or each ADTS frame after its header and
/// 16 bytes, every whole block (each a chain of its own from `iv`).
pub fn seal_payload(payload: &[u8], picture: bool, key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    if picture {
        let mut out = Vec::new();
        for (start, end) in units(payload) {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend(seal_nal(&payload[start..end], key, iv));
        }
        return out;
    }
    let mut out = payload.to_vec();
    let mut at = 0;
    while at + 7 <= out.len() && out[at] == 0xff {
        let header = if out[at + 1] & 1 != 0 { 7 } else { 9 };
        let length = (usize::from(out[at + 3] & 3) << 11)
            | (usize::from(out[at + 4]) << 3)
            | (usize::from(out[at + 5]) >> 5);
        let body = at + header + 16;
        let whole = (at + length).saturating_sub(body) / 16;
        let starts: Vec<usize> = (0..whole).map(|b| body + b * 16).collect();
        seal_at(key, iv, &mut out[..at + length], &starts);
        at += length;
    }
    out
}

/// A transport stream's PES packets, in the order they start: each one's PID and bytes; and
/// the picture's and the sound's PIDs.
pub fn pes_of(ts: &[u8]) -> (Vec<(u16, Vec<u8>)>, u16, u16) {
    let (mut pmt, mut picture, mut sound) = (0u16, 0u16, 0u16);
    let mut gathering: HashMap<u16, usize> = HashMap::new();
    let mut out: Vec<(u16, Vec<u8>)> = Vec::new();
    for packet in ts.as_chunks::<188>().0 {
        let start = packet[1] & 0x40 != 0;
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let control = (packet[3] >> 4) & 3;
        let mut at = 4;
        if control & 2 != 0 {
            at += 1 + usize::from(packet[4]);
        }
        if control & 1 == 0 || at >= 188 {
            continue;
        }
        let payload = &packet[at..];
        if pid == 0 && start {
            let section = &payload[1 + usize::from(payload[0])..];
            pmt = (u16::from(section[10] & 0x1f) << 8) | u16::from(section[11]);
        } else if pid == pmt && start && pmt != 0 {
            let section = &payload[1 + usize::from(payload[0])..];
            let length = usize::from(u16::from_be_bytes([section[1], section[2]]) & 0x0fff);
            let info = usize::from(u16::from_be_bytes([section[10], section[11]]) & 0x0fff);
            let mut e = 12 + info;
            while e + 5 <= 3 + length - 4 {
                let es = (u16::from(section[e + 1] & 0x1f) << 8) | u16::from(section[e + 2]);
                match section[e] {
                    0x1b if picture == 0 => picture = es,
                    0x0f if sound == 0 => sound = es,
                    _ => {}
                }
                e += 5 + usize::from(u16::from_be_bytes([section[e + 3], section[e + 4]]) & 0x0fff);
            }
        } else if pid != 0 && (pid == picture || pid == sound) {
            if start {
                gathering.insert(pid, out.len());
                out.push((pid, payload.to_vec()));
            } else if let Some(&n) = gathering.get(&pid) {
                out[n].1.extend_from_slice(payload);
            }
        }
    }
    (out, picture, sound)
}

/// A PSI section in one packet of `pid`.
pub fn psi(pid: u16, section: &[u8]) -> Vec<u8> {
    let mut packet = vec![0x47, 0x40 | (pid >> 8) as u8, pid as u8, 0x10, 0];
    packet.extend_from_slice(section);
    packet.resize(188, 0xff);
    packet
}

/// A section: its table, its body after the five bytes every one has, and a CRC EDM does not
/// read (left nought).
pub fn section(table: u8, id: u16, body: &[u8]) -> Vec<u8> {
    let length = 5 + body.len() + 4;
    let mut out = vec![table, 0xb0 | (length >> 8) as u8, length as u8];
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0xc1, 0, 0]);
    out.extend_from_slice(body);
    out.extend_from_slice(&[0; 4]);
    out
}

/// A segment of a transport stream: its tables naming the picture and the sound as `kinds` (0xdb
/// and 0xcf for SAMPLE-AES), then each PES in packets.
pub fn segment_of(
    pes: &[(u16, Vec<u8>)],
    (picture, sound): (u16, u16),
    kinds: (u8, u8),
    counters: &mut HashMap<u16, u8>,
) -> Vec<u8> {
    const PMT: u16 = 0x1000;
    let mut out = psi(
        0,
        &section(0, 1, &[0, 1, 0xe0 | (PMT >> 8) as u8, PMT as u8]),
    );
    let mut pmt = vec![0xe0 | (picture >> 8) as u8, picture as u8, 0xf0, 0];
    for (kind, pid) in [(kinds.0, picture), (kinds.1, sound)] {
        pmt.extend_from_slice(&[kind, 0xe0 | (pid >> 8) as u8, pid as u8, 0xf0, 0]);
    }
    out.extend(psi(PMT, &section(2, 1, &pmt)));
    for (pid, bytes) in pes {
        let mut rest: &[u8] = bytes;
        let mut first = true;
        while !rest.is_empty() {
            let counter = counters.entry(*pid).or_insert(0);
            let take = rest.len().min(184);
            let mut packet = vec![
                0x47,
                (if first { 0x40 } else { 0 }) | (pid >> 8) as u8,
                *pid as u8,
            ];
            if take == 184 {
                packet.push(0x10 | *counter);
            } else {
                // Stuffed in the adaptation field to a whole packet.
                packet.push(0x30 | *counter);
                let field = 184 - take - 1;
                packet.push(field as u8);
                if field > 0 {
                    packet.push(0);
                    packet.extend(std::iter::repeat_n(0xff, field - 1));
                }
            }
            *counter = (*counter + 1) & 0x0f;
            packet.extend_from_slice(&rest[..take]);
            assert_eq!(packet.len(), 188);
            out.extend(packet);
            rest = &rest[take..];
            first = false;
        }
    }
    out
}

/// The vector of segment `n` of a playlist that names none: its sequence number.
pub fn iv_of(n: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&n.to_be_bytes());
    iv
}
