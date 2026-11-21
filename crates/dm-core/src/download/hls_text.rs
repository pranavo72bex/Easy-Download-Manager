// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An HLS stream's subtitles, carried inside the saved file where Options › Downloads › Embed
//! subtitles says so (gaps row Y20).
//!
//! A master playlist lists them as renditions of the chosen variant's `SUBTITLES` group
//! (`stream::subtitle_renditions`), each a media playlist of WebVTT segments. Each segment's
//! cues are timed from its own zero, which its `X-TIMESTAMP-MAP` header ties to the picture's
//! 90 kHz clock: a cue at `t` plays at `t - LOCAL + MPEGTS / 90` milliseconds of that clock
//! (RFC 8216, 3.5). The file they go in starts at a time of that clock of its own, which only
//! the writer knows once it has read the stream (`tsmux::to_mp4_with_text`,
//! `mp4mux::join_cut_placed`, `mkvmux::join_placed`): the cues are timed from it then, by
//! [`HlsTexts::timed`]. A transport stream's clock started afresh at a discontinuity is moved on
//! by the remux to follow what came before; it says how far (`tsmux::Clock`), and the segments
//! after the playlist's matching discontinuity are moved with it ([`HlsTexts::timed_on`]).

use futures_util::StreamExt;

use super::*;
use crate::subtitles::{Cue, Subtitle};

/// The most of one language's subtitles fetched, its playlist and segments together.
pub(super) const TEXT_MOST: usize = 16 * 1024 * 1024;
/// Ticks of the 90 kHz clock MPEG timestamps keep, a millisecond.
const TICKS_PER_MS: i64 = 90;
/// MPEG timestamps are 33 bits, and wrap.
const WRAP: i64 = 1 << 33;
/// The most segments of one language fetched at once.
pub(super) const AT_ONCE: usize = 8;

/// What a WebVTT segment's `X-TIMESTAMP-MAP` says: its cue time `local` (milliseconds) is the
/// picture's 90 kHz clock at `mpegts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TimestampMap {
    pub(super) mpegts: i64,
    pub(super) local: u64,
}

/// One WebVTT segment: its cues, timed as it writes them, and its map, where it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VttSegment {
    pub(super) map: Option<TimestampMap>,
    pub(super) cues: Vec<Cue>,
    /// How many discontinuities (`EXT-X-DISCONTINUITY`) its playlist puts before it: the
    /// stretch of the picture's clock its map is on (`tsmux::Clock`).
    pub(super) stretch: usize,
}

/// One language's subtitles, as fetched: its segments in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Rendition {
    pub(super) language: String,
    pub(super) segments: Vec<VttSegment>,
}

/// An HLS stream's subtitles, fetched, waiting to be timed against the file they go in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct HlsTexts {
    pub(super) renditions: Vec<Rendition>,
    /// How much of the stream (milliseconds) plays before the file's first segment: a part
    /// asked for starts there. A segment with no map is timed from the stream's start.
    pub(super) skipped: u64,
}

impl HlsTexts {
    /// Nothing to carry.
    pub(super) fn is_empty(&self) -> bool {
        self.renditions.is_empty()
    }

    /// The same, for a file whose first segment starts `seconds` into the stream.
    pub(super) fn skipping(self, seconds: f64) -> HlsTexts {
        let millis = if seconds.is_finite() && seconds > 0.0 {
            (seconds * 1000.0).round() as u64
        } else {
            0
        };
        HlsTexts {
            skipped: millis,
            ..self
        }
    }

    /// Each language's cues timed for a file that starts at `zero` on the picture's 90 kHz
    /// clock; a language left with no cue is left out.
    pub(super) fn timed(&self, zero: i64) -> Vec<Subtitle> {
        self.timed_on(crate::tsmux::Clock {
            zero,
            moved: Vec::new(),
        })
    }

    /// [`HlsTexts::timed`], for an MP4 a transport stream's remux makes, which says how far it
    /// moved the picture's clock at each discontinuity: a segment after its playlist's k-th
    /// discontinuity is moved as the picture's stretch after its k-th was. Where the two count
    /// their stretches apart (one the other does not show), or a part was asked for (whose
    /// first segment is not the stream's), none is moved, as before.
    pub(super) fn timed_on(&self, clock: crate::tsmux::Clock) -> Vec<Subtitle> {
        let zero = clock.zero;
        let skipped = i64::try_from(self.skipped).unwrap_or(i64::MAX);
        let start = zero.saturating_sub(skipped.saturating_mul(TICKS_PER_MS));
        let mut out = Vec::new();
        for rendition in &self.renditions {
            let stretches = rendition.segments.iter().map(|s| s.stretch).max();
            let moved: &[i64] = match stretches {
                Some(last) if self.skipped == 0 && clock.moved.len() == last + 1 => &clock.moved,
                _ => &[],
            };
            let cues = timed_across(&rendition.segments, start, zero, moved);
            if !cues.is_empty() {
                out.push(Subtitle {
                    language: rendition.language.clone(),
                    cues,
                });
            }
        }
        out
    }
}

