// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A part of a video, cut from the streams dm fetches itself.
//!
//! A fragmented MP4 as a DASH stream is served (`ftyp` and `moov`, then
//! `sidx`, then pieces of `moof` and `mdat` a few seconds each) carries an
//! index saying where every piece is in the file and when it plays. A
//! part from one second to another is the pieces that overlap it: the
//! stream's first boxes (its "init"), then those pieces' bytes, asked for
//! by ranges and written as one shorter fragmented file, which the muxer
//! joins as it joins the whole stream and cuts to the second with an edit
//! list (`mp4mux::join_cut`). Nothing is decoded or encoded, so the part is
//! the site's own picture and sound, byte for byte, and arrives in the
//! time its own bytes take.
//!
//! The index is where the site's reading names it (`youtube.rs` hands on
//! YouTube's `initRange` and `indexRange`), else looked for in the stream's
//! first bytes. A stream with no index (WebM, a plain MP4 with one `moov`,
//! a stream in pieces) cannot be cut this way, and says so.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures_util::StreamExt;

use crate::download::{Headers, Options, Update};
use crate::progress::Meter;
use crate::Error;

/// A part of a video: from one second to another.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Span {
    /// Where the part begins, in seconds of the video.
    pub from: f64,
    /// Where the part ends, in seconds of the video.
    pub to: f64,
}

impl Span {
    /// The span from `from` to `to` seconds, where that is one: `to` after
    /// `from`, neither negative, both finite.
    pub fn new(from: f64, to: f64) -> Option<Span> {
        (from.is_finite() && to.is_finite() && from >= 0.0 && to > from)
            .then_some(Span { from, to })
    }

    /// Both ends in a person's words (`seconds`).
    pub fn parse(from: &str, to: &str) -> Option<Span> {
        Span::new(Span::seconds(from)?, Span::seconds(to)?)
    }

    /// Seconds from a person's words: "90", "1:30", "1:02:03", "1m30s",
    /// "90s", "2h", "1h2m3s", with a fraction allowed in the seconds
    /// ("4.5", "1:30.5"). `None` for anything else.
    pub fn seconds(text: &str) -> Option<f64> {
        let text = text.trim().to_ascii_lowercase();
        if text.is_empty() {
            return None;
        }
        if text.contains(':') {
            let parts: Vec<&str> = text.split(':').collect();
            if parts.len() > 3 {
                return None;
            }
            let mut total = 0.0;
            for part in parts {
                let value: f64 = part.trim().parse().ok()?;
                if !value.is_finite() || value < 0.0 {
                    return None;
                }
                total = total * 60.0 + value;
            }
            return Some(total);
        }
        if text.chars().any(|c| matches!(c, 'h' | 'm' | 's')) {
            let mut total = 0.0;
            let mut number = String::new();
            let mut any = false;
            for c in text.chars() {
                match c {
                    '0'..='9' | '.' => number.push(c),
                    'h' | 'm' | 's' => {
                        let value: f64 = number.parse().ok()?;
                        number.clear();
                        total += value
                            * match c {
                                'h' => 3600.0,
                                'm' => 60.0,
                                _ => 1.0,
                            };
                        any = true;
                    }
                    ' ' => {}
                    _ => return None,
                }
            }
            if !number.is_empty() {
                total += number.parse::<f64>().ok()?;
            }
            return (any && total.is_finite()).then_some(total);
        }
        text.parse::<f64>()
            .ok()
            .filter(|s| s.is_finite() && *s >= 0.0)
    }

    /// Seconds as a clock reads them: "1:30", "1:02:03".
    pub fn clock(seconds: f64) -> String {
        let whole = seconds.max(0.0).round() as u64;
        let (h, m, s) = (whole / 3600, whole % 3600 / 60, whole % 60);
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    }

    /// The span as a file name can carry it: "1m30s-2m05s", "1h02m03s-…".
    pub fn words(&self) -> String {
        let stamp = |seconds: f64| {
            let whole = seconds.max(0.0).round() as u64;
            let (h, m, s) = (whole / 3600, whole % 3600 / 60, whole % 60);
            if h > 0 {
                format!("{h}h{m:02}m{s:02}s")
            } else {
                format!("{m}m{s:02}s")
            }
        };
        format!("{}-{}", stamp(self.from), stamp(self.to))
    }

    /// The span in milliseconds, for the muxer's cut.
    pub fn millis(&self) -> (u64, u64) {
        (
            (self.from * 1000.0).round() as u64,
            (self.to * 1000.0).round() as u64,
        )
    }
}

/// One piece a fragmented file's index names: where it is in the file,
/// and when it plays, in the index's timescale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// Where the piece starts in the file, in bytes.
    pub offset: u64,
    /// How many bytes the piece takes.
    pub size: u64,
    /// When the piece starts playing, in the index's timescale.
    pub start: u64,
    /// How long the piece plays, in the index's timescale.
    pub duration: u64,
}

/// A fragmented file's index (`sidx`): its pieces, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    /// How many ticks make a second in this file.
    pub timescale: u32,
    /// The pieces the index names, in order.
    pub pieces: Vec<Piece>,
}

