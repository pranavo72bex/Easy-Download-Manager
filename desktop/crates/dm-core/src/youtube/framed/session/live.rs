// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A live broadcast's framed stream recorded (gaps row Y2), where its player names no HLS
//! playlist: the requests a video's fetch posts ([`super::fetch`]), each format's pieces added
//! to its file as they come, asked for as the broadcast goes on rather than up to a last piece.
//!
//! - It starts where the broadcast is: the stream's word on its head ([`Head`]) says where the
//!   player is asked to be, and a format that holds nothing takes no piece from much before it,
//!   as a live HLS recording starts at the last few segments its playlist lists. Where only
//!   pieces from before it come, a few answers running, what comes is taken.
//! - Each piece after the last one held is taken; those the stream moved past before dm had
//!   them leave a gap, which the row is told ([`LIVE_PIECES_GONE`]), as a live HLS recording's
//!   is.
//! - An answer that drops is asked again, saying what is held, so the stream goes on from the
//!   last whole piece. What each file holds is written down beside it after each piece, so a
//!   run ended by a crash goes on from there too.
//! - Where nothing new comes for a few pieces' length (no piece, the head not moving on), or the
//!   stream asks for it, the video is read again: no longer live through the framed stream, the
//!   recording ends; still live, it goes on, from the stream that reading names.
//! - A stop keeps what was recorded rather than pausing, as a live HLS recording does; a
//!   cancel's files are cleared by the page's download.
//!
//! The head's field numbers are from the protocol's published descriptions, unchecked against
//! YouTube's servers.

use std::future::Future;

use super::*;
use crate::download::LIVE_PIECES_GONE;

/// A piece's length where the stream has named none: two seconds, as YouTube cuts a
/// low-latency broadcast's.
const PIECE_GUESS: Duration = Duration::from_secs(2);

/// How many pieces' length the broadcast may stand still (no new piece, its head not moving
/// on) before the video is read again, to learn whether it has ended.
const STILL_PIECES: u32 = 4;

/// The least time between two readings of the video while the broadcast stands still.
const READ_AGAIN_EVERY: Duration = Duration::from_secs(30);

/// Readings of the video in a row that may fail before the last failure ends the recording.
const MOST_UNREAD: u32 = 3;

/// Answers in a row that bring only pieces from before where the recording starts, before the
/// head's word is set aside and what comes is taken.
const MOST_EARLY: u32 = 4;

/// Said where a live stream ended before anything was recorded, as a live HLS recording says it.
fn nothing_recorded() -> Error {
    Error::Internal("the live stream sent nothing to record".into())
}

/// What a recording knows of the broadcast between its requests.
struct Watch {
    /// Its head, as the stream last said it.
    head: Option<Head>,
    /// The longest piece heard, in milliseconds.
    piece_ms: Option<u64>,
    /// When a piece last came, or the head last moved on.
    news: Instant,
    /// When the video was last read again.
    read_at: Option<Instant>,
}

impl Watch {
    /// Nothing heard yet, at `now`.
    fn new(now: Instant) -> Watch {
        Watch {
            head: None,
            piece_ms: None,
            news: now,
            read_at: None,
        }
    }

    /// A piece's length, as the stream cuts them.
    fn piece(&self) -> Duration {
        self.piece_ms
            .filter(|ms| *ms > 0)
            .map_or(PIECE_GUESS, Duration::from_millis)
    }

    /// What an answer said, taken at `now`: a piece written, or a head further on, is news. A
    /// head said again, or one behind the last, is not.
    fn heard(&mut self, head: Option<Head>, piece_ms: Option<u64>, pieces: u32, now: Instant) {
        let moved = head.is_some_and(|h| self.head.is_none_or(|was| h.sequence > was.sequence));
        if moved {
            self.head = head;
        }
        self.piece_ms = self.piece_ms.max(piece_ms);
        if moved || pieces > 0 {
            self.news = now;
        }
    }

    /// Whether the broadcast has stood still long enough at `now` that the video is read
    /// again: a few pieces' length with nothing new, and not read again for a while.
    fn still(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.news) >= self.piece() * STILL_PIECES
            && self
                .read_at
                .is_none_or(|at| now.saturating_duration_since(at) >= READ_AGAIN_EVERY)
    }
}

