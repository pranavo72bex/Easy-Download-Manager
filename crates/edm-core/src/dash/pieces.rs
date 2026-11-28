// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A representation's pieces as its template or its list names them (`pieces_of`).

use super::*;

/// That a manifest's `what` is not one EDM can read.
pub(super) fn unreadable(what: &str) -> Error {
    Error::Internal(format!("a manifest's {what} is not one EDM can read"))
}

/// The pieces a `SegmentTemplate` names (`t`, the nearest one), and whether an
/// initialisation piece leads them.
pub(super) fn template_pieces(c: &Context, t: &Node) -> Result<(Vec<Piece>, bool), Error> {
    let id = c.rep.attr("id").unwrap_or_default();
    let bandwidth = c.rep.num("bandwidth").unwrap_or(0);
    let mut pieces = Vec::new();
    // A template's attributes may be split between levels: the timeline
    // on the set and the media address on the representation, say.
    let attr = |name: &str| {
        c.rep
            .child("SegmentTemplate")
            .and_then(|n| n.attr(name))
            .or_else(|| c.set.child("SegmentTemplate").and_then(|n| n.attr(name)))
            .or_else(|| c.period.child("SegmentTemplate").and_then(|n| n.attr(name)))
    };
    let timescale = attr("timescale")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(1);
    let start = attr("startNumber")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1);
    if let Some(init) = attr("initialization") {
        let url =
            fill(init, id, bandwidth, 0, 0).ok_or_else(|| unreadable("initialization template"))?;
        pieces.push(Piece {
            url: join(&c.base, &url)?,
            range: None,
            lock: None,
            millis: 0,
        });
    }
    let media = attr("media").ok_or_else(|| unreadable("media template"))?;
    let to_millis = |ticks: u64| (ticks as f64 * 1000.0 / timescale as f64).round() as u64;
    let at = |number: u64, time: u64, ticks: u64| -> Result<Piece, Error> {
        let url =
            fill(media, id, bandwidth, number, time).ok_or_else(|| unreadable("media template"))?;
        Ok(Piece {
            url: join(&c.base, &url)?,
            range: None,
            lock: None,
            millis: to_millis(ticks),
        })
    };
    let timeline = c
        .rep
        .child("SegmentTemplate")
        .and_then(|n| n.child("SegmentTimeline"))
        .or_else(|| {
            c.set
                .child("SegmentTemplate")
                .and_then(|n| n.child("SegmentTimeline"))
        })
        .or_else(|| t.child("SegmentTimeline"));
    if let Some(timeline) = timeline {
        // Times in the timeline are the media's, which begins at this offset into it.
        let offset = attr("presentationTimeOffset")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0) as f64;
        timeline_pieces(c, timeline, start, offset, timescale, &at, &mut pieces)?;
    } else {
        let d = attr("duration")
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|d| *d > 0.0)
            .ok_or_else(|| unreadable("segment duration"))?;
        // Numbered pieces of one length: all of the period's, or in a live stream the
        // ones the clock says are whole, within the window the server keeps.
        let (first, count) = match c.live {
            Some(clock) => {
                let elapsed = clock
                    .elapsed
                    .ok_or_else(|| unreadable("live start (availabilityStartTime)"))?;
                periods::live_window(elapsed, d / timescale as f64, clock.depth)
            }
            None => {
                let seconds = c.duration.ok_or_else(|| unreadable("duration"))?;
                let count = (seconds * timescale as f64 / d).ceil();
                if !(count.is_finite() && count > 0.0) {
                    return Err(unreadable("segment count"));
                }
                (0, count as u64)
            }
        };
        if count > MOST_PIECES as u64 {
            return Err(unreadable("segment count"));
        }
        for n in first..first + count {
            pieces.push(at(start + n, n * d as u64, d as u64)?);
        }
    }
    let init = attr("initialization").is_some();
    Ok((pieces, init))
}