/// The index in a `sidx` box (`whole`, its header included) that sits at
/// `at` in its file: the pieces it names, each with where it begins.
/// `None` for anything that is not one, or one that names further
/// indexes rather than pieces.
pub fn parse_sidx(whole: &[u8], at: u64) -> Option<Index> {
    let size = u64::from(be32(whole, 0)?);
    if whole.get(4..8)? != b"sidx" || size < 32 || whole.len() < size as usize {
        return None;
    }
    let body = whole.get(8..size as usize)?;
    let version = *body.first()?;
    let timescale = be32(body, 8)?;
    let (earliest, first_offset, mut pos) = if version == 0 {
        (u64::from(be32(body, 12)?), u64::from(be32(body, 16)?), 20)
    } else {
        (be64(body, 12)?, be64(body, 20)?, 28)
    };
    let count = usize::from(be16(body, pos + 2)?);
    pos += 4;
    let mut offset = at.checked_add(size)?.checked_add(first_offset)?;
    let mut start = earliest;
    let mut pieces = Vec::with_capacity(count);
    for _ in 0..count {
        let sized = be32(body, pos)?;
        let duration = u64::from(be32(body, pos + 4)?);
        pos += 12;
        if sized >> 31 != 0 {
            // A reference to another index, which these streams do not use.
            return None;
        }
        let size = u64::from(sized & 0x7fff_ffff);
        pieces.push(Piece {
            offset,
            size,
            start,
            duration,
        });
        offset = offset.checked_add(size)?;
        start = start.checked_add(duration)?;
    }
    (timescale > 0 && !pieces.is_empty()).then_some(Index { timescale, pieces })
}

/// Where a fragmented file's first boxes end and its index is, found in
/// its first bytes (`head`): the length of everything before the index
/// (`ftyp`, `moov` and whatever sits between), and the index box's first
/// and last byte. `None` where `head` holds no whole index.
pub fn locate(head: &[u8]) -> Option<(u64, (u64, u64))> {
    let mut at = 0usize;
    let mut init_end = None;
    while at + 8 <= head.len() {
        let size = match be32(head, at)? {
            0 => head.len().checked_sub(at)?,
            1 => usize::try_from(be64(head, at + 8)?).ok()?,
            n => n as usize,
        };
        if size < 8 {
            return None;
        }
        let kind = head.get(at + 4..at + 8)?;
        match kind {
            b"moov" => init_end = Some(at.checked_add(size)?),
            b"sidx" => {
                let end = at.checked_add(size)?;
                if end > head.len() {
                    return None;
                }
                let init = init_end?;
                return Some((init as u64, (at as u64, end as u64 - 1)));
            }
            b"moof" | b"mdat" => return None,
            _ => {}
        }
        at = at.checked_add(size)?;
    }
    None
}

/// The pieces covering `span`, first and last, by their place in the
/// index. `None` where the span lies past what the index holds.
pub fn covering(index: &Index, span: Span) -> Option<(usize, usize)> {
    let scale = f64::from(index.timescale);
    let from = (span.from * scale).round() as u64;
    let to = (span.to * scale).round() as u64;
    let mut first = None;
    let mut last = None;
    for (i, piece) in index.pieces.iter().enumerate() {
        let end = piece.start.saturating_add(piece.duration);
        // A piece with no bytes or no time is nothing to fetch.
        if piece.size == 0 || piece.duration == 0 {
            continue;
        }
        if piece.start < to && end > from {
            first.get_or_insert(i);
            last = Some(i);
        }
    }
    Some((first?, last?))
}

/// One stream to cut a part from, as the reading describes it.
#[derive(Debug, Clone)]
pub(crate) struct Source {
    pub url: String,
    /// Where the stream's first boxes and its index are, where the site said.
    pub init: Option<(u64, u64)>,
    pub index: Option<(u64, u64)>,
    pub agent: Option<String>,
    pub headers: Headers,
    /// The most one request may ask for.
    pub chunk: Option<u64>,
    /// Where the part is written.
    pub target: PathBuf,
}

/// What a part came to: its bytes, and when the pieces taken start and
/// end, in seconds of the video (a piece is taken whole).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Part {
    pub bytes: u64,
    pub starts: f64,
    pub ends: f64,
}

/// How much of a stream with no stated index is read to look for one.
const PROBE: u64 = 256 << 10;

fn no_index() -> Error {
    Error::Internal(
        "this stream carries no index of its pieces, so a part of it cannot be cut: only a \
         video whose streams do (YouTube's MP4) can"
            .into(),
    )
}

