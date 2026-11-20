// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The framed stream fetched: a request posted, its answer's parts read as they come, each
//! whole piece written to its format's file, and the next request saying what is held, until
//! every format has its last piece.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt as _;

use super::messages::{self, kind, Ask, FormatStart, Held, MediaHeader, NextAsk};
use super::ump::Reader;
use super::{Framed, Wanted, WANTS_TOKEN};
use crate::download::{Options, Update};
use crate::Error;

/// The most requests one fetch posts: each answer brings a few seconds of every format, so a
/// ten-hour video is a few thousand.
const MOST_ASKS: u32 = 20_000;
/// Answers in a row that bring no new piece before the stream is given up on.
const MOST_IDLE: u32 = 8;
/// Failures in a row (a connection lost, a server's error) before the fetch fails.
const MOST_FAILURES: u32 = 5;
/// The longest wait a server's policy is followed for.
const MOST_BACKOFF: Duration = Duration::from_secs(30);
/// The most of the pieces under way held before they are written whole.
const MOST_PIECE: usize = 256 << 20;

/// One format being fetched: the file its pieces go to, and what it holds.
struct Track<'a> {
    wanted: &'a Wanted,
    /// Where its pieces are written until every format is whole.
    partial: PathBuf,
    file: tokio::fs::File,
    written: u64,
    /// Its first word was heard ([`FormatStart`]).
    started: bool,
    /// Its first boxes are written.
    init: bool,
    first: Option<u64>,
    last: Option<u64>,
    /// Where what it holds ends, in milliseconds.
    held_ms: u64,
    end_segment: Option<u64>,
    end_ms: Option<u64>,
}

impl Track<'_> {
    /// Whether it holds its last piece.
    fn done(&self) -> bool {
        let Some(last) = self.last.filter(|_| self.init) else {
            return false;
        };
        match (self.end_segment, self.end_ms) {
            (Some(end), _) => last >= end,
            (None, Some(end)) => self.held_ms >= end,
            (None, None) => false,
        }
    }

    /// Whether a piece belongs next in its file: its first boxes where they are not written,
    /// else the piece after the last one held.
    fn takes(&self, init: bool, sequence: Option<u64>) -> bool {
        if init {
            return !self.init;
        }
        let next = |n: u64| match self.last {
            Some(last) => last.checked_add(1) == Some(n),
            None => true,
        };
        self.init && sequence.is_some_and(next)
    }
}

/// A piece whose media is coming, under its header's id.
struct Piece {
    /// The track it is kept for; `None`, passed over (held already, or not asked for).
    track: Option<usize>,
    init: bool,
    sequence: Option<u64>,
    end_ms: Option<u64>,
    bytes: Vec<u8>,
}

/// What one answer said beyond its pieces.
#[derive(Default)]
struct Heard {
    pieces: u32,
    redirect: Option<String>,
    next: Option<NextAsk>,
    /// The stream's protection status, where it said one.
    protection: Option<u64>,
}

/// What every request of one fetch is made with.
struct Run<'r> {
    client: &'r reqwest::Client,
    framed: &'r Framed,
    options: &'r Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    began: Instant,
}

