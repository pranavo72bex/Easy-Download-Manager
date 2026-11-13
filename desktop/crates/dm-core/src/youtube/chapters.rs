// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a video says of itself beyond its streams, written into the file (gaps row Y11): its
//! picture, the widest the player names, the chapters its description lists, and the day it
//! was published, which its watch page says.

use serde_json::Value;

/// The widest picture the player names for the video (`videoDetails.thumbnail.thumbnails`).
pub(super) fn largest_thumbnail(player: &Value) -> Option<String> {
    player
        .pointer("/videoDetails/thumbnail/thumbnails")?
        .as_array()?
        .iter()
        .filter_map(|t| Some((t.get("width")?.as_u64()?, t.get("url")?.as_str()?)))
        .max_by_key(|(width, _)| *width)
        .map(|(_, url)| url.to_string())
}

/// The chapters a description lists, as YouTube itself reads them: lines that begin with a
/// time (`0:00 Intro`, `1:02:03 - The end`), the first at 0:00, at least three, each later
/// than the one before; each its start in seconds and its name. None where the list is not
/// one by those rules.
pub(super) fn chapters_in(description: &str) -> Vec<(u64, String)> {
    fn seconds(clock: &str) -> Option<u64> {
        let parts: Vec<u64> = clock
            .split(':')
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        match parts.as_slice() {
            [m, s] if *s < 60 => Some(m * 60 + s),
            [h, m, s] if *m < 60 && *s < 60 => Some(h * 3600 + m * 60 + s),
            _ => None,
        }
    }
    let mut chapters: Vec<(u64, String)> = Vec::new();
    for line in description.lines() {
        let line = line.trim();
        let Some((clock, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Some(start) = seconds(clock.trim_start_matches('(').trim_end_matches(')')) else {
            continue;
        };
        let name = rest.trim().trim_start_matches(['-', '–', '—', ':', '|']).trim();
        if name.is_empty() {
            continue;
        }
        if chapters.last().is_some_and(|(last, _)| start <= *last) {
            return Vec::new();
        }
        chapters.push((start, name.to_string()));
    }
    if chapters.len() < 3 || chapters.first().is_none_or(|(start, _)| *start != 0) {
        return Vec::new();
    }
    chapters
}

/// The day the video was published, as YouTube's metadata writes it (`YYYYMMDD`), from the
/// watch page's `"publishDate":"2005-04-23…"`; `None` where the page says no such day.
pub(super) fn publish_date(page: &str) -> Option<String> {
    const MARK: &str = "\"publishDate\":\"";
    let at = page.find(MARK)? + MARK.len();
    let day = page.get(at..at + 10)?;
    let parts: Vec<&str> = day.split('-').collect();
    match parts.as_slice() {
        [y, m, d]
            if y.len() == 4
                && m.len() == 2
                && d.len() == 2
                && day.chars().all(|c| c.is_ascii_digit() || c == '-') =>
        {
            Some(format!("{y}{m}{d}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A description's chapter list is read as YouTube reads one; a list that breaks its rules
    /// (not from 0:00, fewer than three, going back) is none (gaps row Y11).
    #[test]
    fn a_descriptions_chapters_are_read_by_youtubes_rules() {
        let description = "A talk.\n\n0:00 Intro\n1:05 - The zoo\n(1:02:03) Élephants\nThanks!";
        assert_eq!(
            chapters_in(description),
            vec![
                (0, "Intro".to_string()),
                (65, "The zoo".to_string()),
                (3723, "Élephants".to_string())
            ]
        );
        assert!(chapters_in("0:10 a\n0:20 b\n0:30 c").is_empty(), "not from 0:00");
        assert!(chapters_in("0:00 a\n0:20 b").is_empty(), "two");
        assert!(chapters_in("0:00 a\n0:20 b\n0:10 c").is_empty(), "going back");
        assert!(chapters_in("no list here").is_empty());
        let player = json!({"videoDetails": {"thumbnail": {"thumbnails": [
            {"url": "https://i.ytimg.com/vi/x/default.jpg", "width": 120},
            {"url": "https://i.ytimg.com/vi/x/maxresdefault.jpg", "width": 1280},
            {"url": "https://i.ytimg.com/vi/x/hq.jpg", "width": 480}
        ]}}});
        assert_eq!(
            largest_thumbnail(&player).as_deref(),
            Some("https://i.ytimg.com/vi/x/maxresdefault.jpg")
        );
    }

    #[test]
    fn the_watch_pages_day_is_the_videos_date() {
        let page = r#"..."microformat":{"playerMicroformatRenderer":{"publishDate":"2005-04-23T20:31:52-07:00","uploadDate":"x"}}"#;
        assert_eq!(publish_date(page).as_deref(), Some("20050423"));
        assert_eq!(publish_date(r#""publishDate":"soon""#), None);
        assert_eq!(publish_date("no day"), None);
    }
}
