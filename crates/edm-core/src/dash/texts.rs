// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A manifest's WebVTT subtitles, listed to be put inside the file where Options › Downloads ›
//! Embed subtitles says so (gaps row Y20): one file a language (a `BaseURL`), or its pieces (a
//! `SegmentTemplate` or `SegmentList`), listed as the picture's are (`pieces_of`). The download
//! fetches and reads them (`download::dash`).

use super::*;

/// The most pieces one language's WebVTT is listed in: a two-hour film in pieces of a second
/// is 7,200. A template that would have EDM ask for more is passed over.
const MOST_TEXT_PIECES: usize = 20_000;

/// The subtitles of a manifest of one period that are WebVTT (`text/vtt`), the first of each
/// language. Text carried in MP4 pieces (`stpp`, `wvtt`) is not read; nor the text of a
/// manifest of several periods, whose times would each start again where the joined picture
/// does not.
pub(super) fn texts_of(root: &Node, url: &str) -> Vec<Text> {
    let periods = periods::periods_of(root, None);
    let [period] = periods.as_slice() else {
        return Vec::new();
    };
    let Ok(period_base) = base_of(url, root).and_then(|top| base_of(&top, period.node)) else {
        return Vec::new();
    };
    let mut out: Vec<Text> = Vec::new();
    for set in period.node.children("AdaptationSet") {
        if set.child("ContentProtection").is_some() {
            continue;
        }
        let Ok(set_base) = base_of(&period_base, set) else {
            continue;
        };
        for rep in set.children("Representation") {
            let Some(text) = text_of(root, period, set, rep, &set_base) else {
                continue;
            };
            if out.iter().all(|t| t.language != text.language) {
                out.push(text);
            }
        }
    }
    out
}

/// A representation's WebVTT, where it is that: its language, and the one file to fetch or
/// its pieces, in order. `None` for anything else, or pieces EDM cannot list.
fn text_of(
    root: &Node,
    period: &periods::PeriodAt<'_>,
    set: &Node,
    rep: &Node,
    set_base: &str,
) -> Option<Text> {
    let said = |name: &str| {
        rep.attr(name)
            .or_else(|| set.attr(name))
            .unwrap_or_default()
    };
    if !said("mimeType").eq_ignore_ascii_case("text/vtt")
        || rep.child("ContentProtection").is_some()
    {
        return None;
    }
    let c = Context {
        base: base_of(set_base, rep).ok()?,
        mpd: root,
        period: period.node,
        set,
        rep,
        duration: period.duration,
        live: None,
    };
    let pieced = c.inherited("SegmentTemplate").is_some() || c.inherited("SegmentList").is_some();
    let pieces = if pieced {
        pieces_of(&c).ok()?.0
    } else {
        let whole = rep
            .child("BaseURL")
            .or_else(|| set.child("BaseURL"))
            .is_some_and(|b| !b.text.trim().is_empty());
        if !whole {
            return None;
        }
        vec![Piece {
            url: c.base.clone(),
            range: None,
            lock: None,
            millis: 0,
        }]
    };
    if pieces.is_empty() || pieces.len() > MOST_TEXT_PIECES {
        return None;
    }
    let lang = said("lang").trim();
    let named = !lang.is_empty()
        && lang
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'));
    Some(Text {
        language: if named { lang } else { "und" }.to_string(),
        url: c.base,
        pieces,
    })
}
