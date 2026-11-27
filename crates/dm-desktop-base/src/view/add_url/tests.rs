// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn nothing_typed_is_asked_for_and_a_bad_link_is_said_to_be_one() {
    let settings = Settings::default();
    assert_eq!(
        decide("  ", false, "", "", &settings, &[]).next,
        Next::Says("Type or paste the address of what to download.")
    );
    assert!(matches!(
        decide("not a link", false, "", "", &settings, &[]).next,
        Next::Says(s) if s.starts_with("That isn’t a link dm can download")
    ));
}

#[test]
fn an_address_goes_to_download_details_and_is_remembered_without_its_secret() {
    let settings = Settings::default();
    let plain = decide("https://e.com/a.zip", false, "", "", &settings, &[]);
    assert_eq!(
        plain.next,
        Next::Details(vec!["https://e.com/a.zip".into()])
    );
    assert_eq!(plain.remember, Some(vec!["https://e.com/a.zip".into()]));
    let signed = decide(
        "https://e.com/a.zip?token=abc",
        false,
        "",
        "",
        &settings,
        &[],
    );
    assert_eq!(signed.remember, None, "a signed link is not remembered");
    let login = decide("https://e.com/a.zip", true, "me", "pw", &settings, &[]);
    assert_eq!(
        login.next,
        // Built here, as a sign-in in an address is never written out.
        Next::Details(vec![format!("https://me:{}@e.com/a.zip", "pw")])
    );
    assert_eq!(
        login.remember,
        Some(vec!["https://e.com/a.zip".into()]),
        "without its login"
    );
}

/// gaps row Y7: a channel's address and a YouTube Music playlist open the Playlist dialog,
/// as a playlist's does; a video with a list beside it is still that video.
#[test]
fn a_channel_and_a_music_playlist_open_the_playlist_dialog() {
    let settings = Settings::default();
    for url in [
        "https://www.youtube.com/playlist?list=PL1",
        "https://music.youtube.com/playlist?list=PL1",
        "https://www.youtube.com/@YouTube/videos",
    ] {
        assert_eq!(
            decide(url, false, "", "", &settings, &[]).next,
            Next::Playlist(url.into()),
            "{url}"
        );
    }
    let video = "https://www.youtube.com/watch?v=jNQXAC9IVRw&list=PL1";
    assert_eq!(
        decide(video, false, "", "", &settings, &[]).next,
        Next::Details(vec![video.into()])
    );
}

/// gaps row W10: a feed's address (a podcast's RSS) opens the Playlist dialog, its items listed
/// and offered to be followed; a sitemap is a file.
#[test]
fn a_feed_opens_the_playlist_dialog() {
    let settings = Settings::default();
    let feed = "https://pod.example/show/feed.rss";
    assert_eq!(
        decide(feed, false, "", "", &settings, &[]).next,
        Next::Playlist(feed.into())
    );
    let sitemap = "https://pod.example/sitemap.xml";
    assert_eq!(
        decide(sitemap, false, "", "", &settings, &[]).next,
        Next::Details(vec![sitemap.into()])
    );
}

#[test]
fn several_links_a_line_each_are_a_batch_and_run_together_are_asked_about() {
    let settings = Settings::default();
    assert_eq!(
        decide(
            "https://e.com/a.zip\nhttps://e.com/b.zip",
            false,
            "",
            "",
            &settings,
            &[]
        )
        .next,
        Next::Details(vec![
            "https://e.com/a.zip".into(),
            "https://e.com/b.zip".into()
        ])
    );
    assert!(matches!(
        decide("https://e.com/a.ziphttps://e.com/b.zip", false, "", "", &settings, &[]).next,
        Next::Says(s) if s.starts_with("That looks like two links")
    ));
}

#[test]
fn a_secret_in_a_query_keeps_an_address_out_of_recent() {
    assert!(query_carries_a_secret(
        "https://cdn.example/f.zip?token=abc"
    ));
    assert!(query_carries_a_secret("https://example.com/get?Password=x"));
    assert!(!query_carries_a_secret(
        "https://example.com/get?id=7&name=report"
    ));
    let settings = Settings {
        recent_urls: vec!["https://e.com/a?sig=1".into(), "https://e.com/b".into()],
        ..Settings::default()
    };
    assert_eq!(recent(&settings), vec!["https://e.com/b".to_string()]);
}

/// A signed or token-bearing address is not kept in the settings file for
/// Add URL's list; a plain one is.
#[test]
fn an_address_with_a_secret_in_its_query_is_not_remembered() {
    let secret = query_carries_a_secret;
    assert!(secret("https://cdn.example/f.zip?token=abc"));
    assert!(secret(
        "https://cdn.example/f.zip?X-Amz-Signature=1&X-Amz-Credential=2"
    ));
    assert!(secret("https://example.com/get?id=7&api_key=9"));
    assert!(secret("https://example.com/get?Password=x"));
    assert!(secret("https://example.com/get?sessionid=x"));
    assert!(!secret("https://example.com/files/a.zip"));
    assert!(!secret("https://example.com/get?id=7&name=report"));
    assert!(!secret("not an address"));
}
