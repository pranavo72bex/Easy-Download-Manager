// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A WebM input read: its tracks, and its clusters' blocks, through a buffered reader that knows
//! where it is.

use super::*;

impl Reader {
    pub(super) fn read_at(&mut self, at: u64, len: usize) -> Result<Vec<u8>, Error> {
        let mut buf = vec![0u8; len];
        self.file
            .read_into(at, &mut buf)
            .map_err(|e| e.to_string())?;
        Ok(buf)
    }

    /// The element header at `at`: ID, body start, body end (the end of the
    /// file, or of `within`, for an unknown size).
    pub(super) fn header(
        &mut self,
        at: u64,
        within: u64,
    ) -> Result<(u32, u64, Option<u64>), Error> {
        let take = (within.min(self.length).saturating_sub(at)).min(12) as usize;
        let head = self.read_at(at, take)?;
        let (id, id_len) = read_id(&head).ok_or("a damaged element")?;
        let (size, size_len) =
            read_size(head.get(id_len..).unwrap_or_default()).ok_or("a damaged element")?;
        let start = at + (id_len + size_len) as u64;
        Ok((id, start, size.map(|s| start.saturating_add(s))))
    }
}

pub(super) fn read_input(path: &Path, input: usize) -> Result<Input, Error> {
    read_webm(path, input, false)
}

/// A WebM input: its tracks, and its blocks unless `tracks_only`, where
/// the reading stops at the first cluster.
pub(super) fn read_webm(path: &Path, input: usize, tracks_only: bool) -> Result<Input, Error> {
    let length = std::fs::metadata(path)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?
        .len();
    let mut r = Reader {
        file: Positioned::open(path)
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?,
        length,
    };
    let (id, _, end) = r.header(0, length)?;
    if id != EBML {
        return Err("it is not a WebM file".into());
    }
    let mut at = end.ok_or("it is not a WebM file")?;
    let mut segment = None;
    while at < length {
        let (id, start, end) = r.header(at, length)?;
        if id == SEGMENT {
            segment = Some((start, end.unwrap_or(length).min(length)));
            break;
        }
        at = end.ok_or("a damaged element")?;
    }
    let (mut at, segment_end) = segment.ok_or("it has no segment")?;
    let mut scale = SCALE_NS;
    let mut tracks = Vec::new();
    let mut blocks = Vec::new();
    while at + 2 <= segment_end {
        let (id, start, end) = r.header(at, segment_end)?;
        match id {
            INFO | TRACKS => {
                let end = end.ok_or("a damaged element")?.min(segment_end);
                let body = r.read_at(start, (end - start) as usize)?;
                if id == INFO {
                    for (child, value, _) in elements(&body) {
                        if child == TIMECODE_SCALE && uint(value) > 0 {
                            scale = uint(value);
                        }
                    }
                } else {
                    tracks = read_tracks(&body);
                }
                at = end;
            }
            CLUSTER if tracks_only => break,
            CLUSTER => {
                at = read_cluster(&mut r, input, start, end, segment_end, &mut blocks)?;
            }
            _ => match end {
                Some(end) => at = end,
                None => break,
            },
        }
    }
    if tracks.is_empty() {
        return Err("it has no tracks".into());
    }
    Ok(Input {
        scale,
        tracks,
        blocks,
        origin: None,
    })
}

pub(super) fn read_tracks(body: &[u8]) -> Vec<TrackIn> {
    elements(body)
        .filter(|(id, _, _)| *id == TRACK_ENTRY)
        .map(|(_, entry, _)| {
            let mut track = TrackIn {
                number: 0,
                kind: 0,
                codec: String::new(),
                rest: Vec::new(),
            };
            for (id, value, whole) in elements(entry) {
                match id {
                    TRACK_NUMBER => track.number = uint(value),
                    TRACK_UID => {}
                    _ => {
                        if id == TRACK_TYPE {
                            track.kind = uint(value);
                        }
                        if id == CODEC_ID {
                            track.codec = String::from_utf8_lossy(value).into_owned();
                        }
                        track.rest.extend_from_slice(whole);
                    }
                }
            }
            track
        })
        .filter(|t| t.number > 0)
        .collect()
}

/// A block's header: track number, its offset in ticks, flags, length.
pub(super) fn block_header(head: &[u8]) -> Option<(u64, i64, u8, usize)> {
    let (track, len) = read_size(head)?;
    let time = i16::from_be_bytes([*head.get(len)?, *head.get(len + 1)?]);
    let flags = *head.get(len + 2)?;
    Some((track?, i64::from(time), flags, len + 3))
}

fn read_cluster(
    r: &mut Reader,
    input: usize,
    start: u64,
    end: Option<u64>,
    segment_end: u64,
    blocks: &mut Vec<Block>,
) -> Result<u64, Error> {
    let limit = end.unwrap_or(segment_end).min(segment_end);
    let mut cluster_time = 0i64;
    let mut at = start;
    while at + 2 <= limit {
        let (id, body, body_end) = r.header(at, limit)?;
        if end.is_none() && TOP_LEVEL.contains(&id) {
            // A cluster of unknown size ends where the next one starts.
            return Ok(at);
        }
        let body_end = body_end.ok_or("a damaged block")?;
        if body_end > r.length {
            return Err("it is cut short".into());
        }
        match id {
            TIMECODE => {
                cluster_time = uint(&r.read_at(body, (body_end - body) as usize)?) as i64;
            }
            SIMPLE_BLOCK => {
                let head = r.read_at(body, ((body_end - body) as usize).min(12))?;
                let (track, offset, flags, header) =
                    block_header(&head).ok_or("a damaged block")?;
                blocks.push(Block {
                    input,
                    track,
                    // A timecode near `i64::MAX` from the file overflowed (gaps rows H10,
                    // H35): held at the end of the line, a block past it is as late as any.
                    ticks: cluster_time.saturating_add(offset),
                    decode: cluster_time.saturating_add(offset),
                    key: flags & 0x80 != 0,
                    flags,
                    data_at: body + header as u64,
                    data_len: (body_end - body).saturating_sub(header as u64),
                    group: None,
                });
            }
            BLOCK_GROUP => {
                let mut inner = body;
                let mut found = None;
                let mut extra = Vec::new();
                let mut referenced = false;
                while inner + 2 <= body_end {
                    let (child, child_body, child_end) = r.header(inner, body_end)?;
                    let child_end = child_end.ok_or("a damaged block")?.min(body_end);
                    if child == BLOCK {
                        let head =
                            r.read_at(child_body, ((child_end - child_body) as usize).min(12))?;
                        let (track, offset, flags, header) =
                            block_header(&head).ok_or("a damaged block")?;
                        found = Some((
                            track,
                            offset,
                            flags,
                            child_body + header as u64,
                            (child_end - child_body).saturating_sub(header as u64),
                        ));
                    } else {
                        if child == REFERENCE_BLOCK {
                            referenced = true;
                        }
                        extra.extend(r.read_at(inner, (child_end - inner) as usize)?);
                    }
                    inner = child_end;
                }
                if let Some((track, offset, flags, data_at, data_len)) = found {
                    blocks.push(Block {
                        input,
                        track,
                        ticks: cluster_time.saturating_add(offset),
                        decode: cluster_time.saturating_add(offset),
                        key: !referenced,
                        flags,
                        data_at,
                        data_len,
                        group: Some(extra),
                    });
                }
            }
            _ => {}
        }
        at = body_end;
    }
    Ok(limit)
}
