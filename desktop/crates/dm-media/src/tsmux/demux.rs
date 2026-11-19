// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Reading a transport stream: its packets, tables and timestamps.

use super::*;

pub(super) fn read_ts(
    input: &Path,
    scratch: &dyn Fn(&str) -> PathBuf,
) -> Result<Vec<Elementary>, String> {
    read_ts_locked(input, scratch, &crate::sealed::Locks::default())
}

/// The stream type a PMT names, as this reads it: a scrambled one (SAMPLE-AES) as what it holds,
/// and whether it is scrambled. A scrambled kind this does not clear, or one with no key to
/// clear it with, is refused.
fn kind_of(kind: u8, locks: &crate::sealed::Locks) -> Result<(u8, bool), String> {
    let (plain, sealed) = match kind {
        SEALED_H264 => (H264, true),
        SEALED_AAC => (AAC, true),
        SEALED_AC3 | SEALED_EAC3 => {
            return Err("its AC-3 sound is scrambled (SAMPLE-AES), which dm does not clear".into())
        }
        other => (other, false),
    };
    if sealed && !locks.any() {
        return Err("its pictures or sound are scrambled (SAMPLE-AES) and no key was given".into());
    }
    Ok((plain, sealed))
}

/// [`read_ts`], each PES of a scrambled stream (SAMPLE-AES) put in the clear with the lock of
/// the segment where it starts (gaps row Y16).
pub(super) fn read_ts_locked(
    input: &Path,
    scratch: &dyn Fn(&str) -> PathBuf,
    locks: &crate::sealed::Locks,
) -> Result<Vec<Elementary>, String> {
    let input = Joined::new(vec![input.to_path_buf()], None);
    let mut demux = Demux::new(input, Mode::Scratch(scratch), locks);
    loop {
        match demux.step() {
            Ok(true) => {}
            Ok(false) => return Ok(demux.into_streams()),
            Err(why) => {
                demux.drop_scratch();
                return Err(why);
            }
        }
    }
}

/// Where a stream's samples go as they are read out (`Elementary::sink`).
#[derive(Default)]
pub(super) enum Sink {
    /// Into its scratch file (`Elementary::out`).
    #[default]
    Scratch,
    /// Counted alone: what the index needs, each sample's place among the stream's samples.
    Counted,
    /// Kept in memory until taken (`Demux::take_kept`): one track made again as the MP4 writer
    /// reads it (`replay`).
    Kept(Vec<u8>),
}

/// What a [`Demux`] does with the samples it reads out.
#[derive(Clone, Copy)]
pub(super) enum Mode<'a> {
    /// Each stream's into a scratch file of its own, named by this.
    Scratch(&'a dyn Fn(&str) -> PathBuf),
    /// Only counted: the index of every sample, and nothing written.
    Counted,
    /// The picture's (`true`) or the sound's kept to be read; the other's counted.
    Kept(bool),
}

/// A transport stream read a packet at a time: the tables that say which packets are which
/// stream, and each stream's PES packets taken as they end.
pub(super) struct Demux<'a> {
    reader: BufReader<Joined>,
    mode: Mode<'a>,
    locks: crate::sealed::Locks,
    packet: [u8; PACKET],
    pmt_pids: Vec<u16>,
    /// PID → stream, in the order the tables named them.
    streams: Vec<(u16, Elementary)>,
    lost_sync: u32,
    /// Bytes read so far: where the packet in hand starts is this, less its length.
    consumed: u64,
    /// Read to its end, and every stream's last PES taken.
    ended: bool,
}

impl<'a> Demux<'a> {
    /// A stream about to be read from `input`.
    pub(super) fn new(input: Joined, mode: Mode<'a>, locks: &crate::sealed::Locks) -> Self {
        Demux {
            reader: BufReader::with_capacity(1 << 20, input),
            mode,
            locks: locks.clone(),
            packet: [0u8; PACKET],
            pmt_pids: Vec::new(),
            streams: Vec::new(),
            lost_sync: 0,
            consumed: 0,
            ended: false,
        }
    }

