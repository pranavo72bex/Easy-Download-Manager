// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#[cfg(unix)]
/// A stream whose renditions name no codecs, as a player
/// reads an HLS master playlist without CODECS, still lists its heights.
#[test]
fn renditions_without_codecs_are_listed_by_height() {
    let described = serde_json::json!({ "formats": [
        { "format_id": "hls-360", "ext": "mp4", "height": 360, "width": 640, "vcodec": null, "acodec": null },
        { "format_id": "hls-720", "ext": "mp4", "height": 720, "width": 1280 },
        { "format_id": "a", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2" },
    ]})
    .to_string();
    let heights: Vec<String> = choices(&described).into_iter().map(|c| c.quality).collect();
    assert_eq!(heights, ["720/mp4", "360/mp4", "audio/m4a"]);
}

#[tokio::test]
async fn a_cancelled_runs_staging_is_removed_and_a_missing_one_is_no_wait() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("parts");
    std::fs::create_dir_all(staging.join("deeper")).unwrap();
    std::fs::write(staging.join("deeper/a.part"), b"x").unwrap();
    super::remove_staging(&staging).await;
    assert!(!staging.exists());
    let at = std::time::Instant::now();
    super::remove_staging(&staging).await;
    assert!(at.elapsed() < std::time::Duration::from_millis(150));
}

use super::*;
use crate::stream::Quality;

#[test]
fn the_list_offers_mp4_where_there_is_one_as_youtube_lists_webm_bigger() {
    // YouTube's real shape: VP9 WebM bigger than H.264 MP4 at a height,
    // Opus sound at a higher bitrate than M4A, and 4K only in WebM.
    let described = serde_json::json!({"formats": [
        {"ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 129.0, "filesize": 1_000},
        {"ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 140.0, "filesize": 1_100},
        {"ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1920, "height": 1080, "filesize": 9_000},
        {"ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 1920, "height": 1080, "filesize": 12_000},
        {"ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 3840, "height": 2160, "filesize": 40_000},
    ]})
    .to_string();
    let list = choices(&described);
    let labels: Vec<&str> = list.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["4K WEBM", "1080p MP4", "M4A audio"]);
    assert_eq!(list[1].quality, "1080/mp4");
    assert_eq!(list[1].size, Some(10_000), "joined with the M4A sound");
}

#[test]
fn a_pages_formats_become_one_choice_per_height_and_the_sound_alone() {
    let described = serde_json::json!({"formats": [
        {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 129.5, "filesize": 1_000},
        {"format_id": "249", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 50.0, "filesize": 400},
        {"format_id": "18", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a", "width": 640, "height": 360, "filesize": 5_000},
        {"format_id": "136", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1280, "height": 720, "filesize": 9_000},
        {"format_id": "247", "ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 1280, "height": 720, "filesize_approx": 7_000},
        {"format_id": "sb0", "ext": "mhtml", "vcodec": "none", "acodec": "none", "height": 90},
    ]})
    .to_string();
    let list = choices(&described);
    let labels: Vec<&str> = list.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["720p MP4", "360p MP4", "M4A audio"]);
    // The picture's format number, for the download to take that very one (gaps row Y9).
    assert_eq!(list[0].quality, "720/mp4/f136");
    assert_eq!(Quality::format_of(&list[0].quality), Some(136));
    assert_eq!(
        list[0].size,
        Some(10_000),
        "the picture and the sound it is joined with"
    );
    assert_eq!(list[1].size, Some(5_000), "already carries its sound");
    assert_eq!(list[2].quality, "audio/m4a");
    assert_eq!(Quality::named(&list[0].quality), Quality::UpTo(720));
    assert_eq!(Quality::named(&list[2].quality), Quality::Audio);
    assert_eq!(
        Quality::container_of(&list[0].quality).as_deref(),
        Some("mp4")
    );
    assert_eq!(Quality::container_of("720"), None);
    assert_eq!(Quality::container_of("720/../x"), None, "only a plain word");
    assert!(choices("not json").is_empty());
    assert!(choices("{}").is_empty());
}

/// A height with a picture above 30 frames a second and one at or below
/// is two lines, as reference A lists "720p HD 60 fps" and "720p HD"; a
/// height with one frame rate is one line, as before (gaps row 72d).
#[test]
fn a_height_at_two_frame_rates_is_two_lines() {
    let described = serde_json::json!({"formats": [
        {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 129.5, "filesize": 1_000},
        {"format_id": "136", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1280, "height": 720, "fps": 30, "filesize": 9_000},
        {"format_id": "298", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1280, "height": 720, "fps": 60, "filesize": 14_000},
        {"format_id": "135", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 854, "height": 480, "fps": 30, "filesize": 4_000},
    ]})
    .to_string();
    let list = choices(&described);
    let lines: Vec<(&str, &str)> = list
        .iter()
        .map(|c| (c.label.as_str(), c.quality.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            ("720p MP4 60 fps", "720/mp4/60/f298"),
            ("720p MP4", "720/mp4/30/f136"),
            ("480p MP4", "480/mp4/f135"),
            ("M4A audio", "audio/m4a"),
        ]
    );
    assert_eq!(list[0].size, Some(15_000));
    // The format number after the frame rate leaves the rest of the word as it was read.
    for (word, height, rate, format) in [
        ("720/mp4/60/f298", 720, Some(60), Some(298)),
        ("720/mp4/30/f136", 720, Some(30), Some(136)),
        ("480/mp4/f135", 480, None, Some(135)),
        ("480/mp4", 480, None, None),
        ("audio/m4a", 0, None, None),
    ] {
        if height > 0 {
            assert_eq!(Quality::named(word), Quality::UpTo(height), "{word}");
        }
        assert_eq!(Quality::frame_rate_of(word), rate, "{word}");
        assert_eq!(Quality::format_of(word), format, "{word}");
        assert!(Quality::container_of(word).is_some(), "{word}");
    }
    assert_eq!(Quality::without_format("720/mp4/60/f298"), "720/mp4/60");
    assert_eq!(Quality::without_format("480/mp4/f135"), "480/mp4");
    assert_eq!(Quality::without_format("480/mp4"), "480/mp4");
    assert_eq!(Quality::without_format("audio/flac"), "audio/flac");
    // A container's place, not a format number's.
    assert_eq!(Quality::format_of("720/f136"), None);
    assert_eq!(Quality::format_of("720/mp4/fx"), None);
    assert_eq!(Quality::named("720/mp4/60"), Quality::UpTo(720));
    assert_eq!(Quality::container_of("720/mp4/60").as_deref(), Some("mp4"));
    assert_eq!(Quality::frame_rate_of("720/mp4/60"), Some(60));
    assert_eq!(Quality::frame_rate_of("720/mp4"), None);
}
