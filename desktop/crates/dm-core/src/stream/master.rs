// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A master playlist read: its variants, the sound and subtitle renditions they name, and the
//! variant picked for a quality.

use super::*;

/// The playlist of the sound a rendition plays with, where its audio group
/// keeps the sound apart from the picture: the group's track in the sound
/// language asked for (Options, gaps row Y10) where one speaks it, else its
/// default track, else its first, joined onto `base`. `None` for sound
/// carried in the picture's own segments (a track with no address).
pub fn audio_playlist(master: &str, group: &str, base: &str) -> Option<String> {
    let want = crate::sound_language::wanted();
    join(base, &audio_uri(master, group, want.as_deref())?).ok()
}

/// Whether an `#EXT-X-MEDIA` track describes the picture for those who cannot see it.
fn describes_video(track: &str) -> bool {
    attribute(track, "CHARACTERISTICS")
        .is_some_and(|c| c.contains("public.accessibility.describes-video"))
}

/// The address a rendition's separate sound is listed under, as written. With a language
/// `want`ed, the group's track speaking it (`LANGUAGE`) comes first: a plain one before one
/// describing the picture, the default among them before the rest.
pub(super) fn audio_uri(master: &str, group: &str, want: Option<&str>) -> Option<String> {
    let rows = lines(master);
    let tracks: Vec<&str> = rows
        .iter()
        .copied()
        .filter(|l| {
            let u = l.to_ascii_uppercase();
            u.starts_with("#EXT-X-MEDIA")
                && u.contains("TYPE=AUDIO")
                && attribute(l, "GROUP-ID").as_deref() == Some(group)
        })
        .collect();
    let default = |l: &&str| attribute(l, "DEFAULT").is_some_and(|d| d.eq_ignore_ascii_case("YES"));
    let spoken = want.and_then(|want| {
        tracks
            .iter()
            .filter(|l| {
                attribute(l, "LANGUAGE")
                    .is_some_and(|lang| crate::sound_language::speaks(&lang, want))
            })
            // The first of the best: `max_by_key` keeps the last of equals, so the order is
            // turned round first.
            .rev()
            .max_by_key(|l| (!describes_video(l), default(l)))
    });
    let chosen = spoken
        .or_else(|| tracks.iter().find(|l| default(l)))
        .or(tracks.first())?;
    attribute(chosen, "URI")
}

/// The subtitle renditions of a master playlist's group `group` (a variant's
/// `SUBTITLES=`), one a language: the first of each, in the order listed,
/// one marked `FORCED=YES` (only what the picture does not say) passed over
/// where a plain one of its language is listed. Each address is joined onto
/// `base`; one with no address, or one that cannot be joined, is left out.
pub fn subtitle_renditions(master: &str, group: &str, base: &str) -> Vec<SubtitleRendition> {
    let is_forced = |l: &str| attribute(l, "FORCED").is_some_and(|f| f.eq_ignore_ascii_case("YES"));
    let mut found: Vec<(SubtitleRendition, bool)> = Vec::new();
    for row in lines(master) {
        let upper = row.to_ascii_uppercase();
        if !upper.starts_with("#EXT-X-MEDIA:")
            || attribute(row, "TYPE").is_none_or(|t| !t.eq_ignore_ascii_case("SUBTITLES"))
            || attribute(row, "GROUP-ID").as_deref() != Some(group)
        {
            continue;
        }
        let Some(url) = attribute(row, "URI").and_then(|uri| join(base, &uri).ok()) else {
            continue;
        };
        let language = attribute(row, "LANGUAGE")
            .map(|l| l.trim().to_string())
            .filter(|l| {
                !l.is_empty()
                    && l.len() <= 20
                    && l.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .unwrap_or_else(|| "und".to_string());
        let forced = is_forced(row);
        let rendition = SubtitleRendition { language, url };
        let listed = found
            .iter()
            .position(|(r, _)| r.language.eq_ignore_ascii_case(&rendition.language));
        match listed {
            // A plain one in place of the forced one listed first.
            Some(i) => {
                if let Some(slot) = found.get_mut(i).filter(|slot| slot.1 && !forced) {
                    *slot = (rendition, false);
                }
            }
            None => found.push((rendition, forced)),
        }
    }
    found.into_iter().map(|(r, _)| r).collect()
}

/// Every rendition a master playlist lists, in the order it lists them.
pub fn variants(text: &str, base: &str) -> Vec<Variant> {
    let rows = lines(text);
    let mut out = Vec::new();

    for (i, row) in rows.iter().enumerate() {
        if !row.to_ascii_uppercase().starts_with("#EXT-X-STREAM-INF") {
            continue;
        }
        let Some(uri) = rows.get(i + 1) else { continue };
        if uri.starts_with('#') {
            continue;
        }
        let Ok(url) = join(base, uri) else { continue };

        out.push(Variant {
            url,
            height: attribute(row, "RESOLUTION")
                .and_then(|r| {
                    let (w, h) = r.split_once(['x', 'X'])?;
                    let (w, h) = (w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?);
                    Some(if w > 0 && w < h { w } else { h })
                })
                .unwrap_or(0),
            bandwidth: attribute(row, "BANDWIDTH")
                .and_then(|b| b.parse().ok())
                .unwrap_or(0),
            audio: attribute(row, "AUDIO"),
            subtitles: attribute(row, "SUBTITLES"),
        });
    }
    out
}

/// Chooses a rendition. `None` when the playlist offered none.
pub fn pick(offered: Vec<Variant>, quality: Quality) -> Option<Variant> {
    if offered.is_empty() {
        return None;
    }

    /* One element out of a list, so a pick rather than a sort: each arm used
     * to build a filtered copy, sort the whole of it, and take the front.
     *
     * `min_by_key(Reverse(..))` rather than `max_by_key`, because the two
     * break ties in opposite directions: `max_by_key` keeps the *last* of
     * equal candidates, and a playlist offering the same height at two
     * bitrates lists the cheaper one first. Under a cap especially, that is
     * the one to take. */
    match quality {
        // The smallest stream carrying no picture, else the smallest overall.
        Quality::Audio => {
            let silent = offered.iter().any(|v| v.height == 0);
            offered
                .into_iter()
                .filter(|v| !silent || v.height == 0)
                .min_by_key(|v| v.bandwidth)
        }
        Quality::Best => offered
            .into_iter()
            .min_by_key(|v| std::cmp::Reverse((v.height, v.bandwidth))),
        Quality::UpTo(target) => {
            if let Some(best) = offered
                .iter()
                .filter(|v| v.height > 0 && v.height <= target)
                .min_by_key(|v| std::cmp::Reverse(v.height))
            {
                return Some(best.clone());
            }
            /* Every rendition is taller than asked for, so the shortest of
             * them. A cap must never quietly fetch something larger than the
             * one the person capping it was trying to avoid. */
            offered
                .iter()
                .filter(|v| v.height > 0)
                .min_by_key(|v| v.height)
                .cloned()
                .or_else(|| offered.into_iter().next())
        }
    }
}