/// A template's timeline (its `S` entries) as pieces, numbered from `start`, added to
/// `pieces`; `offset` is where in the media's times the period begins.
fn timeline_pieces(
    c: &Context,
    timeline: &Node,
    start: u64,
    offset: f64,
    timescale: u64,
    at: &dyn Fn(u64, u64, u64) -> Result<Piece, Error>,
    pieces: &mut Vec<Piece>,
) -> Result<(), Error> {
    let mut time = 0u64;
    let mut number = start;
    let entries: Vec<&Node> = timeline.children("S").collect();
    for (n, s) in entries.iter().enumerate() {
        if let Some(t0) = s.num("t") {
            time = t0;
        } else if n == 0 {
            time = 0;
        }
        let d = s
            .num("d")
            .filter(|d| *d > 0)
            .ok_or_else(|| unreadable("timeline"))?;
        let count = match s.attr("r").and_then(|r| r.trim().parse::<i64>().ok()) {
            Some(r) if r >= 0 => r as u64 + 1,
            // A negative repeat runs to the next entry's start; the last one to the
            // period's end, or in a live stream to what has been made so far (whole
            // pieces only: the one being made is not there yet).
            Some(_) => {
                let next = entries.get(n + 1).and_then(|e| e.num("t"));
                let (end, whole_only) = match (next, c.live) {
                    (Some(next), _) => (next as f64, false),
                    (None, Some(clock)) => (
                        offset
                            + clock.elapsed.ok_or_else(|| unreadable("timeline"))?
                                * timescale as f64,
                        true,
                    ),
                    (None, None) => (
                        offset
                            + c.duration.ok_or_else(|| unreadable("timeline"))? * timescale as f64,
                        false,
                    ),
                };
                let pieces = (end - time as f64) / d as f64;
                let pieces = if whole_only {
                    pieces.floor()
                } else {
                    pieces.ceil().max(1.0)
                };
                if !pieces.is_finite() || pieces > MOST_PIECES as f64 {
                    return Err(unreadable("timeline (far too many pieces)"));
                }
                pieces.max(0.0) as u64
            }
            None => 1,
        };
        for _ in 0..count {
            if pieces.len() >= MOST_PIECES {
                return Err(unreadable("timeline (far too many pieces)"));
            }
            pieces.push(at(number, time, d)?);
            time = time.saturating_add(d);
            number += 1;
        }
    }
    Ok(())
}

/// The pieces a `SegmentList` names, and whether an initialisation piece leads them.
pub(super) fn list_pieces(c: &Context, list: &Node) -> Result<(Vec<Piece>, bool), Error> {
    let mut pieces = Vec::new();
    let has_init = list.child("Initialization").is_some();
    if let Some(init) = list.child("Initialization") {
        if let Some(src) = init.attr("sourceURL") {
            pieces.push(Piece {
                url: join(&c.base, src)?,
                range: init.attr("range").and_then(byte_range),
                lock: None,
                millis: 0,
            });
        } else {
            pieces.push(Piece {
                url: c.base.clone(),
                range: init.attr("range").and_then(byte_range),
                lock: None,
                millis: 0,
            });
        }
    }
    // Each URL plays for the list's duration, in its own timescale.
    let each = list
        .attr("duration")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|d| *d > 0.0)
        .map(|d| {
            let scale = list
                .attr("timescale")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| *v > 0.0)
                .unwrap_or(1.0);
            (d * 1000.0 / scale).round() as u64
        })
        .unwrap_or(0);
    for s in list.children("SegmentURL") {
        if pieces.len() >= MOST_PIECES {
            return Err(unreadable("segment list (far too many pieces)"));
        }
        let url = match s.attr("media") {
            Some(m) => join(&c.base, m)?,
            None => c.base.clone(),
        };
        pieces.push(Piece {
            url,
            range: s.attr("mediaRange").and_then(byte_range),
            lock: None,
            millis: each,
        });
    }
    Ok((pieces, has_init))
}