/// What a recording counts between its requests.
#[derive(Default)]
struct Counts {
    /// Requests posted.
    asked: u32,
    /// Answers in a row that brought no piece.
    idle: u32,
    /// Failures in a row.
    failures: u32,
    /// Answers in a row that brought only pieces from before where the recording starts.
    early: u32,
    /// Readings of the video in a row that failed.
    unread: u32,
    /// The pieces gone the row was told of.
    told: u64,
}

/// Every format of `wanted` recorded from the live broadcast's framed stream `framed`, each to
/// its `target`, until the broadcast ends or the person stops it: written beside it as it
/// comes, and put there once the recording ends. `read_again` reads the video again: the
/// framed stream it names where it is still live through one, `None` where it is not. Stopped
/// or failed, what was recorded is kept; ended with nothing recorded, the error, or that
/// nothing came.
pub async fn record_live<F, Fut>(
    client: &reqwest::Client,
    framed: &Framed,
    wanted: &[Wanted],
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    read_again: F,
) -> Result<(), Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<Option<Framed>, Error>>,
{
    // Recorded already (a run stopped while the recording was joined): nothing is asked.
    let there = |w: &Wanted| std::fs::metadata(&w.target).is_ok_and(|m| m.len() > 0);
    if wanted.iter().any(there) {
        return Ok(());
    }
    let mut tracks = Vec::new();
    for w in wanted {
        let mut track = track_of(w).await?;
        track.live = true;
        track.head_start = track.held.last.is_none();
        tracks.push(track);
    }
    let run = Run {
        client,
        framed,
        options,
        report,
        began: Instant::now(),
        kept: tracks.iter().map(|t| t.held.written).sum(),
    };
    let ended = record_until_end(&run, &mut tracks, read_again).await;
    for track in &mut tracks {
        let _ = track.file.sync_data().await;
        let _ = track.ledger.sync_data().await;
    }
    let recorded = tracks.iter().any(|t| t.held.last.is_some());
    match ended {
        Ok(()) | Err(Error::Paused) => {}
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        // Kept with their records, for a run after to go on from.
        Err(e) if !recorded => return Err(e),
        // Whatever stopped it, what was recorded is kept.
        Err(e) => crate::note!(
            info,
            "dm: the live recording ended ({e}); what it holds is kept"
        ),
    }
    if !recorded {
        return Err(nothing_recorded());
    }
    placed(tracks).await
}

/// Each track that holds a piece put at its target, the others' files removed, and every
/// record with them.
async fn placed(tracks: Vec<Track<'_>>) -> Result<(), Error> {
    for track in tracks {
        let whole = track.held.last.is_some();
        drop(track.file);
        drop(track.ledger);
        if whole {
            tokio::fs::rename(&track.partial, &track.wanted.target).await?;
        } else {
            let _ = tokio::fs::remove_file(&track.partial).await;
        }
        let _ = tokio::fs::remove_file(record::path_of(&track.partial)).await;
    }
    Ok(())
}

