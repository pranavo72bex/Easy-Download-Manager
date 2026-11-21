// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

// ---- is_meaningless ----

#[test]
fn gifpaiwya_is_meaningless() {
    assert!(is_meaningless("GiFPAi8WYAEgiBu.mp4"));
}

#[test]
fn long_base64ish_hash_is_meaningless() {
    assert!(is_meaningless("o4DPxfQfIIk3KMBHYxVIEGxCCEuyk9deAAzL6j.mp4"));
}

#[test]
fn generic_video_is_meaningless() {
    assert!(is_meaningless("video.mp4"));
}

#[test]
fn all_digits_is_meaningless() {
    assert!(is_meaningless("123456789.mp4"));
}

#[test]
fn hex_hash_is_meaningless() {
    assert!(is_meaningless("3f2a9c1e7b8d4a6f9c0e1d2b3a4c5d6e.mp4"));
}

#[test]
fn uuid_is_meaningless() {
    assert!(is_meaningless("0b6d3c1a-7e2f-4a8b-9c1d-2e3f4a5b6c7d.mp4"));
}

#[test]
fn setup_installer_is_not_meaningless() {
    assert!(!is_meaningless("setup-installer.exe"));
}

#[test]
fn big_buck_bunny_is_not_meaningless() {
    assert!(!is_meaningless("BigBuckBunny720p.mp4"));
}

#[test]
fn report_2026_is_not_meaningless() {
    assert!(!is_meaningless("Report2026_Final.pdf"));
}

#[test]
fn holiday_photos_is_not_meaningless() {
    assert!(!is_meaningless("holiday photos.zip"));
}

#[test]
fn nasa_is_not_meaningless() {
    assert!(!is_meaningless("NASA.mp4"));
}

// ---- title_name ----

#[test]
fn title_with_colon_and_special_chars() {
    let name = title_name(r#"NASA on X: "Everybody, wave!" / Moon"#, "mp4").unwrap();
    assert_eq!(name, "NASA on X - Everybody, wave! Moon.mp4");
}

#[test]
fn con_is_reserved() {
    assert!(title_name("CON", "mp4").is_none());
}

#[test]
fn long_title_cut_at_word_boundary() {
    // Build a title that is slightly over 120 characters.
    let title = "word ".repeat(30); // 150 chars
    let name = title_name(title.trim(), "mp4").unwrap();
    assert!(name.len() <= 125, "too long: {}", name.len()); // 120 + ".mp4" = 124
}

#[test]
fn empty_title_gives_none() {
    assert!(title_name("", "mp4").is_none());
    assert!(title_name("   ", "mp4").is_none());
}

// ---- choose ----

#[test]
fn header_name_beats_everything() {
    let name = choose(
        Some("report.pdf"),
        "https://cdn.example.com/3f2a9c1e7b8d4a6f.pdf",
        Some("My Report"),
        "pdf",
    );
    assert_eq!(name, "report.pdf");
}

#[test]
fn hash_url_name_with_title_takes_the_title() {
    let name = choose(
        None,
        "https://x.com/GiFPAi8WYAEgiBu.mp4",
        Some("NASA on X: \"Everybody, wave!\""),
        "mp4",
    );
    assert!(name.starts_with("NASA on X"), "{name}");
}

#[test]
fn real_url_name_beats_the_title() {
    let name = choose(
        None,
        "https://example.com/BigBuckBunny720p.mp4",
        Some("My video"),
        "mp4",
    );
    assert_eq!(name, "BigBuckBunny720p.mp4");
}

#[test]
fn no_title_keeps_url_name() {
    let name = choose(
        None,
        "https://cdn.example.com/3f2a9c1e7b8d4a6f.mp4",
        None,
        "mp4",
    );
    assert_eq!(name, "3f2a9c1e7b8d4a6f.mp4");
}
