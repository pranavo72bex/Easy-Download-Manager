// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A DASH manifest's WebVTT subtitles fetched and read (gaps row Y20): one file a language, or
//! its pieces joined, a cue two pieces both write kept once.
//!
//! A piece's cue times are on the period's timeline, which the joined picture starts at (the
//! MP4 writer starts it at its earliest first time, as the whole files of a manifest are timed),
//! unless the piece maps them to the media's 90 kHz clock as HLS's segments do
//! (`X-TIMESTAMP-MAP`): then every language is timed by the writer once it knows where the file
//! starts on that clock, as an HLS stream's are (`hls_text`), a piece with no map still from the
//! period's start.

use futures_util::StreamExt;

use super::*;
use crate::download::hls_text::{self, HlsTexts, Rendition, Texts, VttSegment};
use crate::subtitles::Subtitle;

/// The subtitles `plan` lists, fetched and read: timed already where no piece maps its cues
/// (`Texts::Timed`), else timed by the writer (`Texts::Hls`). One that cannot be had, is larger
/// than subtitles are (16 MiB a language) or holds no cue is passed over: subtitles are not
/// worth the video. A stop is a stop.
pub(super) async fn fetch(
    client: &reqwest::Client,
    plan: &crate::dash::Plan,
    options: &Options,
) -> Result<Texts, Error> {
    let mut renditions = Vec::new();
    for text in &plan.texts {
        match rendition(client, text, options).await {
            Ok(Some(read)) => renditions.push(read),
            Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
            Ok(None) | Err(_) => {}
        }
    }
    Ok(timed(renditions, plan, options))
}

/// `renditions` as `join_tracks` carries them: with no piece mapped, each language's cues as
/// they are, joined; else every one timed by the writer. A part asked for starts at the
/// picture's first piece that overlaps it, so a piece with no map is timed from the period's
/// start, that far before the file's.
fn timed(renditions: Vec<Rendition>, plan: &crate::dash::Plan, options: &Options) -> Texts {
    let mapped = renditions
        .iter()
        .any(|r| r.segments.iter().any(|s| s.map.is_some()));
    if mapped {
        let starts = match (options.span, &plan.video) {
            (Some(span), Some(video)) => {
                crate::dash::cut(video, span.from, span.to).map_or(0.0, |(_, starts, _)| starts)
            }
            _ => 0.0,
        };
        let texts = HlsTexts {
            renditions,
            skipped: 0,
        };
        return Texts::of_hls(texts.skipping(starts));
    }
    let joined = renditions
        .into_iter()
        .filter_map(|r| {
            let cues = hls_text::timed(&r.segments, 0, 0);
            (!cues.is_empty()).then_some(Subtitle {
                language: r.language,
                cues,
            })
        })
        .collect();
    Texts::Timed(joined)
}

/// One language's WebVTT, its file or pieces fetched a few at a time, each read. `None` where
/// it is passed over.
async fn rendition(
    client: &reqwest::Client,
    text: &crate::dash::Text,
    options: &Options,
) -> Result<Option<Rendition>, Error> {
    // Owned addresses, so each fetch's future borrows nothing of the closure's argument.
    let asked: Vec<(String, Option<(u64, u64)>)> = text
        .pieces
        .iter()
        .map(|p| (p.url.clone(), p.range))
        .collect();
    let mut pieces = futures_util::stream::iter(asked)
        .map(|(url, range)| async move { hls_text::piece(client, &url, range, options).await })
        .buffered(options.connections.clamp(1, hls_text::AT_ONCE));
    let mut total = 0usize;
    let mut segments: Vec<VttSegment> = Vec::new();
    while let Some(body) = pieces.next().await {
        let body = body?;
        total = total.saturating_add(body.len());
        if total > hls_text::TEXT_MOST {
            return Ok(None);
        }
        segments.push(hls_text::segment_of(&String::from_utf8_lossy(&body)));
    }
    if segments.iter().all(|s| s.cues.is_empty()) {
        return Ok(None);
    }
    Ok(Some(Rendition {
        language: text.language.clone(),
        segments,
    }))
}

#[cfg(test)]
mod tests;