/// Subtitles to carry inside a joined file.
pub(super) enum Texts {
    /// Timed already, on the file's own timeline (a DASH manifest's WebVTT, no piece mapped).
    Timed(Vec<Subtitle>),
    /// An HLS stream's, timed once the writer knows when the file starts.
    Hls(HlsTexts),
}

impl Texts {
    /// An HLS stream's subtitles; none at all where it has none, which is what was written
    /// before they were carried.
    pub(super) fn of_hls(texts: HlsTexts) -> Texts {
        if texts.is_empty() {
            Texts::Timed(Vec::new())
        } else {
            Texts::Hls(texts)
        }
    }
}

/// `ticks` brought within half a wrap of 0: the distance between two MPEG timestamps, one of
/// which may have wrapped past the 33 bits the other has not.
fn near(ticks: i64) -> i64 {
    let ticks = ticks.rem_euclid(WRAP);
    if ticks >= WRAP / 2 {
        ticks - WRAP
    } else {
        ticks
    }
}

/// The cues of `segments`, in order, timed in milliseconds from `zero`, the time the file
/// starts on the picture's 90 kHz clock. A segment's map ties its cue times to that clock; one
/// with none is taken as starting at `start`, the stream's own start (LOCAL 0 at MPEGTS
/// `start`). A cue that ends before the file starts is dropped, one that starts before it
/// starts with it; a cue repeated by the next segment (one that runs across both) is kept once.
pub(super) fn timed(segments: &[VttSegment], start: i64, zero: i64) -> Vec<Cue> {
    timed_across(segments, start, zero, &[])
}

/// [`timed`], a segment's map read on the picture's clock as moved at the discontinuity before
/// it: by `moved[k]` ticks after its playlist's k-th (`tsmux::Clock::moved`). A segment
/// `moved` does not name, or with no map, is not moved.
pub(super) fn timed_across(
    segments: &[VttSegment],
    start: i64,
    zero: i64,
    moved: &[i64],
) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for segment in segments {
        let (map, by) = match segment.map {
            Some(map) => (map, moved.get(segment.stretch).copied().unwrap_or(0)),
            None => (
                TimestampMap {
                    mpegts: start,
                    local: 0,
                },
                0,
            ),
        };
        let local = i64::try_from(map.local).unwrap_or(i64::MAX);
        let shift = near(map.mpegts.saturating_add(by).saturating_sub(zero))
            .div_euclid(TICKS_PER_MS)
            .saturating_sub(local);
        let at = |t: u64| i64::try_from(t).unwrap_or(i64::MAX).saturating_add(shift);
        for cue in &segment.cues {
            let (from, to) = (at(cue.start).max(0), at(cue.end));
            if to <= from {
                continue;
            }
            let placed = Cue {
                start: u64::try_from(from).unwrap_or(0),
                end: u64::try_from(to).unwrap_or(0),
                text: cue.text.clone(),
            };
            if seen.insert((placed.start, placed.end, placed.text.clone())) {
                out.push(placed);
            }
        }
    }
    out.sort_by_key(|c| c.start);
    out
}

/// What a WebVTT segment's header says in `X-TIMESTAMP-MAP=MPEGTS:<ticks>,LOCAL:<cue time>`
/// (either order). `None` where it has none, or one that cannot be read.
pub(super) fn timestamp_map(vtt: &str) -> Option<TimestampMap> {
    let text = vtt.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    // The header: what comes before the first blank line.
    let header = text.split("\n\n").next()?;
    let line = header
        .lines()
        .find_map(|l| l.trim().strip_prefix("X-TIMESTAMP-MAP="))?;
    let (mut mpegts, mut local) = (None, None);
    for pair in line.split(',') {
        let (key, value) = pair.split_once(':')?;
        match key.trim().to_ascii_uppercase().as_str() {
            "MPEGTS" => mpegts = value.trim().parse::<i64>().ok(),
            "LOCAL" => local = crate::subtitles::timestamp(value),
            _ => {}
        }
    }
    Some(TimestampMap {
        mpegts: mpegts?,
        local: local?,
    })
}