/// The part of `source` that plays within `span`, written whole to its
/// target: the stream's first boxes, then every piece that overlaps the
/// span, fetched a slice at a time on `connections` at once.
pub(crate) async fn fetch_part(
    client: &reqwest::Client,
    source: &Source,
    span: Span,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Part, Error> {
    use crate::download::or_stop;
    let control = &options.control;
    let (init, index) = match (source.init, source.index) {
        (Some(init), Some(index)) => {
            let first = or_stop(control, get_range(client, source, init.0, init.1)).await?;
            let listed = or_stop(control, get_range(client, source, index.0, index.1)).await?;
            (first, parse_sidx(&listed, index.0).ok_or_else(no_index)?)
        }
        _ => {
            let head = or_stop(control, get_range(client, source, 0, PROBE - 1)).await?;
            let (init_end, (index_at, index_end)) = locate(&head).ok_or_else(no_index)?;
            let index = parse_sidx(&head[index_at as usize..=index_end as usize], index_at)
                .ok_or_else(no_index)?;
            (head.slice(0..init_end as usize), index)
        }
    };
    let (first, last) = covering(&index, span).ok_or_else(|| {
        Error::Internal(format!(
            "the video has nothing between {} and {}",
            Span::clock(span.from),
            Span::clock(span.to)
        ))
    })?;
    let pieces = &index.pieces[first..=last];
    let odd = || Error::Internal("this stream's index does not add up".into());
    let begin = pieces[0].offset;
    let last_piece = pieces[pieces.len() - 1];
    let end = last_piece
        .offset
        .checked_add(last_piece.size)
        .and_then(|e| e.checked_sub(1))
        .filter(|e| *e >= begin)
        .ok_or_else(odd)?;
    let total = (init.len() as u64)
        .checked_add(end - begin + 1)
        .ok_or_else(odd)?;
    let scale = f64::from(index.timescale);
    let part = Part {
        bytes: total,
        starts: pieces[0].start as f64 / scale,
        ends: last_piece.start.saturating_add(last_piece.duration) as f64 / scale,
    };

    // A part already fetched whole (a resume after a pause) is not fetched
    // again; one fetched in part is, from the start: a part is small.
    if tokio::fs::metadata(&source.target)
        .await
        .is_ok_and(|m| m.len() == total)
    {
        return Ok(part);
    }
    if let Some(dir) = source.target.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(Error::Disk)?;
    }
    // Written beside its name and moved there whole, so a part cut short
    // never passes for one that is not.
    let mut writing = source.target.as_os_str().to_owned();
    writing.push(".part");
    let writing = PathBuf::from(writing);
    let mut file = tokio::fs::File::create(&writing)
        .await
        .map_err(Error::Disk)?;
    use tokio::io::AsyncWriteExt;
    file.write_all(&init).await.map_err(Error::Disk)?;

    // Slices no larger than the server's cap on one request (YouTube slows
    // a response past about ten megabytes) or four megabytes, fetched
    // several at once and written in order; each slice asked again on a
    // passing failure, as the engine asks a range again.
    let slice = source.chunk.unwrap_or(10 << 20).clamp(64 << 10, 4 << 20);
    let slices = (end - begin) / slice + 1;
    let tries = if options.attempts == 0 {
        10
    } else {
        options.attempts
    };
    let at_once = options.connections.clamp(1, 16);
    let mut fetched = futures_util::stream::iter(0..slices)
        .map(|n| {
            let a = begin + n * slice;
            let b = (a + slice - 1).min(end);
            get_range_again(client, source, a, b, tries, &options.control)
        })
        .buffered(at_once);
    let mut meter = Meter::new(Some(total), 0);
    let mut downloaded = init.len() as u64;
    let mut last_said = Instant::now() - Duration::from_secs(1);
    let mut last_sample = Instant::now();
    loop {
        let next = or_stop(control, async { Ok::<_, Error>(fetched.next().await) }).await?;
        let Some(bytes) = next else {
            break;
        };
        let bytes = bytes?;
        options.limiter.take(bytes.len() as u64).await;
        file.write_all(&bytes).await.map_err(Error::Disk)?;
        downloaded += bytes.len() as u64;
        if let Some(tx) = &report {
            if last_said.elapsed() >= Duration::from_millis(250) || downloaded == total {
                let progress = meter.sample(downloaded, last_sample.elapsed());
                last_sample = Instant::now();
                last_said = Instant::now();
                let _ = tx.send(Update::Progress(progress)).await;
            }
        }
    }
    file.flush().await.map_err(Error::Disk)?;
    file.sync_all().await.map_err(Error::Disk)?;
    drop(file);
    if downloaded != total {
        let _ = tokio::fs::remove_file(&writing).await;
        return Err(Error::Incomplete {
            got: downloaded,
            expected: total,
        });
    }
    tokio::fs::rename(&writing, &source.target)
        .await
        .map_err(Error::Disk)?;
    Ok(part)
}

/// [`get_range`], asked again after a passing failure (a dropped
/// connection, a 503, a timeout), with the engine's own backoff, up to
/// `tries` times; a pause or cancel asked for meanwhile is answered at
/// once.
async fn get_range_again(
    client: &reqwest::Client,
    source: &Source,
    from: u64,
    to: u64,
    tries: u32,
    control: &crate::Control,
) -> Result<bytes::Bytes, Error> {
    let mut attempt = 0u32;
    loop {
        control.check()?;
        match get_range(client, source, from, to).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                attempt += 1;
                if attempt >= tries || !crate::retry::transient(&e) {
                    return Err(e);
                }
                let wait = crate::retry::backoff(attempt, None);
                crate::download::or_stop(control, async {
                    tokio::time::sleep(wait).await;
                    Ok::<(), Error>(())
                })
                .await?;
            }
        }
    }
}

