// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The picture's codec when H.264 is preferred (gaps row Y8c).

#![allow(clippy::unwrap_used)]

use super::choosing::{choose_named, choose_with};
use super::*;

/// A video as YouTube's player lists it: AV1 at 2160p, VP9 at 1440p, H.264 at 1080p and
/// 720p, and its sound.
fn formats(with_h264: bool) -> Vec<Format> {
    let mut list = vec![
        r#"{"itag":401,"mimeType":"video/mp4; codecs=\"av01.0.12M.08\"","width":3840,"height":2160,"bitrate":9,"url":"https://r1.example/401"}"#,
        r#"{"itag":271,"mimeType":"video/webm; codecs=\"vp9\"","width":2560,"height":1440,"bitrate":8,"url":"https://r1.example/271"}"#,
        r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":1,"url":"https://r1.example/140"}"#,
    ];
    if with_h264 {
        list.push(r#"{"itag":137,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1920,"height":1080,"bitrate":5,"url":"https://r1.example/137"}"#);
        list.push(r#"{"itag":136,"mimeType":"video/mp4; codecs=\"avc1.4d401f\"","width":1280,"height":720,"bitrate":3,"url":"https://r1.example/136"}"#);
    }
    list.iter()
        .filter_map(|json| Format::of(&serde_json::from_str(json).unwrap()))
        .collect()
}

fn itags(chosen: Option<Vec<Format>>) -> Option<Vec<u64>> {
    chosen.map(|c| c.iter().map(|f| f.itag).collect())
}

#[test]
fn with_h264_preferred_the_best_download_is_the_tallest_h264_picture() {
    let video = formats(true);
    // Off: the tallest picture, AV1 at 2160p.
    assert_eq!(
        itags(choose_with(&video, Quality::Best, None, None, false)),
        Some(vec![401, 140])
    );
    // On: the tallest H.264, 1080p; within 720p, the 720p one.
    assert_eq!(
        itags(choose_with(&video, Quality::Best, None, None, true)),
        Some(vec![137, 140])
    );
    assert_eq!(
        itags(choose_with(&video, Quality::UpTo(720), None, None, true)),
        Some(vec![136, 140])
    );
    // A video with no H.264 at all: still its tallest picture.
    assert_eq!(
        itags(choose_with(
            &formats(false),
            Quality::Best,
            None,
            None,
            true
        )),
        Some(vec![401, 140])
    );
    // The switch is off for a new EDM; a line picked by its format number is taken as named.
    assert!(!crate::prefer_h264::wanted());
    assert_eq!(
        itags(choose_named(&video, Some(401), Quality::Best, None, None)),
        Some(vec![401, 140])
    );
}
