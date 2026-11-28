// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The framed stream fetched: a request posted, its answer's parts read as they come, each
//! whole piece written to its format's file, and the next request saying what is held, until
//! every format has its last piece. What each file holds is written down beside it after each
//! piece ([`record`]), so a fetch paused or failed goes on from there. A live broadcast's are
//! recorded by the same requests until it ends ([`live`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt as _;

use super::context::Contexts;
use super::messages::{self, kind, Ask, FormatStart, Head, Held, MediaHeader, NextAsk};
use super::record::{self, Record};
use super::ump::Reader;
use super::{Framed, Wanted, WANTS_TOKEN};
use crate::download::{Options, Update};
use crate::Error;

mod live;

pub use live::record_live;

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
/// How many pieces before a live broadcast's head a recording that holds nothing yet takes
/// one from: about where a live HLS recording starts, at the last few its playlist lists.
const LIVE_BEHIND: u64 = 3;

/// One format being fetched: the file its pieces go to, and what it holds.
struct Track<'a> {
    wanted: &'a Wanted,
    /// Where its pieces are written until every format is whole.
    partial: PathBuf,
    file: tokio::fs::File,
    /// Its record ([`record::path_of`]), written over after each whole piece.
    ledger: tokio::fs::File,
    /// What it holds: its first word heard ([`FormatStart`]), its first boxes, the pieces.
    held: Record,
    /// A live broadcast's: any piece after the last one held is taken, those between gone.
    live: bool,
    /// A live broadcast's that holds no piece yet: it starts at the broadcast's head, `from`
    /// set from the stream's word on it ([`Head`]).
    head_start: bool,
    /// The first piece it takes while it holds none, where its head is known.
    from: Option<u64>,
    /// Pieces the stream moved past before they were had: a live recording's gaps.
    missed: u64,
}

impl Track<'_> {
    /// Whether it holds its last piece.
    fn done(&self) -> bool {
        let held = &self.held;
        let Some(last) = held.last.filter(|_| held.init) else {
            return false;
        };
        match (held.end_segment, held.end_ms) {
            (Some(end), _) => last >= end,
            (None, Some(end)) => held.held_ms >= end,
            (None, None) => false,
        }
    }

    /// Whether a piece belongs next in its file: its first boxes where they are not written,
    /// else the piece after the last one held (a live broadcast's: any after it, and its first
    /// from where it starts).
    fn takes(&self, init: bool, sequence: Option<u64>) -> bool {
        if init {
            return !self.held.init;
        }
        let next = |n: u64| match self.held.last {
            Some(last) if self.live => n > last,
            Some(last) => last.checked_add(1) == Some(n),
            None => self.from.is_none_or(|from| n >= from),
        };
        self.held.init && sequence.is_some_and(next)
    }

    /// Whether a piece is passed over only as it comes from before where a live recording
    /// starts.
    fn before_start(&self, init: bool, sequence: Option<u64>) -> bool {
        !init
            && self.held.init
            && self.held.last.is_none()
            && matches!((self.from, sequence), (Some(from), Some(n)) if n < from)
    }
}

/// Format `wanted`'s file, beside its target: taken up from its last whole piece where an
/// earlier run left it and its record agrees, else begun again.
async fn track_of(wanted: &Wanted) -> Result<Track<'_>, Error> {
    let _ = tokio::fs::remove_file(&wanted.target).await;
    let mut name = wanted.target.clone().into_os_string();
    name.push(".framed");
    let partial = PathBuf::from(name);
    let (file, held, kept) = match record::taken_up(&partial, &wanted.pick).await {
        Some((file, held)) => (file, held, true),
        None => (
            tokio::fs::File::create(&partial).await?,
            Record::of(&wanted.pick),
            false,
        ),
    };
    let ledger = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(!kept)
        .open(record::path_of(&partial))
        .await?;
    if kept {
        crate::note!(
            info,
            "EDM: the framed stream's format {} goes on after its piece {}",
            wanted.pick.itag,
            held.last.unwrap_or(0)
        );
    }
    Ok(Track {
        wanted,
        partial,
        file,
        ledger,
        held,
        live: false,
        head_start: false,
        from: None,
        missed: 0,
    })
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
    /// Its context updates and sending policies, in order: each part's kind and bytes.
    contexts: Vec<(u64, Vec<u8>)>,
    /// A live broadcast's head, as the answer last said it.
    head: Option<Head>,
    /// The longest piece the answer's headers named, in milliseconds.
    piece_ms: Option<u64>,
    /// Pieces passed over only as they came from before where a live recording starts.
    early: u32,
    /// The stream asked for the video to be read again.
    reload: bool,
}