/// Every format of `wanted` fetched from the framed stream `framed`, each to its `target`:
/// written beside it as it comes, and put there once every one is whole. Where every target is
/// there already (a run stopped after its fetch), nothing is asked.
pub async fn fetch(
    client: &reqwest::Client,
    framed: &Framed,
    wanted: &[Wanted],
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<(), Error> {
    let there = |w: &Wanted| std::fs::metadata(&w.target).is_ok_and(|m| m.len() > 0);
    if !wanted.is_empty() && wanted.iter().all(there) {
        return Ok(());
    }
    let config = framed.config_bytes().ok_or_else(|| {
        Error::Internal("the player's word for its framed stream is not base64".into())
    })?;
    let mut tracks = Vec::new();
    for w in wanted {
        let _ = tokio::fs::remove_file(&w.target).await;
        let mut name = w.target.clone().into_os_string();
        name.push(".framed");
        let partial = PathBuf::from(name);
        let file = tokio::fs::File::create(&partial).await?;
        tracks.push(Track {
            wanted: w,
            partial,
            file,
            written: 0,
            started: false,
            init: false,
            first: None,
            last: None,
            held_ms: 0,
            end_segment: None,
            end_ms: None,
        });
    }
    let run = Run {
        client,
        framed,
        options,
        report,
        began: Instant::now(),
    };
    let asked = match ask_until_whole(&run, &mut tracks, &config).await {
        Ok(asked) => asked,
        Err(e) => {
            // Nothing of it is taken up again: a later run asks the stream from its start.
            for track in tracks {
                drop(track.file);
                let _ = tokio::fs::remove_file(&track.partial).await;
            }
            return Err(e);
        }
    };
    for track in &mut tracks {
        track.file.flush().await?;
        track.file.sync_all().await?;
    }
    for track in tracks {
        drop(track.file);
        tokio::fs::rename(&track.partial, &track.wanted.target).await?;
    }
    crate::note!(
        info,
        "dm: fetched from YouTube's framed stream in {asked} requests"
    );
    Ok(())
}

/// Requests posted until every track holds its last piece: how many were.
async fn ask_until_whole(
    run: &Run<'_>,
    tracks: &mut [Track<'_>],
    config: &[u8],
) -> Result<u32, Error> {
    let mut url = run.framed.url.clone();
    let mut cookie: Option<Vec<u8>> = None;
    let (mut idle, mut failures, mut asked) = (0u32, 0u32, 0u32);
    let mut pending = false;
    while !tracks.iter().all(Track::done) {
        asked += 1;
        if asked > MOST_ASKS {
            return Err(Error::Internal(
                "YouTube's framed stream did not end".into(),
            ));
        }
        let body = ask_of(tracks, config, run.framed, cookie.as_deref()).body();
        let heard = match answered(run, &url, asked, body, tracks).await {
            Ok(heard) => {
                failures = 0;
                heard
            }
            Err(e) if again(&e) && failures + 1 < MOST_FAILURES => {
                failures += 1;
                wait(run.options, Duration::from_secs(u64::from(failures))).await?;
                continue;
            }
            Err(e) => return Err(e),
        };
        if let Some(next) = heard.next {
            cookie = next.cookie.or(cookie);
            if let Some(ms) = next.backoff_ms.filter(|ms| *ms > 0) {
                wait(run.options, Duration::from_millis(ms).min(MOST_BACKOFF)).await?;
            }
        }
        // A token "wanted soon" stands until an answer says otherwise.
        if heard.protection.is_some() {
            pending = heard.protection == Some(2);
        }
        if let Some(to) = heard.redirect {
            url = to;
            idle = 0;
            continue;
        }
        idle = if heard.pieces > 0 { 0 } else { idle + 1 };
        if idle >= MOST_IDLE {
            return Err(Error::Internal(if pending {
                WANTS_TOKEN.into()
            } else {
                "YouTube's framed stream stopped sending before the video's end".into()
            }));
        }
    }
    Ok(asked)
}

/// Whether a failure is worth asking again for: a connection lost or cut short, a server's own
/// error or its "too many".
fn again(e: &Error) -> bool {
    match e {
        Error::Network(_) | Error::Connection(_) | Error::Incomplete { .. } => true,
        Error::Refused { status, .. } => *status == 429 || *status >= 500,
        _ => false,
    }
}

/// A wait, cut short by a pause or a cancel.
async fn wait(options: &Options, spell: Duration) -> Result<(), Error> {
    crate::download::or_stop(&options.control, async {
        tokio::time::sleep(spell).await;
        Ok(())
    })
    .await
}

/// The next request: every format asked for, what is held of each, and where the player is
/// (the least any unfinished format holds).
fn ask_of<'a>(
    tracks: &'a [Track<'_>],
    config: &'a [u8],
    framed: &'a Framed,
    cookie: Option<&'a [u8]>,
) -> Ask<'a> {
    let picks = |video: bool| {
        tracks
            .iter()
            .map(|t| &t.wanted.pick)
            .filter(move |p| p.video == video)
            .collect::<Vec<_>>()
    };
    let player_time_ms = tracks
        .iter()
        .filter(|t| !t.done())
        .map(|t| t.held_ms)
        .min()
        .unwrap_or(0);
    Ask {
        config,
        client_name: framed.client_name,
        client_version: &framed.client_version,
        player_time_ms,
        height: picks(true).iter().map(|p| p.height).max().unwrap_or(0),
        audio: picks(false),
        video: picks(true),
        started: tracks
            .iter()
            .filter(|t| t.started)
            .map(|t| &t.wanted.pick)
            .collect(),
        held: tracks
            .iter()
            .filter_map(|t| {
                Some(Held {
                    pick: &t.wanted.pick,
                    duration_ms: t.held_ms,
                    first: t.first?,
                    last: t.last?,
                })
            })
            .collect(),
        cookie,
    }
}

