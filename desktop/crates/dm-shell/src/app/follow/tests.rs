// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use dm_core::youtube::{Playlist, PlaylistEntry};

use super::*;

fn video(id: &str) -> PlaylistEntry {
    PlaylistEntry {
        id: id.into(),
        title: format!("Video {id}"),
        seconds: Some(60),
        page: None,
    }
}

fn playlist(ids: &[&str]) -> Playlist {
    Playlist {
        id: "PL1".into(),
        title: "Lessons".into(),
        entries: ids.iter().map(|&id| video(id)).collect(),
        cut_short: false,
    }
}

/// A stand-in playlist followed with two videos gains a third: only the
/// third is added, for later, to the queue chosen, at the quality chosen;
/// a video the person cleared from the list is not added again.
#[test]
fn a_followed_playlist_that_gains_a_video_adds_only_that_one_to_its_queue() {
    let address = "https://www.youtube.com/playlist?list=PL1";
    let follow = following(&playlist(&["a", "b"]), address, 6, "Lessons", "720p", 1000);
    assert_eq!(follow.seen.len(), 2);
    assert_eq!(follow.title, "Lessons");
    let settings = followed(&crate::Settings::default(), follow.clone());
    assert_eq!(settings.follows, [follow.clone()]);
    // Not due before its six hours; due after.
    assert!(due_now(&settings, 1000 + 3600).is_none());
    let (due, stamped) = due_now(&settings, 1000 + 6 * 3600).unwrap();
    assert_eq!(due, [follow.clone()]);
    assert_eq!(stamped.follows[0].last_run, 1000 + 6 * 3600);

    // Uploads come first: the new one at the top.
    let now = playlist(&["c", "a", "b"]);
    let added = new_items(&follow, &now, &[]);
    assert_eq!(added.len(), 1, "{added:?}");
    let (url, with) = &added[0];
    assert_eq!(url, "https://www.youtube.com/watch?v=c");
    assert!(with.later);
    assert_eq!(with.queue, "Lessons");
    assert_eq!(with.quality.as_deref(), Some("720p"));

    // Followed again at the same address: kept once, not twice.
    let again = followed(
        &settings,
        Follow {
            every_hours: 1,
            ..follow.clone()
        },
    );
    assert_eq!(again.follows.len(), 1);
    assert_eq!(again.follows[0].every_hours, 1);
    assert!(unfollowed(&again, &follow.address).follows.is_empty());
}

/// A queue left empty is the main queue; an interval past a month is a month.
#[test]
fn a_follow_without_a_queue_adds_to_the_main_queue() {
    let follow = following(
        &playlist(&[]),
        "https://e.example/feed.rss",
        10_000,
        "",
        "",
        0,
    );
    assert_eq!(follow.every_hours, crate::lists::MOST_HOURS);
    let added = new_items(&follow, &playlist(&["z"]), &[]);
    assert_eq!(added[0].1.queue, dm_core::MAIN_QUEUE);
    assert_eq!(added[0].1.quality, None);
}

/// The feed's text with `items`, each an episode's file on `origin`.
fn feed(origin: &str, items: &[&str]) -> String {
    let items: String = items
        .iter()
        .map(|name| {
            format!(
                "<item><title>{name}</title><guid>{name}</guid>\
                 <enclosure url=\"{origin}/files/{name}.mp3\" type=\"audio/mpeg\"/></item>"
            )
        })
        .collect();
    format!("<rss version=\"2.0\"><channel><title>A podcast</title>{items}</channel></rss>")
}

/// A stand-in feed, served, followed with one episode, gains a second: read
/// again (as the Playlist dialog reads it), only the second is added, to the
/// queue chosen, and what it has seen is kept.
#[test]
fn a_followed_feed_that_gains_an_item_adds_only_that_one_to_its_queue() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, dir.path());
    let listed: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(vec!["one"]));
    let served = Arc::clone(&listed);
    let origin = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let at = origin.clone();
        let app = axum::Router::new().route(
            "/podcast.rss",
            axum::routing::get(move || {
                let items = served.lock().unwrap().clone();
                let text = feed(&at, &items);
                async move { text }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        origin
    });
    let address = format!("{origin}/podcast.rss");
    let read = |address: &str| {
        let (tell, heard) = std::sync::mpsc::channel();
        read_later(&backend, address.to_string(), move |got| {
            let _ = tell.send(got);
        });
        heard
            .recv_timeout(Duration::from_secs(25))
            .unwrap()
            .unwrap()
    };

    // Followed as the dialog follows it: its one episode taken by hand, seen.
    let first = read(&address);
    assert_eq!(first.title, "A podcast");
    assert_eq!(first.entries.len(), 1);
    let follow = following(&first, &address, 24, "Podcasts", "", 0);
    let added = backend.add_each(new_items(&Follow::default(), &first, &[]));
    assert!(added.iter().all(Result::is_ok), "{added:?}");
    // Kept as the window keeps it (not written to a settings file here).
    let settings = followed(&backend.settings(), follow);

    listed.lock().unwrap().push("two");
    let again = read(&address);
    assert_eq!(again.entries.len(), 2);
    let kept = &settings.follows[0];
    let rows = backend.list_shared();
    let new = new_items(kept, &again, &rows);
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!(new[0].0, format!("{origin}/files/two.mp3"));
    let added = backend.add_each(new);
    assert!(added.iter().all(Result::is_ok), "{added:?}");
    let rows = backend.list_shared();
    let two = rows.iter().find(|r| r.url.ends_with("/two.mp3")).unwrap();
    assert_eq!(two.queue, "Podcasts");
    assert_eq!(rows.len(), 2, "the first episode is not added again");

    let saved = seen_saved(&settings, &kept.address, &again, &rows).unwrap();
    assert_eq!(saved.follows[0].seen.len(), 2);
    assert!(new_items(&saved.follows[0], &again, &[]).is_empty());
}