/// What every request of one fetch is made with.
struct Run<'r> {
    client: &'r reqwest::Client,
    framed: &'r Framed,
    options: &'r Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    began: Instant,
    /// The bytes an earlier run left, not counted in this one's speed.
    kept: u64,
}

/// Every format of `wanted` fetched from the framed stream `framed`, each to its `target`:
/// written beside it as it comes, and put there once every one is whole. Where every target is
/// there already (a run stopped after its fetch), nothing is asked. Stopped or failed, what is
/// written is kept with its record, as a ranged download's parts are, and the next run asks
/// only for the pieces after it.
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
    let config = config_of(framed)?;
    let mut tracks = Vec::new();
    for w in wanted {
        tracks.push(track_of(w).await?);
    }
    let run = Run {
        client,
        framed,
        options,
        report,
        began: Instant::now(),
        kept: tracks.iter().map(|t| t.held.written).sum(),
    };
    let asked = match ask_until_whole(&run, &mut tracks, &config).await {
        Ok(asked) => asked,
        Err(e) => {
            // Kept, each with its record, for the next run to go on from; a cancel's staging
            // is cleared by the page's download, as every other download's parts are.
            for track in &mut tracks {
                let _ = track.file.sync_data().await;
                let _ = track.ledger.sync_data().await;
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
        drop(track.ledger);
        tokio::fs::rename(&track.partial, &track.wanted.target).await?;
        let _ = tokio::fs::remove_file(record::path_of(&track.partial)).await;
    }
    crate::note!(
        info,
        "EDM: fetched from YouTube's framed stream in {asked} requests"
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
    // The contexts the stream gives, sent back in every request after.
    let mut contexts = Contexts::default();
    let (mut idle, mut failures, mut asked) = (0u32, 0u32, 0u32);
    let mut pending = false;
    while !tracks.iter().all(Track::done) {
        asked += 1;
        if asked > MOST_ASKS {
            return Err(Error::Internal(
                "YouTube's framed stream did not end".into(),
            ));
        }
        let time_ms = player_time(tracks);
        let body = ask_of(
            tracks,
            config,
            run.framed,
            cookie.as_deref(),
            &contexts,
            time_ms,
        )
        .body();
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
        if heard.reload {
            return Err(read_again_asked());
        }
        for (part, data) in &heard.contexts {
            contexts.take(*part, data);
        }
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

/// Said where the stream asks for the video to be read again.
fn read_again_asked() -> Error {
    Error::Internal("YouTube's framed stream asks for the video to be read again".into())
}

/// The bytes of `framed`'s config, which every request carries.
fn config_of(framed: &Framed) -> Result<Vec<u8>, Error> {
    framed.config_bytes().ok_or_else(|| {
        Error::Internal("the player's word for its framed stream is not base64".into())
    })
}

/// Where the player is, for a video's request: the least any unfinished format holds up to.
fn player_time(tracks: &[Track<'_>]) -> u64 {
    tracks
        .iter()
        .filter(|t| !t.done())
        .map(|t| t.held.held_ms)
        .min()
        .unwrap_or(0)
}

/// The next request: every format asked for, what is held of each, where the player is
/// (`player_time_ms`), and the contexts the stream gave to be sent back.
fn ask_of<'a>(
    tracks: &'a [Track<'_>],
    config: &'a [u8],
    framed: &'a Framed,
    cookie: Option<&'a [u8]>,
    contexts: &'a Contexts,
    player_time_ms: u64,
) -> Ask<'a> {
    let picks = |video: bool| {
        tracks
            .iter()
            .map(|t| &t.wanted.pick)
            .filter(move |p| p.video == video)
            .collect::<Vec<_>>()
    };
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
            .filter(|t| t.held.started)
            .map(|t| &t.wanted.pick)
            .collect(),
        held: tracks
            .iter()
            .filter_map(|t| {
                Some(Held {
                    pick: &t.wanted.pick,
                    duration_ms: t.held.held_ms,
                    first: t.held.first?,
                    last: t.held.last?,
                })
            })
            .collect(),
        cookie,
        contexts: contexts.sent(),
        unsent: contexts.unsent(),
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
/// there (another of the same answer may have taken its place), and then, once it is on the
/// disk itself, written down in the track's record. Whether it was.
async fn write_piece(piece: Piece, tracks: &mut [Track<'_>]) -> Result<bool, Error> {
    let Some(track) = piece.track.and_then(|at| tracks.get_mut(at)) else {
        return Ok(false);
    };
    if piece.bytes.is_empty() || !track.takes(piece.init, piece.sequence) {
        return Ok(false);
    }
    track.file.write_all(&piece.bytes).await?;
    // A live broadcast's pieces between the last held and this one were gone before EDM had
    // them; a video's never are (only the next is taken).
    if let (false, Some(last), Some(n)) = (piece.init, track.held.last, piece.sequence) {
        track.missed += n.saturating_sub(last).saturating_sub(1);
    }
    let held = &mut track.held;
    held.written += piece.bytes.len() as u64;
    if piece.init {
        held.init = true;
    } else {
        held.first = held.first.or(piece.sequence);
        held.last = piece.sequence;
        if let Some(end) = piece.end_ms {
            held.held_ms = held.held_ms.max(end);
        }
    }
    // Behind the bytes, never ahead of them: a record that lags costs a piece fetched again.
    track.file.flush().await?;
    track.file.sync_data().await?;
    record::put(&mut track.ledger, &track.held).await?;
    Ok(true)
}

/// The row told how far the fetch has got: the bytes written of every format, against their
/// sizes where the answer stated them all.
async fn progress(run: &Run<'_>, tracks: &[Track<'_>]) {
    let Some(tx) = &run.report else {
        return;
    };
    let downloaded: u64 = tracks.iter().map(|t| t.held.written).sum();
    let total = tracks
        .iter()
        .map(|t| t.wanted.size)
        .sum::<Option<u64>>()
        .map(|total| total.max(downloaded));
    let seconds = run.began.elapsed().as_secs_f64();
    let speed = if seconds > 0.0 {
        downloaded.saturating_sub(run.kept) as f64 / seconds
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
/// policy, a context to send back, a redirect, a refusal. The header id of a piece the part
/// ends, where it ends one.
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
                    "YouTube's framed stream sent a compressed piece, which EDM does not read"
                        .into(),
                ));
            }
            let at = of_itag(tracks, header.itag);
            let track = at.filter(|at| tracks[*at].takes(header.init, header.sequence));
            if track.is_none()
                && at.is_some_and(|at| tracks[at].before_start(header.init, header.sequence))
            {
                heard.early += 1;
            }
            heard.piece_ms = heard.piece_ms.max(header.duration_ms);
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
                t.held.started = true;
                t.held.end_segment = start.end_segment.or(t.held.end_segment);
                t.held.end_ms = start.end_ms.or(t.held.end_ms);
            }
        }
        kind::LIVE_METADATA => {
            // A live broadcast's head: where a recording that holds nothing yet starts.
            if let Some(head) = Head::read(data) {
                heard.head = Some(head);
                for t in tracks.iter_mut().filter(|t| t.head_start) {
                    t.from = Some(head.sequence.saturating_sub(LIVE_BEHIND));
                }
            }
        }
        kind::NEXT_REQUEST_POLICY => heard.next = Some(NextAsk::read(data)),
        kind::SABR_CONTEXT_UPDATE | kind::SABR_CONTEXT_SENDING_POLICY => {
            heard.contexts.push((part, data.to_vec()));
        }
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
        // A video's fetch ends on it (`read_again_asked`); a live recording reads the video
        // again.
        kind::RELOAD_PLAYER_RESPONSE => heard.reload = true,
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