/// One request posted and its answer read whole, each piece written as it ends.
async fn answered(
    run: &Run<'_>,
    url: &str,
    number: u32,
    body: Vec<u8>,
    tracks: &mut [Track<'_>],
) -> Result<Heard, Error> {
    let control = &run.options.control;
    let joiner = if url.contains('?') { '&' } else { '?' };
    let ask = run
        .client
        .post(format!("{url}{joiner}rn={number}"))
        .header(reqwest::header::USER_AGENT, &run.framed.agent)
        .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
        .header(reqwest::header::ACCEPT, "application/vnd.yt-ump")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .body(body);
    let answer = crate::download::or_stop(control, crate::redirects::send(ask)).await?;
    let answer = Error::refused_or(answer).await?;
    let mut chunks = answer.bytes_stream();
    let mut reader = Reader::new();
    let mut pieces: HashMap<u64, Piece> = HashMap::new();
    let mut heard = Heard::default();
    while let Some(chunk) = crate::download::next_or_stop(&mut chunks, control).await? {
        let chunk = chunk?;
        reader.push(&chunk);
        // The speed ceiling, as every other download is held to it.
        crate::download::or_stop(control, async {
            run.options.limiter.take(chunk.len() as u64).await;
            Ok(())
        })
        .await?;
        while let Some((part, data)) = reader.next_part().map_err(Error::Internal)? {
            let Some(ended) = take(part, &data, tracks, &mut pieces, &mut heard, url)? else {
                continue;
            };
            let Some(piece) = pieces.remove(&ended) else {
                continue;
            };
            if write_piece(piece, tracks).await? {
                heard.pieces += 1;
                progress(run, tracks).await;
            }
        }
    }
    Ok(heard)
}

/// A piece whose media has ended, written to its track's file where it still belongs next
/// there (another of the same answer may have taken its place). Whether it was.
async fn write_piece(piece: Piece, tracks: &mut [Track<'_>]) -> Result<bool, Error> {
    let Some(track) = piece.track.and_then(|at| tracks.get_mut(at)) else {
        return Ok(false);
    };
    if piece.bytes.is_empty() || !track.takes(piece.init, piece.sequence) {
        return Ok(false);
    }
    track.file.write_all(&piece.bytes).await?;
    track.written += piece.bytes.len() as u64;
    if piece.init {
        track.init = true;
    } else {
        track.first = track.first.or(piece.sequence);
        track.last = piece.sequence;
        if let Some(end) = piece.end_ms {
            track.held_ms = track.held_ms.max(end);
        }
    }
    Ok(true)
}

/// The row told how far the fetch has got: the bytes written of every format, against their
/// sizes where the answer stated them all.
async fn progress(run: &Run<'_>, tracks: &[Track<'_>]) {
    let Some(tx) = &run.report else {
        return;
    };
    let downloaded: u64 = tracks.iter().map(|t| t.written).sum();
    let total = tracks
        .iter()
        .map(|t| t.wanted.size)
        .sum::<Option<u64>>()
        .map(|total| total.max(downloaded));
    let seconds = run.began.elapsed().as_secs_f64();
    let speed = if seconds > 0.0 {
        downloaded as f64 / seconds
    } else {
        0.0
    };
    let eta = total.filter(|_| speed > 0.0).and_then(|total| {
        Duration::try_from_secs_f64(total.saturating_sub(downloaded) as f64 / speed).ok()
    });
    let _ = tx
        .send(Update::Progress(crate::Progress {
            downloaded,
            total,
            speed,
            eta,
            estimated: false,
        }))
        .await;
}

/// The track of format `itag`.
fn of_itag(tracks: &[Track<'_>], itag: Option<u64>) -> Option<usize> {
    tracks
        .iter()
        .position(|t| itag == Some(u64::from(t.wanted.pick.itag)))
}

/// What one part says, taken: a piece's header or its media, a format's first word, the
/// policy, a redirect, a refusal. The header id of a piece the part ends, where it ends one.
fn take(
    part: u64,
    data: &[u8],
    tracks: &mut [Track<'_>],
    pieces: &mut HashMap<u64, Piece>,
    heard: &mut Heard,
    url: &str,
) -> Result<Option<u64>, Error> {
    match part {
        kind::MEDIA_HEADER => {
            let Some(header) = MediaHeader::read(data) else {
                return Ok(None);
            };
            if header.compressed {
                return Err(Error::Internal(
                    "YouTube's framed stream sent a compressed piece, which dm does not read"
                        .into(),
                ));
            }
            let track = of_itag(tracks, header.itag)
                .filter(|at| tracks[*at].takes(header.init, header.sequence));
            let end_ms = header
                .start_ms
                .zip(header.duration_ms)
                .map(|(start, length)| start.saturating_add(length));
            let piece = Piece {
                track,
                init: header.init,
                sequence: header.sequence,
                end_ms,
                bytes: Vec::new(),
            };
            pieces.insert(header.header_id, piece);
        }
        kind::MEDIA => {
            let Some((id, bytes)) = messages::media_of(data) else {
                return Ok(None);
            };
            let held: usize = pieces.values().map(|p| p.bytes.len()).sum();
            if let Some(piece) = pieces.get_mut(&id).filter(|p| p.track.is_some()) {
                if held + bytes.len() > MOST_PIECE {
                    return Err(Error::Internal(
                        "YouTube's framed stream sent a piece larger than any video's".into(),
                    ));
                }
                piece.bytes.extend_from_slice(bytes);
            }
        }
        kind::MEDIA_END => return Ok(messages::media_of(data).map(|(id, _)| id)),
        kind::FORMAT_INITIALIZATION_METADATA => {
            let start = FormatStart::read(data);
            if let Some(t) = of_itag(tracks, start.itag).and_then(|at| tracks.get_mut(at)) {
                t.started = true;
                t.end_segment = start.end_segment.or(t.end_segment);
                t.end_ms = start.end_ms.or(t.end_ms);
            }
        }
        kind::NEXT_REQUEST_POLICY => heard.next = Some(NextAsk::read(data)),
        kind::REDIRECT => {
            // On to the web only; to plain `http` only from a stand-in that is.
            let plain = url.starts_with("http://");
            heard.redirect = messages::redirect_of(data)
                .filter(|to| to.starts_with("https://") || (plain && to.starts_with("http://")))
                .or(heard.redirect.take());
        }
        kind::ERROR => {
            return Err(Error::Internal(format!(
                "YouTube's framed stream refused: {}",
                messages::error_of(data)
            )));
        }
        kind::RELOAD_PLAYER_RESPONSE => {
            return Err(Error::Internal(
                "YouTube's framed stream asks for the video to be read again".into(),
            ));
        }
        kind::STREAM_PROTECTION_STATUS => {
            let status = messages::protection_of(data);
            if status == Some(3) {
                return Err(Error::Internal(WANTS_TOKEN.into()));
            }
            heard.protection = status;
        }
        _ => {}
    }
    Ok(None)
}