/// Requests posted as the broadcast goes on, until it ends (`Ok`) or the recording is stopped
/// or fails (the error).
async fn record_until_end<F, Fut>(
    run: &Run<'_>,
    tracks: &mut [Track<'_>],
    read_again: F,
) -> Result<(), Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<Option<Framed>, Error>>,
{
    let mut framed = run.framed.clone();
    let mut config = config_of(&framed)?;
    let mut url = framed.url.clone();
    let mut cookie: Option<Vec<u8>> = None;
    // The contexts the stream gives, sent back in every request after.
    let mut contexts = Contexts::default();
    let mut watch = Watch::new(Instant::now());
    let mut count = Counts::default();
    let mut pending = false;
    loop {
        count.asked = count.asked.saturating_add(1);
        let time_ms = live_player_time(tracks, watch.head);
        let body = ask_of(
            tracks,
            &config,
            &framed,
            cookie.as_deref(),
            &contexts,
            time_ms,
        )
        .body();
        // Asked as the stream named now, which a reading again may renew.
        let answer = {
            let asking = Run {
                client: run.client,
                framed: &framed,
                options: run.options,
                report: run.report.clone(),
                began: run.began,
                kept: run.kept,
            };
            answered(&asking, &url, count.asked, body, tracks).await
        };
        let heard = match answer {
            Ok(heard) => {
                count.failures = 0;
                heard
            }
            // Dropped: asked again, saying what is held, so it goes on from the last whole piece.
            Err(e) if again(&e) && count.failures + 1 < MOST_FAILURES => {
                count.failures += 1;
                wait(run.options, Duration::from_secs(u64::from(count.failures))).await?;
                continue;
            }
            Err(e) => return Err(e),
        };
        for (part, data) in &heard.contexts {
            contexts.take(*part, data);
        }
        let mut backoff = Duration::ZERO;
        if let Some(next) = heard.next {
            cookie = next.cookie.or(cookie);
            backoff = next
                .backoff_ms
                .map_or(Duration::ZERO, Duration::from_millis);
        }
        // A token "wanted soon" stands until an answer says otherwise.
        if heard.protection.is_some() {
            pending = heard.protection == Some(2);
        }
        tell_missed(run, tracks, &mut count.told).await;
        let now = Instant::now();
        watch.heard(heard.head, heard.piece_ms, heard.pieces, now);
        if let Some(to) = heard.redirect {
            url = to;
            count.idle = 0;
            continue;
        }
        count.idle = if heard.pieces > 0 { 0 } else { count.idle + 1 };
        count.early = if heard.pieces == 0 && heard.early > 0 {
            count.early + 1
        } else {
            0
        };
        if count.early >= MOST_EARLY {
            take_what_comes(tracks);
        }
        let recorded = tracks.iter().any(|t| t.held.last.is_some());
        if count.idle >= MOST_IDLE && (pending || !recorded) {
            return Err(if pending {
                Error::Internal(WANTS_TOKEN.into())
            } else {
                nothing_recorded()
            });
        }
        if heard.reload || (recorded && watch.still(now)) {
            watch.read_at = Some(now);
            match read_again().await {
                // No longer live through the framed stream: the broadcast has ended.
                Ok(None) => return Ok(()),
                // Live through a stream named afresh (its address renewed): asked there on.
                Ok(Some(fresh)) if fresh != framed => {
                    config = config_of(&fresh)?;
                    url = fresh.url.clone();
                    framed = fresh;
                    cookie = None;
                    contexts = Contexts::default();
                    count.unread = 0;
                }
                Ok(Some(_)) if heard.reload => return Err(read_again_asked()),
                Ok(Some(_)) => count.unread = 0,
                Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
                Err(e) => {
                    count.unread += 1;
                    if count.unread >= MOST_UNREAD {
                        return Err(e);
                    }
                }
            }
        }
        // Where nothing new came, half a piece's length, so a new one is not left waiting
        // long; the stream's own wait where it asks a longer one.
        let idle_wait = if heard.pieces == 0 {
            watch.piece() / 2
        } else {
            Duration::ZERO
        };
        let spell = backoff.max(idle_wait).min(MOST_BACKOFF);
        if !spell.is_zero() {
            wait(run.options, spell).await?;
        }
    }
}

/// Where the player is, for a live recording's request: the least any format holds up to; a
/// format that holds no piece yet where the broadcast's head starts (the recording starts
/// there), or at the stream's start where the head is not known.
fn live_player_time(tracks: &[Track<'_>], head: Option<Head>) -> u64 {
    let start = head.and_then(|h| h.time_ms).unwrap_or(0);
    tracks
        .iter()
        .map(|t| {
            if t.held.last.is_some() {
                t.held.held_ms
            } else {
                start
            }
        })
        .min()
        .unwrap_or(0)
}

/// The row told how many pieces the stream moved past before dm had them, where more than it
/// was told before.
async fn tell_missed(run: &Run<'_>, tracks: &[Track<'_>], told: &mut u64) {
    let missed = tracks.iter().map(|t| t.missed).max().unwrap_or(0);
    if missed <= *told {
        return;
    }
    *told = missed;
    if let Some(tx) = &run.report {
        let said = LIVE_PIECES_GONE.replace("{}", &missed.to_string());
        let _ = tx.send(Update::Note(said)).await;
    }
}

/// The head's word set aside: only pieces from before where it says the recording starts have
/// come, a few answers running, so it is not read as it is meant. What comes is taken.
fn take_what_comes(tracks: &mut [Track<'_>]) {
    for track in tracks.iter_mut().filter(|t| t.head_start) {
        crate::note!(
            info,
            "dm: the live stream's format {} sends only pieces from before its head; taken as they come",
            track.wanted.pick.itag
        );
        track.head_start = false;
        track.from = None;
    }
}

#[cfg(test)]
mod tests;
