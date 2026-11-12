// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A manifest's periods: when each starts and how long it lasts, the longest
//! run of alike ones joined into one track, and a live manifest read as it
//! stands at one moment.
//!
//! A stream may change period part-way: a programme split in chapters, or an
//! advert put between two halves. Where periods' picture and sound are the
//! same kind (codec, container, size, sound rate, and each with an
//! initialisation piece), their pieces are fetched one after the other into
//! one track, the first period's initialisation piece standing for all: the
//! samples then play on from one period into the next, as the muxer counts
//! each sample's length from the first, so each period starts where the one
//! before it ended. A period unlike the rest (an advert in another format)
//! is left out and the longest group of alike periods taken; the download
//! checks each later initialisation piece is alike before joining
//! (`download::dash`). A live manifest (`type="dynamic"`) lists, at one
//! moment, the pieces the clock says are made: the download reads it again
//! and again and records the new ones (`download::dash_live`).

use super::*;

/// How much of a live period has been made, read from the clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Clock {
    /// Seconds of the period made so far; `None` where the manifest gives no
    /// start (`availabilityStartTime`) to count from.
    pub(super) elapsed: Option<f64>,
    /// How far back the server keeps pieces (`timeShiftBufferDepth`), in
    /// seconds, where it says.
    pub(super) depth: Option<f64>,
}

/// One period, with how long it lasts, in seconds.
pub(super) struct PeriodAt<'a> {
    pub(super) node: &'a Node,
    /// How long it lasts, where the manifest says or its neighbours do.
    pub(super) duration: Option<f64>,
    /// Set for a live manifest read at a moment.
    pub(super) clock: Option<Clock>,
}

/// The manifest's periods in order, each with its start and length: its own
/// `start`, else where the one before ended; its own `duration`, else up to
/// the next one's start, else (the last) up to the stream's end. `now`
/// (seconds since 1970) reads a live manifest's periods against the clock.
pub(super) fn periods_of(root: &Node, now: Option<f64>) -> Vec<PeriodAt<'_>> {
    let nodes: Vec<&Node> = root.children("Period").collect();
    let total = root.attr("mediaPresentationDuration").and_then(iso_seconds);
    let live = root.attr("type") == Some("dynamic");
    let available = root.attr("availabilityStartTime").and_then(unix_seconds);
    let depth = root.attr("timeShiftBufferDepth").and_then(iso_seconds);
    let mut out = Vec::with_capacity(nodes.len());
    let mut follows = 0.0;
    for (i, node) in nodes.iter().enumerate() {
        let start = node.attr("start").and_then(iso_seconds).unwrap_or(follows);
        let next_start = nodes
            .get(i + 1)
            .and_then(|n| n.attr("start"))
            .and_then(iso_seconds);
        let duration = node
            .attr("duration")
            .and_then(iso_seconds)
            .or_else(|| next_start.map(|next| next - start))
            .or_else(|| {
                (i + 1 == nodes.len())
                    .then_some(total)
                    .flatten()
                    .map(|t| t - start)
            })
            .filter(|d| d.is_finite() && *d >= 0.0);
        let clock = match (live, now) {
            (true, Some(now)) => Some(Clock {
                elapsed: available.map(|at| {
                    let made = now - at - start;
                    duration.map_or(made, |d| made.min(d))
                }),
                depth,
            }),
            _ => None,
        };
        follows = start + duration.unwrap_or(0.0);
        out.push(PeriodAt {
            node,
            duration,
            clock,
        });
    }
    out
}

/// An `xs:dateTime` (`2026-11-12T10:00:00Z`) in seconds since 1970; one with
/// no zone is taken as UTC, as the manifest's clock is.
pub(super) fn unix_seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    let millis = match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(at) => at.timestamp_millis(),
        Err(_) => chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
            .ok()?
            .and_utc()
            .timestamp_millis(),
    };
    Some(millis as f64 / 1000.0)
}

/// The numbered pieces of `piece` seconds each that a live period has made
/// whole `elapsed` seconds into it: the first's index (0 the period's first)
/// and how many, kept to the `depth` seconds the server keeps (the last three
/// where it does not say), and to [`MOST_PIECES`].
pub(super) fn live_window(elapsed: f64, piece: f64, depth: Option<f64>) -> (u64, u64) {
    if !(elapsed.is_finite() && piece.is_finite() && piece > 0.0 && elapsed > 0.0) {
        return (0, 0);
    }
    let made = (elapsed / piece).floor();
    let kept = depth
        .filter(|d| d.is_finite() && *d > 0.0)
        .map_or(3.0, |d| (d / piece).floor().max(1.0));
    let first = (made - kept).max(0.0);
    let count = (made - first).min(MOST_PIECES as f64);
    (first as u64, count as u64)
}

/// One period's picture and sound, as taken.
type Taken = (Option<Candidate>, Option<Candidate>);

