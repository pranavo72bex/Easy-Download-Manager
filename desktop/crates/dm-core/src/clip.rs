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
    pub from: f64,
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
    pub offset: u64,
    pub size: u64,
    pub start: u64,
    pub duration: u64,
}

/// A fragmented file's index (`sidx`): its pieces, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub timescale: u32,
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
    let expected = to - from + 1;
    // The answer must be the range asked for, said so before any of it is
    // read: a server answering from another offset, or with more than was
    // asked, is not read into memory and then found out.
    let served = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes "))
        .and_then(|v| v.split_once('/'))
        .and_then(|(span, _)| span.split_once('-'))
        .and_then(|(a, b)| Some((a.trim().parse::<u64>().ok()?, b.trim().parse::<u64>().ok()?)));
    if served != Some((from, to)) {
        return Err(Error::DishonestRanges);
    }
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