/// A WebVTT segment's cues and map, on the first stretch of its stream's clock.
pub(super) fn segment_of(vtt: &str) -> VttSegment {
    VttSegment {
        map: timestamp_map(vtt),
        cues: crate::subtitles::parse_vtt(vtt),
        stretch: 0,
    }
}

/// For each segment a media playlist lists, how many discontinuities (`EXT-X-DISCONTINUITY`)
/// come before it.
pub(super) fn stretches_of(playlist: &str) -> Vec<usize> {
    let mut seen = 0;
    let mut out = Vec::new();
    for line in playlist.lines().map(str::trim) {
        if line == "#EXT-X-DISCONTINUITY" {
            seen += 1;
        } else if !line.is_empty() && !line.starts_with('#') {
            out.push(seen);
        }
    }
    out
}

/// The subtitle renditions `listed`, fetched and read. One that cannot be had, is larger than
/// subtitles are (16 MiB), is not WebVTT in plain segments (text in MP4 pieces, or segments
/// locked by a key) or holds no cue is passed over: subtitles are not worth the video. A stop
/// is a stop.
pub(super) async fn fetch(
    client: &reqwest::Client,
    listed: &[stream::SubtitleRendition],
    options: &Options,
) -> Result<HlsTexts, Error> {
    let mut renditions = Vec::new();
    for track in listed {
        match rendition(client, track, options).await {
            Ok(Some(read)) => renditions.push(read),
            Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
            Ok(None) | Err(_) => {}
        }
    }
    Ok(HlsTexts {
        renditions,
        skipped: 0,
    })
}

/// One subtitle rendition fetched: its media playlist, then its segments, a few at a time.
/// `None` where it is passed over.
async fn rendition(
    client: &reqwest::Client,
    track: &stream::SubtitleRendition,
    options: &Options,
) -> Result<Option<Rendition>, Error> {
    let got = fetch_capped(client, &track.url, TEXT_MOST, options).await?;
    let text = got.text();
    let read = |segments: Vec<VttSegment>| {
        if segments.iter().all(|s| s.cues.is_empty()) {
            return None;
        }
        Some(Rendition {
            language: track.language.clone(),
            segments,
        })
    };
    // A rendition that is one WebVTT file rather than a playlist of them.
    if text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with("WEBVTT")
    {
        return Ok(read(vec![segment_of(&text)]));
    }
    if !text.contains("#EXTM3U") {
        return Ok(None);
    }
    let playlist = stream::media_playlist(&text, &got.landed, &track.url, Quality::Best)?;
    if playlist.parts.iter().any(|p| p.init || p.key.is_some()) {
        return Ok(None);
    }
    // Which stretch of the clock each segment is on, where every segment is counted.
    let stretches = Some(stretches_of(&text)).filter(|s| s.len() == playlist.parts.len());
    let mut total = got.body.len();
    // Owned addresses, so each fetch's future borrows nothing of the closure's argument.
    let asked: Vec<(String, Option<(u64, u64)>)> = playlist
        .parts
        .iter()
        .map(|part| (part.url.clone(), part.range))
        .collect();
    let mut pieces = futures_util::stream::iter(asked)
        .map(|(url, range)| async move { piece(client, &url, range, options).await })
        .buffered(options.connections.clamp(1, AT_ONCE));
    let mut segments = Vec::new();
    while let Some(body) = pieces.next().await {
        let body = body?;
        total = total.saturating_add(body.len());
        if total > TEXT_MOST {
            return Ok(None);
        }
        let stretch = stretches.as_ref().and_then(|s| s.get(segments.len()));
        segments.push(VttSegment {
            stretch: stretch.copied().unwrap_or(0),
            ..segment_of(&String::from_utf8_lossy(&body))
        });
    }
    Ok(read(segments))
}

/// One WebVTT segment's bytes: of `url`, or the bytes `range` of it (first and last).
///
/// Its future is boxed and named `Send`: made in a stream's closure (`buffered`), an async
/// call taking references there leaves rustc unable to prove a download's spawned task `Send`
/// ("implementation of `Send` is not general enough").
pub(super) fn piece<'a>(
    client: &'a reqwest::Client,
    url: &'a str,
    range: Option<(u64, u64)>,
    options: &'a Options,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, Error>> + Send + 'a>> {
    Box::pin(async move {
        match range {
            Some(_) => {
                retry::run_up_to(&options.control, options.tries(), || {
                    hls::fetch_segment_range(client, url, range, options)
                })
                .await
            }
            None => Ok(fetch_capped(client, url, TEXT_MOST, options).await?.body),
        }
    })
}

#[cfg(test)]
mod tests;