/// What every track of a period must say alike for the period to be joined
/// with another: each present track's shape. `None` where a present track
/// cannot be joined at all, and the period stands alone.
fn key_of(taken: &Taken) -> Option<(Option<String>, Option<String>)> {
    let shape = |c: &Option<Candidate>| -> Option<Option<String>> {
        match c {
            None => Some(None),
            Some(c) => c.shape.clone().map(Some),
        }
    };
    Some((shape(&taken.0)?, shape(&taken.1)?))
}

/// How long a period plays, for choosing the longest group: what the manifest
/// says, else what its pieces say.
fn weight(period: &PeriodAt, taken: &Taken) -> f64 {
    period.duration.unwrap_or_else(|| {
        taken.0.as_ref().or(taken.1.as_ref()).map_or(0.0, |c| {
            c.track.pieces.iter().map(|p| p.millis).sum::<u64>() as f64 / 1000.0
        })
    })
}

/// The heaviest group of alike items, as their indices in order. `keys[i]`
/// is `None` where `i` is no item (it is skipped), `Some(None)` where it is
/// an item alike to no other, and `Some(Some(key))` where it is alike to the
/// items of the same key; `weights[i]` is how much it counts. The earliest
/// group wins among equals. Empty where there are no items.
pub(crate) fn heaviest<K: PartialEq>(keys: &[Option<Option<K>>], weights: &[f64]) -> Vec<usize> {
    let mut groups: Vec<(Option<&K>, Vec<usize>, f64)> = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        let Some(key) = key else {
            continue; // not an item: a period with nothing to take
        };
        let weight = weights.get(i).copied().unwrap_or(0.0);
        let joinable = key.as_ref();
        match groups
            .iter_mut()
            .find(|(k, _, _)| joinable.is_some() && *k == joinable)
        {
            Some((_, members, total)) => {
                members.push(i);
                *total += weight;
            }
            None => groups.push((joinable, vec![i], weight)),
        }
    }
    let mut best: Option<(Vec<usize>, f64)> = None;
    for (_, members, total) in groups {
        if best.as_ref().is_none_or(|(_, most)| total > *most) {
            best = Some((members, total));
        }
    }
    best.map(|(members, _)| members).unwrap_or_default()
}

/// `later`'s pieces put after `track`'s, its initialisation piece kept aside
/// at the seam.
fn append(track: &mut Track, later: Track) -> Result<(), Error> {
    let mut pieces = later.pieces.into_iter();
    let Some(init) = pieces.next() else {
        return Ok(());
    };
    track.seams.push(Seam {
        at: track.pieces.len(),
        init,
    });
    track.pieces.extend(pieces);
    if track.pieces.len() > MOST_PIECES {
        return Err(Error::Internal(
            "a manifest's periods are not one dm can read (far too many pieces)".into(),
        ));
    }
    Ok(())
}

/// The picture and sound to fetch over the manifest's periods, and how many
/// periods are left out: each period's taken at `quality`, grouped with the
/// periods whose picture and sound are alike, and the longest group (by
/// playing time; the earliest among equals) joined in order. A programme
/// with an advert in another format between its halves is so kept whole,
/// the advert left out. A period dm cannot read is left out; where none can
/// be read, the first one's reason is given.
pub(super) fn joined(
    root: &Node,
    periods: &[PeriodAt],
    url: &str,
    quality: Quality,
    container: Option<&str>,
) -> Result<(Option<Track>, Option<Track>, usize), Error> {
    let mut taken: Vec<Option<Taken>> = Vec::with_capacity(periods.len());
    let mut first_error: Option<Error> = None;
    for period in periods {
        match candidates(root, period, url).map(|all| choose(all, quality, container)) {
            Ok((None, None)) => {
                first_error.get_or_insert_with(|| Error::Internal(NOTHING_FETCHABLE.into()));
                taken.push(None);
            }
            Ok(pair) => taken.push(Some(pair)),
            Err(e) => {
                first_error.get_or_insert(e);
                taken.push(None);
            }
        }
    }
    let keys: Vec<_> = taken.iter().map(|t| t.as_ref().map(key_of)).collect();
    let weights: Vec<f64> = taken
        .iter()
        .zip(periods)
        .map(|(t, p)| t.as_ref().map_or(0.0, |t| weight(p, t)))
        .collect();
    let kept = heaviest(&keys, &weights);
    if kept.is_empty() {
        return Err(first_error
            .unwrap_or_else(|| Error::Internal("that manifest lists nothing to download".into())));
    }
    let mut group = taken
        .into_iter()
        .enumerate()
        .filter(|(i, _)| kept.contains(i))
        .filter_map(|(_, t)| t);
    let Some((mut video, mut audio)) = group
        .next()
        .map(|(v, a)| (v.map(|c| c.track), a.map(|c| c.track)))
    else {
        return Err(Error::Internal(NOTHING_FETCHABLE.into()));
    };
    for (later_video, later_audio) in group {
        if let (Some(track), Some(later)) = (video.as_mut(), later_video) {
            append(track, later.track)?;
        }
        if let (Some(track), Some(later)) = (audio.as_mut(), later_audio) {
            append(track, later.track)?;
        }
    }
    Ok((video, audio, periods.len() - kept.len()))
}