/// Bytes `from` to `to` of the stream, as one request.
async fn get_range(
    client: &reqwest::Client,
    source: &Source,
    from: u64,
    to: u64,
) -> Result<bytes::Bytes, Error> {
    let mut request = client
        .get(&source.url)
        .header(reqwest::header::RANGE, format!("bytes={from}-{to}"));
    if let Some(agent) = &source.agent {
        request = request.header(reqwest::header::USER_AGENT, agent);
    }
    request = source.headers.onto(request, false, false);
    let response = crate::redirects::send(request).await?;
    let mut response = Error::refused_or(response).await?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(Error::RangesIgnored);
    }
    // The answer must be the range asked for, said so before any of it is
    // read: a server answering from another offset, or with more than was
    // asked, is not read into memory and then found out. One that stops at
    // the file's last byte, short of the end asked for, is the file's end:
    // the 256 KiB looked at first, of a file smaller than that, is all of it
    // (coverage tests, 2026-11-08).
    let said = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes "))
        .and_then(|v| v.split_once('/'))
        .and_then(|(span, total)| {
            let (a, b) = span.split_once('-')?;
            Some((
                a.trim().parse::<u64>().ok()?,
                b.trim().parse::<u64>().ok()?,
                total.trim().parse::<u64>().ok(),
            ))
        });
    let to = match said {
        Some((a, b, _)) if a == from && b == to => to,
        Some((a, b, Some(total))) if a == from && b < to && b >= a && b + 1 == total => b,
        _ => return Err(Error::DishonestRanges),
    };
    let expected = to - from + 1;
    if response.content_length().is_some_and(|len| len != expected) {
        return Err(Error::DishonestRanges);
    }
    let mut bytes = Vec::with_capacity(expected.min(4 << 20) as usize);
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() as u64 + chunk.len() as u64 > expected {
            return Err(Error::DishonestRanges);
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() as u64 != expected {
        return Err(Error::Incomplete {
            got: bytes.len() as u64,
            expected,
        });
    }
    Ok(bytes::Bytes::from(bytes))
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn be64(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `sidx` box naming `pieces` of (size, duration), version 0.
    pub(crate) fn sidx(timescale: u32, first_offset: u32, pieces: &[(u32, u32)]) -> Vec<u8> {
        let mut body = vec![0u8, 0, 0, 0]; // version 0, flags
        body.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
        body.extend_from_slice(&timescale.to_be_bytes());
        body.extend_from_slice(&0u32.to_be_bytes()); // earliest_presentation_time
        body.extend_from_slice(&first_offset.to_be_bytes());
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend_from_slice(&(pieces.len() as u16).to_be_bytes());
        for (size, duration) in pieces {
            body.extend_from_slice(&size.to_be_bytes());
            body.extend_from_slice(&duration.to_be_bytes());
            body.extend_from_slice(&0x9000_0000u32.to_be_bytes());
        }
        let mut whole = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        whole.extend_from_slice(b"sidx");
        whole.extend_from_slice(&body);
        whole
    }

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut whole = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        whole.extend_from_slice(kind);
        whole.extend_from_slice(body);
        whole
    }

    #[test]
    fn seconds_are_read_from_every_way_a_person_writes_them() {
        assert_eq!(Span::seconds("90"), Some(90.0));
        assert_eq!(Span::seconds("1:30"), Some(90.0));
        assert_eq!(Span::seconds("1:02:03"), Some(3723.0));
        assert_eq!(Span::seconds("1m30s"), Some(90.0));
        assert_eq!(Span::seconds("90s"), Some(90.0));
        assert_eq!(Span::seconds("2h"), Some(7200.0));
        assert_eq!(Span::seconds("1h2m3s"), Some(3723.0));
        assert_eq!(Span::seconds(" 4.5 "), Some(4.5));
        assert_eq!(Span::seconds("1:30.5"), Some(90.5));
        assert_eq!(Span::seconds(""), None);
        assert_eq!(Span::seconds("-5"), None);
        assert_eq!(Span::seconds("1:2:3:4"), None);
        assert_eq!(Span::seconds("soon"), None);
        assert_eq!(Span::parse("1:00", "0:30"), None, "an end before its start");
        assert_eq!(
            Span::parse("0:30", "1:00"),
            Some(Span {
                from: 30.0,
                to: 60.0
            })
        );
        assert_eq!(
            Span {
                from: 250.0,
                to: 285.0
            }
            .words(),
            "4m10s-4m45s"
        );
        assert_eq!(
            Span {
                from: 3723.0,
                to: 3800.0
            }
            .words(),
            "1h02m03s-1h03m20s"
        );
        assert_eq!(Span::clock(250.0), "4:10");
        assert_eq!(Span::clock(3723.4), "1:02:03");
        assert_eq!(
            Span {
                from: 1.26,
                to: 2.0
            }
            .millis(),
            (1260, 2000)
        );
    }

    #[test]
    fn an_index_names_each_piece_with_where_it_is_and_when_it_plays() {
        // The index sits at 700 in its file and is 44 bytes long; its first
        // piece begins 10 bytes after it.
        let whole = sidx(1000, 10, &[(5000, 5000), (6000, 5000), (1000, 2500)]);
        assert_eq!(whole.len(), 32 + 36);
        let index = parse_sidx(&whole, 700).unwrap();
        assert_eq!(index.timescale, 1000);
        assert_eq!(
            index.pieces,
            [
                Piece {
                    offset: 778,
                    size: 5000,
                    start: 0,
                    duration: 5000
                },
                Piece {
                    offset: 5778,
                    size: 6000,
                    start: 5000,
                    duration: 5000
                },
                Piece {
                    offset: 11778,
                    size: 1000,
                    start: 10000,
                    duration: 2500
                },
            ]
        );
        assert_eq!(covering(&index, Span::new(0.0, 1.0).unwrap()), Some((0, 0)));
        assert_eq!(covering(&index, Span::new(4.0, 6.0).unwrap()), Some((0, 1)));
        assert_eq!(
            covering(&index, Span::new(5.0, 10.0).unwrap()),
            Some((1, 1))
        );
        assert_eq!(
            covering(&index, Span::new(7.0, 60.0).unwrap()),
            Some((1, 2))
        );
        assert_eq!(
            covering(&index, Span::new(12.5, 60.0).unwrap()),
            None,
            "past the end"
        );
        // A piece with no bytes, or no time, is never taken.
        let odd = parse_sidx(&sidx(1000, 0, &[(0, 5000), (5000, 0), (6000, 5000)]), 0).unwrap();
        assert_eq!(covering(&odd, Span::new(0.0, 60.0).unwrap()), Some((2, 2)));
        // Not an index: another box, one cut short, one naming indexes.
        assert_eq!(parse_sidx(&boxed(b"moov", &[0; 40]), 0), None);
        assert_eq!(parse_sidx(&whole[..whole.len() - 1], 0), None);
        let mut nested = sidx(1000, 0, &[(5000, 5000)]);
        nested[32] |= 0x80;
        assert_eq!(parse_sidx(&nested, 0), None);
    }

    #[test]
    fn the_index_is_found_in_a_streams_first_bytes() {
        let ftyp = boxed(b"ftyp", b"isom\0\0\0\0isomiso2");
        let moov = boxed(b"moov", &[1; 100]);
        let index = sidx(1000, 0, &[(50, 1000)]);
        let moof = boxed(b"moof", &[2; 20]);
        let head = [ftyp.clone(), moov.clone(), index.clone(), moof].concat();
        let init_end = (ftyp.len() + moov.len()) as u64;
        assert_eq!(
            locate(&head),
            Some((init_end, (init_end, init_end + index.len() as u64 - 1)))
        );
        // Cut before the index is whole, or with no index at all.
        assert_eq!(locate(&head[..head.len() - 30]), None);
        assert_eq!(locate(&[ftyp.clone(), moov.clone()].concat()), None);
        let plain = [ftyp, moov, boxed(b"mdat", &[0; 10])].concat();
        assert_eq!(locate(&plain), None);
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::tests::sidx;
    use super::*;
    use crate::{Categories, Control, Limiter, Quality};
    use axum::body::Body;
    use axum::extract::Path as UrlPath;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::Response;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// Four pieces of a second each, 100 000 bytes apiece.
    const PIECE: usize = 100_000;
    const PIECES: usize = 4;

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut whole = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        whole.extend_from_slice(kind);
        whole.extend_from_slice(body);
        whole
    }

    /// A stream laid out as a DASH MP4 is: `ftyp` and `moov`, its index,
    /// then its pieces, whose bytes differ from place to place so a slice
    /// written in the wrong order or from the wrong offset shows. Returns
    /// the stream, where its init ends, and where its index ends.
    fn stream() -> (Vec<u8>, usize, usize) {
        let init = [
            boxed(b"ftyp", b"iso6\0\0\0\0iso6dash"),
            boxed(b"moov", &[7; 200]),
        ]
        .concat();
        let index = sidx(1000, 0, &[(PIECE as u32, 1000); PIECES]);
        let mut data = [init.clone(), index.clone()].concat();
        let pieces_at = data.len();
        data.extend((0..PIECE * PIECES).map(|i| ((i * 7 + i / 977) % 251) as u8));
        assert_eq!(data.len(), pieces_at + PIECE * PIECES);
        (data, init.len(), pieces_at)
    }

    /// What each request carried: its range, user agent and referrer.
    #[derive(Debug, Clone, PartialEq)]
    struct Asked {
        /// The case the request was made in (the path's first part).
        how: String,
        range: String,
        agent: String,
        referer: String,
    }

    #[derive(Default)]
    struct Served {
        asked: Mutex<Vec<Asked>>,
        /// How often each range was asked, for a server that fails a
        /// range the first time.
        times: Mutex<HashMap<String, u32>>,
    }

    /// Serves `data` at `/{how}/s.mp4`, where `how` says how the server
    /// answers a range from `from` on (the stream's own first bytes are
    /// always served honestly): `honest`, `ignores` (the whole file),
    /// `shifted` (another range than asked), `longer` (a byte more than
    /// stated, the length said), `streams-more` and `streams-less` (a
    /// body of no stated length with a byte more or less), `flaky` (a 503
    /// the first time each range is asked), `gone` (404).
    async fn serve(data: Vec<u8>, from: usize) -> (String, Arc<Served>) {
        let data = Arc::new(data);
        let served = Arc::new(Served::default());
        let state = served.clone();
        let app = axum::Router::new().route(
            "/{how}/s.mp4",
            axum::routing::get(move |UrlPath(how): UrlPath<String>, headers: HeaderMap| {
                let (data, state) = (data.clone(), state.clone());
                async move {
                    let header = |name: &str| {
                        headers
                            .get(name)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .to_string()
                    };
                    let range = header("range");
                    state.asked.lock().unwrap().push(Asked {
                        how: how.clone(),
                        range: range.clone(),
                        agent: header("user-agent"),
                        referer: header("referer"),
                    });
                    let times = {
                        let mut times = state.times.lock().unwrap();
                        let n = times.entry(range.clone()).or_default();
                        *n += 1;
                        *n
                    };
                    let n = data.len();
                    let (a, z) = range
                        .strip_prefix("bytes=")
                        .and_then(|r| r.split_once('-'))
                        .and_then(|(a, z)| {
                            Some((a.parse::<usize>().ok()?, z.parse::<usize>().ok()?))
                        })
                        .map(|(a, z)| (a, z.min(n - 1)))
                        .unwrap();
                    let how = if a < from { "honest" } else { how.as_str() };
                    let partial = |a: usize, z: usize, body: Body| {
                        Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-range", format!("bytes {a}-{z}/{n}"))
                            .body(body)
                            .unwrap()
                    };
                    let streamed = |bytes: Vec<u8>| {
                        Body::from_stream(futures_util::stream::iter([Ok::<_, std::io::Error>(
                            bytes::Bytes::from(bytes),
                        )]))
                    };
                    match how {
                        "ignores" => Response::new(Body::from(data.to_vec())),
                        "shifted" => partial(a + 1, z, Body::from(data[a + 1..=z].to_vec())),
                        "longer" => partial(a, z, Body::from(data[a..=z + 1].to_vec())),
                        "streams-more" => partial(a, z, streamed(data[a..=z + 1].to_vec())),
                        "streams-less" => partial(a, z, streamed(data[a..z].to_vec())),
                        "flaky" if times == 1 => Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .body(Body::empty())
                            .unwrap(),
                        "gone" => Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Body::empty())
                            .unwrap(),
                        _ => partial(a, z, Body::from(data[a..=z].to_vec())),
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), served)
    }

    fn options(dir: &std::path::Path, attempts: u32) -> Options {
        Options {
            connections: 3,
            control: Control::new(),
            dest_dir: dir.join("out"),
            parts_dir: dir.join("parts"),
            limiter: Limiter::unlimited(),
            categories: Categories::default(),
            checksum: None,
            quality: Quality::Best,
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            media: false,
            referrer: None,
            proxy: None,
            container: None,
            attempts,
            host_limits: Vec::new(),
            proxy_bypass: Vec::new(),
            subtitles: false,
            room: Default::default(),
            remux: false,
            server_dates: false,
            retry_wait: 0,
            record_limit: None,
            frame_rate: None,
            caption: None,
            sign_in: None,
            save_name: None,
            accept_page: false,
            torrent: Default::default(),
            system_proxy: false,
            title_hint: None,
        }
    }

    /// The stream at `{base}/{how}/s.mp4`, its init and index stated where
    /// `stated` holds them, written to `target`.
    fn source(base: &str, how: &str, stated: Option<(usize, usize)>, target: PathBuf) -> Source {
        Source {
            url: format!("{base}/{how}/s.mp4"),
            init: stated.map(|(init, _)| (0, init as u64 - 1)),
            index: stated.map(|(init, index)| (init as u64, index as u64 - 1)),
            agent: Some("dm-test-agent".into()),
            headers: Headers::of([("referer", "http://example.test/page")]),
            chunk: Some(64 << 10),
            target,
        }
    }

    fn span(from: f64, to: f64) -> Span {
        Span::new(from, to).unwrap()
    }

    /// A part with its init and index stated: the init, then the bytes of
    /// the pieces that overlap the span (the second and third), fetched in
    /// slices no larger than the cap, each with the stream's user agent and
    /// headers; progress said up to the whole; and a second fetch of the
    /// same part, now whole on disk, asks for nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_part_is_its_init_and_the_pieces_over_its_span() {
        let (data, init, index) = stream();
        let (base, served) = serve(data.clone(), index).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested/part.mp4");
        let source = source(&base, "honest", Some((init, index)), target.clone());
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let part = fetch_part(
            &crate::client(),
            &source,
            span(1.2, 2.5),
            &options(dir.path(), 3),
            Some(tx),
        )
        .await
        .unwrap();
        let (begin, end) = (index + PIECE, index + 3 * PIECE - 1);
        let expected = [&data[..init], &data[begin..=end]].concat();
        assert_eq!(
            part,
            Part {
                bytes: expected.len() as u64,
                starts: 1.0,
                ends: 3.0
            }
        );
        assert!(
            std::fs::read(&target).unwrap() == expected,
            "the part's bytes differ"
        );
        assert!(!dir.path().join("nested/part.mp4.part").exists());

        let asked = served.asked.lock().unwrap().clone();
        let ranges: Vec<&str> = asked.iter().map(|a| a.range.as_str()).collect();
        assert_eq!(ranges[0], format!("bytes=0-{}", init - 1));
        assert_eq!(ranges[1], format!("bytes={init}-{}", index - 1));
        let slice = 64 << 10;
        let mut slices: Vec<String> = (0..)
            .map(|n| begin + n * slice)
            .take_while(|a| *a <= end)
            .map(|a| format!("bytes={a}-{}", (a + slice - 1).min(end)))
            .collect();
        let mut got: Vec<String> = ranges[2..].iter().map(|r| r.to_string()).collect();
        got.sort();
        slices.sort();
        assert_eq!(got, slices, "the slices asked for");
        assert!(asked
            .iter()
            .all(|a| a.agent == "dm-test-agent" && a.referer == "http://example.test/page"));

        let mut last = None;
        while let Ok(update) = rx.try_recv() {
            if let Update::Progress(p) = update {
                last = Some(p);
            }
        }
        let last = last.expect("no progress was said");
        assert_eq!(last.downloaded, part.bytes);
        assert_eq!(last.total, Some(part.bytes));

        let again = fetch_part(
            &crate::client(),
            &source,
            span(1.2, 2.5),
            &options(dir.path(), 3),
            None,
        )
        .await
        .unwrap();
        assert_eq!(again, part);
        assert_eq!(
            served.asked.lock().unwrap().len(),
            asked.len() + 2,
            "a part whole on disk was fetched again"
        );
    }

    /// With no index stated, the stream's first 256 KiB are read and the
    /// index found in them; the part is the same as with it stated.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_index_is_found_in_the_first_bytes_where_none_is_stated() {
        let (data, init, index) = stream();
        let (base, served) = serve(data.clone(), index).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("part.mp4");
        let source = source(&base, "honest", None, target.clone());
        let part = fetch_part(
            &crate::client(),
            &source,
            span(0.0, 0.5),
            &options(dir.path(), 3),
            None,
        )
        .await
        .unwrap();
        assert_eq!((part.starts, part.ends), (0.0, 1.0));
        let expected = [&data[..init], &data[index..index + PIECE]].concat();
        assert!(std::fs::read(&target).unwrap() == expected);
        let asked = served.asked.lock().unwrap();
        assert_eq!(asked[0].range, format!("bytes=0-{}", PROBE - 1));
    }

    /// A stream shorter than the 256 KiB read to look for its index, with
    /// no index stated: a server answers the range honestly with the bytes
    /// it has (RFC 9110 14.1.2: a last byte past the end means the end), and
    /// the part is cut from them.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stream_shorter_than_the_probe_is_cut_with_no_index_stated() {
        let init = [
            boxed(b"ftyp", b"iso6\0\0\0\0iso6dash"),
            boxed(b"moov", &[7; 200]),
        ]
        .concat();
        let mut data = [init.clone(), sidx(1000, 0, &[(1000, 1000); 2])].concat();
        let index = data.len();
        data.extend((0..2000).map(|i| (i % 251) as u8));
        let (base, _) = serve(data.clone(), usize::MAX).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("part.mp4");
        let part = fetch_part(
            &crate::client(),
            &source(&base, "honest", None, target.clone()),
            span(1.0, 2.0),
            &options(dir.path(), 1),
            None,
        )
        .await
        .expect("a short stream could not be cut");
        assert_eq!((part.starts, part.ends), (1.0, 2.0));
        let expected = [&data[..init.len()], &data[index + 1000..]].concat();
        assert!(std::fs::read(&target).unwrap() == expected);
    }

    /// A stream with no index (a plain MP4's `moov` then `mdat`), or an
    /// index stated where there is none, cannot be cut, and says so; a
    /// span past the stream's end says there is nothing there.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stream_with_no_index_or_nothing_in_the_span_is_refused_in_words() {
        let mut plain = [
            boxed(b"ftyp", b"isom\0\0\0\0isom"),
            boxed(b"moov", &[1; 64]),
        ]
        .concat();
        plain.extend_from_slice(&boxed(b"mdat", &vec![2; PROBE as usize]));
        let (base, _) = serve(plain, usize::MAX).await;
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path(), 3);
        let target = dir.path().join("part.mp4");
        let said = fetch_part(
            &crate::client(),
            &source(&base, "honest", None, target.clone()),
            span(0.0, 1.0),
            &options,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(said.contains("no index of its pieces"), "{said}");
        let said = fetch_part(
            &crate::client(),
            &source(&base, "honest", Some((32, 100)), target.clone()),
            span(0.0, 1.0),
            &options,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(said.contains("no index of its pieces"), "{said}");

        let (data, init, index) = stream();
        let (base, _) = serve(data, index).await;
        let said = fetch_part(
            &crate::client(),
            &source(&base, "honest", Some((init, index)), target.clone()),
            span(10.0, 20.0),
            &options,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(said.contains("nothing between 0:10 and 0:20"), "{said}");
        assert!(!target.exists());
    }

    /// Each way a server can answer a range dishonestly is an error of its
    /// own, and no part is left on disk under its name.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dishonest_answer_to_a_range_is_refused() {
        let (data, init, index) = stream();
        let (base, served) = serve(data, index).await;
        let dir = tempfile::tempdir().unwrap();
        for (how, wanted) in [
            ("ignores", "RangesIgnored"),
            ("shifted", "DishonestRanges"),
            ("longer", "DishonestRanges"),
            ("streams-more", "DishonestRanges"),
            ("streams-less", "Incomplete"),
            ("gone", "Refused"),
        ] {
            served.asked.lock().unwrap().clear();
            let target = dir.path().join(format!("{how}.mp4"));
            let error = fetch_part(
                &crate::client(),
                &source(&base, how, Some((init, index)), target.clone()),
                span(0.0, 1.0),
                &options(dir.path(), 1),
                None,
            )
            .await
            .unwrap_err();
            assert!(format!("{error:?}").starts_with(wanted), "{how}: {error:?}");
            assert!(!target.exists(), "{how}: a part was left under its name");
            // Init, index and the one piece's slices (the second may not be
            // asked once the first has failed), none asked twice.
            assert_one_try(&served, how);
        }
        // A missing piece is not asked again however many tries are left.
        served.asked.lock().unwrap().clear();
        let error = fetch_part(
            &crate::client(),
            &source(
                &base,
                "gone",
                Some((init, index)),
                dir.path().join("gone.mp4"),
            ),
            span(0.0, 1.0),
            &options(dir.path(), 5),
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, Error::Refused { status: 404, .. }),
            "{error:?}"
        );
        assert_one_try(&served, "gone, five tries");
    }

    fn assert_one_try(served: &Served, how: &str) {
        // This case's own requests: the slice a case before it had already
        // sent when its first one failed can reach the server after the log
        // was cleared for this one (seen under coverage instrumentation).
        let asked = served.asked.lock().unwrap();
        let case = how.split(',').next().unwrap_or(how);
        let mine: Vec<&Asked> = asked.iter().filter(|a| a.how == case).collect();
        let mut ranges: Vec<&str> = mine.iter().map(|a| a.range.as_str()).collect();
        assert!((3..=4).contains(&ranges.len()), "{how}: {ranges:?}");
        ranges.sort_unstable();
        ranges.dedup();
        assert_eq!(ranges.len(), mine.len(), "{how}: a range was asked again");
    }

    /// A slice that fails in passing (a 503) is asked again after a wait
    /// and the part is whole; with no number of tries set, ten are allowed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_slice_that_fails_in_passing_is_asked_again() {
        let (data, init, index) = stream();
        let (base, served) = serve(data.clone(), index).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("part.mp4");
        let part = fetch_part(
            &crate::client(),
            &source(&base, "flaky", Some((init, index)), target.clone()),
            span(3.0, 4.0),
            &options(dir.path(), 0),
            None,
        )
        .await
        .unwrap();
        assert_eq!((part.starts, part.ends), (3.0, 4.0));
        let expected = [&data[..init], &data[index + 3 * PIECE..]].concat();
        assert!(std::fs::read(&target).unwrap() == expected);
        let times = served.times.lock().unwrap();
        let slices: Vec<_> = times
            .iter()
            .filter(|(range, _)| {
                range
                    .strip_prefix("bytes=")
                    .and_then(|r| r.split('-').next())
                    .and_then(|a| a.parse::<usize>().ok())
                    .is_some_and(|a| a >= index)
            })
            .collect();
        assert_eq!(slices.len(), 2, "{times:?}");
        assert!(slices.iter().all(|(_, n)| **n == 2), "{times:?}");
    }

    /// A cancel asked for while a slice waits to be asked again is
    /// answered at once, not after the wait; one asked before anything is
    /// fetched stops it before a request.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancel_stops_a_part_at_once() {
        let (data, init, index) = stream();
        let (base, served) = serve(data, index).await;
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path(), 5);
        let control = options.control.clone();
        let source = source(
            &base,
            "flaky",
            Some((init, index)),
            dir.path().join("part.mp4"),
        );
        let started = Instant::now();
        let stop = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            control.cancel();
        });
        let error = fetch_part(&crate::client(), &source, span(0.0, 1.0), &options, None)
            .await
            .unwrap_err();
        stop.await.unwrap();
        assert!(matches!(error, Error::Cancelled), "{error:?}");
        assert!(
            started.elapsed() < Duration::from_millis(1000),
            "{:?}",
            started.elapsed()
        );

        served.asked.lock().unwrap().clear();
        let error = fetch_part(&crate::client(), &source, span(0.0, 1.0), &options, None)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Cancelled), "{error:?}");
        assert!(served.asked.lock().unwrap().is_empty());
    }

    /// A part that cannot be written where it is to go is a disk error.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_part_with_nowhere_to_be_written_is_a_disk_error() {
        let (data, init, index) = stream();
        let (base, _) = serve(data, index).await;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a-file");
        std::fs::write(&file, b"not a folder").unwrap();
        let error = fetch_part(
            &crate::client(),
            &source(&base, "honest", Some((init, index)), file.join("part.mp4")),
            span(0.0, 1.0),
            &options(dir.path(), 1),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Disk(_)), "{error:?}");
    }

    /// The index's other forms: version 1 (64-bit times and offsets), and
    /// a box that says it runs to the end of what was read (size 0) or
    /// gives a 64-bit size (size 1).
    #[test]
    fn an_index_of_version_one_and_boxes_of_every_size_form_are_read() {
        let mut body = vec![1u8, 0, 0, 0];
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&90_000u32.to_be_bytes());
        body.extend_from_slice(&(1u64 << 33).to_be_bytes()); // earliest
        body.extend_from_slice(&5u64.to_be_bytes()); // first offset
        body.extend_from_slice(&[0, 0, 0, 1]);
        body.extend_from_slice(&300u32.to_be_bytes());
        body.extend_from_slice(&45_000u32.to_be_bytes());
        body.extend_from_slice(&0x9000_0000u32.to_be_bytes());
        let v1 = boxed(b"sidx", &body);
        let index = parse_sidx(&v1, 1000).unwrap();
        assert_eq!(index.timescale, 90_000);
        assert_eq!(
            index.pieces,
            [Piece {
                offset: 1000 + v1.len() as u64 + 5,
                size: 300,
                start: 1 << 33,
                duration: 45_000
            }]
        );
        // An index with a timescale of 0, or naming no pieces, is none.
        assert_eq!(parse_sidx(&sidx(0, 0, &[(10, 10)]), 0), None);
        assert_eq!(parse_sidx(&sidx(1000, 0, &[]), 0), None);

        let ftyp = boxed(b"ftyp", b"isom\0\0\0\0");
        let moov = boxed(b"moov", &[0; 16]);
        // A free box of 64-bit size between `moov` and the index.
        let mut free = 1u32.to_be_bytes().to_vec();
        free.extend_from_slice(b"free");
        free.extend_from_slice(&24u64.to_be_bytes());
        free.extend_from_slice(&[0; 8]);
        let index = sidx(1000, 0, &[(50, 1000)]);
        let head = [ftyp.clone(), moov.clone(), free.clone(), index.clone()].concat();
        let init = (ftyp.len() + moov.len()) as u64;
        let at = init + free.len() as u64;
        assert_eq!(
            locate(&head),
            Some((init, (at, at + index.len() as u64 - 1)))
        );
        // A box running to the end with no index after it, a box smaller
        // than its own header, and an index before any `moov`.
        let mut open = 0u32.to_be_bytes().to_vec();
        open.extend_from_slice(b"free");
        assert_eq!(locate(&[ftyp.clone(), moov.clone(), open].concat()), None);
        assert_eq!(
            locate(&[ftyp.clone(), vec![0, 0, 0, 4, 0, 0, 0, 0]].concat()),
            None
        );
        assert_eq!(locate(&[ftyp, index].concat()), None);
    }

    /// Words for seconds that are not seconds are refused, and the clock
    /// and the file name agree for a span under a minute.
    #[test]
    fn odd_words_for_seconds_are_refused() {
        for odd in ["1:x", "1:-2", "1m2x", "m", "1..2s", "1h 2q", "inf", "NaN"] {
            assert_eq!(Span::seconds(odd), None, "{odd}");
        }
        assert_eq!(
            Span::seconds("1m 30"),
            Some(90.0),
            "a bare number after units"
        );
        assert_eq!(Span::new(f64::NAN, 1.0), None);
        assert_eq!(Span::new(-1.0, 1.0), None);
        assert_eq!(Span::new(2.0, f64::INFINITY), None);
        assert_eq!(Span::clock(-5.0), "0:00");
        assert_eq!(span(5.0, 59.6).words(), "0m05s-1m00s");
    }
}