    /// The whole stream read: its picture and sound.
    pub(super) fn finish(mut self) -> Result<Vec<Elementary>, String> {
        while self.step()? {}
        Ok(self.into_streams())
    }

    /// The streams read so far.
    pub(super) fn into_streams(self) -> Vec<Elementary> {
        self.streams.into_iter().map(|(_, stream)| stream).collect()
    }

    /// The scratch files made so far, removed: what a stream refused leaves behind.
    pub(super) fn drop_scratch(&self) {
        for (_, stream) in &self.streams {
            if !stream.path.as_os_str().is_empty() {
                let _ = std::fs::remove_file(&stream.path);
            }
        }
    }

    /// The samples the kept stream has made since this was last asked, in `into` (emptied
    /// first). Every stream's index is let go of but its last sample, the one a picture's rest
    /// is added to (`append_to_last`): the writer reads by the first pass's index, not this.
    pub(super) fn take_kept(&mut self, into: &mut Vec<u8>) {
        into.clear();
        for (_, stream) in &mut self.streams {
            if let Sink::Kept(kept) = &mut stream.sink {
                std::mem::swap(kept, into);
            }
            let before_last = stream.samples.len().saturating_sub(1);
            stream.samples.drain(..before_last);
        }
    }

    /// Reads the next packet: `false` once the stream is read to its end and each stream's last
    /// PES taken.
    pub(super) fn step(&mut self) -> Result<bool, String> {
        if self.ended {
            return Ok(false);
        }
        match self.reader.read_exact(&mut self.packet) {
            Ok(()) => self.consumed += PACKET as u64,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.end()?;
                return Ok(false);
            }
            Err(e) => return Err(format!("the stream could not be read: {e}")),
        }
        if self.packet[0] != 0x47 && !self.resync()? {
            return Ok(true);
        }
        self.take_packet()?;
        Ok(true)
    }

    /// Out of step: finds the next sync byte and carries on from it. `false` where the packet
    /// in hand is still not one.
    fn resync(&mut self) -> Result<bool, String> {
        self.lost_sync += 1;
        if self.lost_sync > 1000 {
            return Err("it is not a transport stream".into());
        }
        let Some(at) = self.packet.iter().skip(1).position(|b| *b == 0x47) else {
            return Ok(false);
        };
        let mut rest = vec![0u8; at + 1];
        self.reader
            .read_exact(&mut rest)
            .map_err(|_| "the stream ends mid-packet".to_string())?;
        self.consumed += rest.len() as u64;
        let mut shifted = self.packet.get(at + 1..).unwrap_or_default().to_vec();
        shifted.extend_from_slice(&rest);
        self.packet.copy_from_slice(&shifted);
        Ok(self.packet[0] == 0x47)
    }

    /// The packet in hand: a table, or part of a stream's PES.
    fn take_packet(&mut self) -> Result<(), String> {
        let packet = self.packet;
        let start = packet[1] & 0x40 != 0;
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let control = (packet[3] >> 4) & 3;
        let mut at = 4usize;
        if control & 2 != 0 {
            at += 1 + usize::from(packet[4]);
        }
        if control & 1 == 0 {
            return Ok(());
        }
        let Some(payload) = packet.get(at..).filter(|p| !p.is_empty()) else {
            return Ok(());
        };
        if pid == 0 && start {
            self.pmt_pids = read_pat(payload);
        } else if self.pmt_pids.contains(&pid) && start {
            self.take_pmt(payload)?;
        } else if let Some((_, stream)) = self.streams.iter_mut().find(|(p, _)| *p == pid) {
            if start && !stream.pes.is_empty() {
                let pes = std::mem::take(&mut stream.pes);
                stream.lock = stream.gathering;
                take_pes(stream, &pes)?;
            }
            if start && stream.sealed {
                stream.gathering = self.locks.at(self.consumed.saturating_sub(PACKET as u64));
            }
            if start || !stream.pes.is_empty() {
                stream.pes.extend_from_slice(payload);
            }
        }
        Ok(())
    }

    /// A program's table: the first picture and the first sound it names are read out.
    fn take_pmt(&mut self, payload: &[u8]) -> Result<(), String> {
        for (kind, es_pid) in read_pmt(payload) {
            let (kind, sealed) = match kind_of(kind, &self.locks) {
                Ok(read) => read,
                Err(why) => {
                    self.drop_scratch();
                    return Err(why);
                }
            };
            if OTHERS.contains(&kind) {
                self.drop_scratch();
                return Err(format!(
                    "it carries a kind of picture or sound (stream type {kind:#04x}) dm does not \
                     put into an MP4"
                ));
            }
            let picture = PICTURES.contains(&kind);
            if !self.streams.iter().any(|(p, _)| *p == es_pid)
                && (picture || SOUNDS.contains(&kind))
            {
                let has_kind = self
                    .streams
                    .iter()
                    .any(|(_, s)| PICTURES.contains(&s.kind) == picture);
                // One picture and one sound: the first of each.
                if !has_kind {
                    let stream = self.open(kind, sealed, picture)?;
                    self.streams.push((es_pid, stream));
                }
            }
        }
        Ok(())
    }

    /// A stream newly named, its samples going where the mode says.
    fn open(&self, kind: u8, sealed: bool, picture: bool) -> Result<Elementary, String> {
        let mut stream = Elementary {
            kind,
            sealed,
            ..Elementary::default()
        };
        match self.mode {
            Mode::Scratch(scratch) => {
                let path = scratch(if picture { "video" } else { "audio" });
                let out = File::create(&path)
                    .map_err(|e| format!("a scratch file could not be made: {e}"))?;
                stream.out = Some(BufWriter::with_capacity(1 << 20, out));
                stream.path = path;
            }
            Mode::Counted => stream.sink = Sink::Counted,
            Mode::Kept(wanted) => {
                stream.sink = if wanted == picture {
                    Sink::Kept(Vec::new())
                } else {
                    Sink::Counted
                };
            }
        }
        Ok(stream)
    }

    /// The end of the stream: each stream's last PES taken, its scratch file written out.
    fn end(&mut self) -> Result<(), String> {
        self.ended = true;
        for (_, stream) in &mut self.streams {
            if !stream.pes.is_empty() {
                let pes = std::mem::take(&mut stream.pes);
                stream.lock = stream.gathering;
                take_pes(stream, &pes)?;
            }
            if let Some(mut writer) = stream.out.take() {
                writer
                    .flush()
                    .map_err(|e| format!("a scratch file could not be written: {e}"))?;
            }
        }
        Ok(())
    }
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
    let raw_pts = pts.map(|p| unwrap(p, stream.raw_last));
    let raw_dts = dts.map(|d| unwrap(d, raw_pts.or(stream.raw_last)));
    // A clock started afresh (a discontinuity): what follows is moved on to go on a frame after
    // the last, where it would have gone back in time or leapt ahead.
    if let (Some(p), Some(last)) = (raw_pts, stream.last_at) {
        let at = raw_dts.unwrap_or(p) + stream.shift;
        if at - last > JUMP || last - at > JUMP_BACK {
            stream.shift += last + STEP - at;
        }
    }
    let pts = raw_pts.map(|p| p + stream.shift);
    let dts = raw_dts.map(|d| d + stream.shift);
    if let Some(p) = raw_pts {
        stream.raw_last = Some(p);
    }
    if let Some(p) = pts {
        stream.last_pts = Some(p);
        stream.last_at = Some(dts.unwrap_or(p));
    }
    if PICTURES.contains(&stream.kind) {
        take_picture(stream, data, pts, dts)
    } else {
        take_sound(stream, data, pts)
    }
}