/// A track joined from several periods, cut to the periods `keep` names (0
/// its first; in order): the first kept period's own initialisation piece
/// leads, and each later one's is kept at its seam. `None` where `keep` names
/// none or one past the track's periods.
pub fn keep_periods(track: &Track, keep: &[usize]) -> Option<Track> {
    let lead = track.pieces.first()?;
    // Each period's initialisation piece, and where its pieces begin and end.
    let inits: Vec<&Piece> = std::iter::once(lead)
        .chain(track.seams.iter().map(|s| &s.init))
        .collect();
    let starts: Vec<usize> = std::iter::once(1)
        .chain(track.seams.iter().map(|s| s.at))
        .collect();
    let end_of = |k: usize| starts.get(k + 1).copied().unwrap_or(track.pieces.len());
    let (&first, _) = keep.split_first()?;
    let mut pieces = vec![(*inits.get(first)?).clone()];
    let mut seams = Vec::new();
    for &k in keep {
        if k != first {
            seams.push(Seam {
                at: pieces.len(),
                init: (*inits.get(k)?).clone(),
            });
        }
        pieces.extend(
            track
                .pieces
                .get(*starts.get(k)?..end_of(k))?
                .iter()
                .cloned(),
        );
    }
    Some(Track {
        pieces,
        seams,
        ..track.clone()
    })
}

/// One period of a live manifest, as it stands: its picture and its sound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePeriod {
    /// The picture to record, where there is one.
    pub video: Option<LiveTrack>,
    /// The sound to record, where it is listed apart.
    pub audio: Option<LiveTrack>,
}

/// One track of a live period: its initialisation piece and the pieces made
/// so far that the server still keeps, in playing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTrack {
    /// WebM rather than MP4.
    pub webm: bool,
    /// The piece every other is read with; `None` where the manifest lists none.
    pub init: Option<Piece>,
    /// The pieces made so far, oldest first.
    pub pieces: Vec<Piece>,
}

/// A live manifest as read at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Live {
    /// Still live (`type="dynamic"`); once static, the stream has ended and
    /// what it lists is all there will be.
    pub live: bool,
    /// How long to wait before reading it again, in seconds: its
    /// `minimumUpdatePeriod`, kept between half a second and half a minute
    /// (two seconds where it says none).
    pub every: f64,
    /// Its periods begun so far, in order.
    pub periods: Vec<LivePeriod>,
}

/// Whether a manifest's text is of a live stream (`type="dynamic"`).
pub fn is_live(text: &str) -> bool {
    parse(text).is_ok_and(|root| root.attr("type") == Some("dynamic"))
}

/// A live manifest's text, served at `url`, read `now` (seconds since 1970):
/// each begun period's picture and sound at `quality`, with the pieces made
/// so far. A period not yet begun is not listed; a manifest none of whose
/// periods can be read gives the first one's reason.
pub fn live(
    text: &str,
    url: &str,
    quality: Quality,
    container: Option<&str>,
    now: f64,
) -> Result<Live, Error> {
    const SHORTEST: f64 = 0.5;
    const LONGEST: f64 = 30.0;
    let root = parse(text)?;
    let dynamic = root.attr("type") == Some("dynamic");
    let mut periods = Vec::new();
    let mut first_error = None;
    for period in periods_of(&root, Some(now)) {
        if period
            .clock
            .is_some_and(|c| c.elapsed.is_some_and(|made| made <= 0.0))
        {
            continue;
        }
        match candidates(&root, &period, url) {
            Ok(all) => {
                let (video, audio) = choose(all, quality, container);
                periods.push(LivePeriod {
                    video: video.map(live_track),
                    audio: audio.map(live_track),
                });
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    if periods.is_empty() {
        if let Some(e) = first_error {
            return Err(e);
        }
    }
    let every = root
        .attr("minimumUpdatePeriod")
        .and_then(iso_seconds)
        .filter(|s| s.is_finite())
        .unwrap_or(2.0)
        .clamp(SHORTEST, LONGEST);
    Ok(Live {
        live: dynamic,
        every,
        periods,
    })
}

/// A taken representation as a live track: its initialisation piece apart.
fn live_track(taken: Candidate) -> LiveTrack {
    let mut pieces = taken.track.pieces;
    let init = (taken.init && !pieces.is_empty()).then(|| pieces.remove(0));
    LiveTrack {
        webm: taken.track.webm,
        init,
        pieces,
    }
}
