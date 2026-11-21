// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A podcast's RSS: each episode's enclosure, its title and its guid; a
/// relative enclosure made whole against the feed's address; `&amp;` undone.
#[test]
fn a_podcast_is_its_episodes_files() {
    let xml = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Talk &amp; More</title>
    <image><title>Not the feed's</title></image>
    <item>
      <title><![CDATA[Episode 2]]></title>
      <guid isPermaLink="false">ep-2</guid>
      <link>https://pod.example/ep2</link>
      <enclosure url="https://cdn.example/ep2.mp3?a=1&amp;b=2" type="audio/mpeg"/>
    </item>
    <item>
      <title>Episode 1</title>
      <enclosure url="media/ep1.mp3" type="audio/mpeg"></enclosure>
    </item>
    <item><title>No file</title><link>mailto:a@b.example</link></item>
  </channel>
</rss>"#;
    let feed = parse(xml, "https://pod.example/feed.rss").unwrap();
    assert_eq!(feed.title, "Talk & More");
    let urls: Vec<String> = feed.entries.iter().map(|e| e.url()).collect();
    assert_eq!(
        urls,
        [
            "https://cdn.example/ep2.mp3?a=1&b=2",
            "https://pod.example/media/ep1.mp3"
        ]
    );
    assert_eq!(feed.entries[0].title, "Episode 2");
    assert_eq!(feed.entries[0].id, "ep-2");
    assert_eq!(feed.entries[1].id, "https://pod.example/media/ep1.mp3");
}

/// An Atom feed (a channel's uploads): each entry's alternate link, or its
/// enclosure where it has one.
#[test]
fn an_atom_feed_is_its_entries_links() {
    let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
  <id>yt:channel:UC1</id>
  <title>A channel</title>
  <link rel="self" href="https://www.youtube.com/feeds/videos.xml?channel_id=UC1"/>
  <entry>
    <id>yt:video:abc</id>
    <title>New video</title>
    <link rel="alternate" href="https://www.youtube.com/watch?v=abc"/>
  </entry>
  <entry>
    <id>tag:2</id>
    <title>A file</title>
    <link href="https://e.example/page"/>
    <link rel="enclosure" href="https://e.example/f.zip"/>
  </entry>
</feed>"#;
    let feed = parse(
        xml,
        "https://www.youtube.com/feeds/videos.xml?channel_id=UC1",
    )
    .unwrap();
    assert_eq!(feed.title, "A channel");
    let urls: Vec<String> = feed.entries.iter().map(|e| e.url()).collect();
    assert_eq!(
        urls,
        [
            "https://www.youtube.com/watch?v=abc",
            "https://e.example/f.zip"
        ]
    );
    assert_eq!(feed.entries[0].id, "yt:video:abc");
}

/// A page, or nothing XML at all, is no feed.
#[test]
fn a_page_is_not_a_feed() {
    assert!(parse(
        "<html><body><a href='x.zip'>x</a></body></html>",
        "https://e.example/"
    )
    .is_none());
    assert!(parse("not xml", "https://e.example/").is_none());
}

/// A feed's address by its look; a sitemap or a file is not one.
#[test]
fn a_feed_is_known_by_its_address() {
    for url in [
        "https://pod.example/feed.rss",
        "https://e.example/blog/feed/",
        "https://e.example/rss",
        "https://e.example/podcast.xml",
        "https://www.youtube.com/feeds/videos.xml?channel_id=UC1",
        "https://e.example/news.atom",
    ] {
        assert!(looks_like_feed(url), "{url}");
    }
    for url in [
        "https://e.example/sitemap.xml",
        "https://e.example/file.zip",
        "ftp://e.example/feed.rss",
        "not an address",
    ] {
        assert!(!looks_like_feed(url), "{url}");
    }
}
