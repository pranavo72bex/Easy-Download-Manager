// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Playlist window, driven by the words on screen: what it lists, what its ticks and
//! quality pick, what Start now and Add for later add, and what it says while it reads or
//! when it cannot. Ported from the egui window's `tests/dialogs_cover.rs`.

use dm_core::youtube::{Playlist, PlaylistEntry};
use dm_core::State;

use crate::harness::world::{world, World};
use crate::harness::Harness;

/// A playlist of three videos, each on a page of the loopback server.
fn three_videos(world: &World) -> Playlist {
    let video = |id: &str, title: &str, seconds: Option<u64>, page: &str| PlaylistEntry {
        id: id.into(),
        title: title.into(),
        seconds,
        page: Some(world.url(page)),
    };
    Playlist {
        id: "talks".into(),
        title: "Autumn talks".into(),
        entries: vec![
            video("1", "Opening", Some(65), "file.zip"),
            video("2", "Second", None, "other.pdf"),
            video("3", "Closing", Some(3_725), "deep.pdf"),
        ],
        cut_short: true,
    }
}

/// The Playlist window over `read`, or reading `url` where there is none.
fn open(h: &mut Harness, url: String, read: Option<Playlist>) {
    h.update(|_, cx| crate::playlist::open(cx.entity().downgrade(), url, read, cx));
    h.until("the Playlist window", |h| h.dialog_open("Playlist"));
}

/// Every video listed ticked, with its number and length, the list said to be cut short;
/// Start now takes the ticked ones from their own pages, from the playlist's page.
#[test]
fn a_playlist_lists_its_videos_ticked_and_start_now_takes_the_ticked_ones() {
    let world = world();
    let mut h = world.harness();
    open(&mut h, world.url("talks"), Some(three_videos(&world)));
    h.until_shown("Autumn talks");
    assert!(
        h.shows_part("Only the first 3 videos are listed."),
        "{:?}",
        h.texts()
    );
    assert!(h.shows_part("Opening · 1:05"), "{:?}", h.texts());
    assert!(h.shows_part("Second"));
    assert!(h.shows_part("Closing · 1:02:05"), "{:?}", h.texts());

    let second = h
        .texts()
        .into_iter()
        .find(|t| t.contains("Second"))
        .unwrap();
    h.click(&second);
    h.click("Start now");
    h.until("the window to close", |h| !h.dialog_open("Playlist"));
    assert_eq!(h.said(), "Added 2 downloads.");
    h.until("the rows", |h| h.rows().len() == 2);
    let rows = h.rows();
    let urls: Vec<&str> = rows.iter().map(|r| r.url.as_str()).collect();
    assert!(urls.contains(&world.url("file.zip").as_str()), "{urls:?}");
    assert!(urls.contains(&world.url("deep.pdf").as_str()), "{urls:?}");
    let taken: Vec<(&str, Option<&str>)> = rows
        .iter()
        .map(|r| (r.referrer.as_str(), r.quality.as_deref()))
        .collect();
    // At the best quality there is, as the window opens with it picked.
    assert!(
        rows.iter().all(|r| r.referrer == world.url("talks")
            && matches!(r.quality.as_deref(), None | Some("best"))),
        "{taken:?}"
    );
}

/// Select all unticks and ticks every video; with none ticked the window says to pick one;
/// Add for later puts them in the Main queue at the quality picked.
#[test]
fn a_playlist_is_added_for_later_at_the_quality_picked() {
    let world = world();
    let mut h = world.harness();
    open(&mut h, world.url("talks"), Some(three_videos(&world)));
    h.until_shown("Select all");
    h.click("Select all");
    h.click("Add for later");
    h.until_shown("Pick at least one video.");
    assert!(h.rows().is_empty());
    h.click("Select all");
    h.click("Best available");
    h.click("Up to 720p");
    h.click("Add for later");
    h.until("the window to close", |h| !h.dialog_open("Playlist"));
    assert_eq!(h.said(), "Added 3 downloads.");
    h.until("the rows", |h| h.rows().len() == 3);
    assert!(h.rows().iter().all(|r| r.queue == dm_core::MAIN_QUEUE
        && r.state == State::Paused
        && r.quality.as_deref() == Some("720")));
}

/// While the page is read the window says so and takes nothing; a page of another site that
/// is one video, not a playlist, is said to be so; Escape closes the window.
#[test]
fn a_page_that_is_one_video_is_said_not_to_be_a_playlist_and_escape_closes() {
    let world = world();
    let mut h = world.harness();
    open(&mut h, world.url("article"), None);
    h.until("the page read", |h| {
        h.shows_part("That page is one video, not a playlist.")
    });
    h.click("Start now");
    assert!(h.dialog_open("Playlist"));
    assert!(h.rows().is_empty());
    h.key("escape", &[]);
    h.until("Escape to close it", |h| !h.dialog_open("Playlist"));
    assert!(h.rows().is_empty());
}

/// Enter takes the ticked videos, as Start now does; a video whose page is not one dm can
/// fetch is refused with the reason, and the window stays.
#[test]
fn enter_starts_a_playlist_and_a_refusal_is_said_in_it() {
    let world = world();
    let mut h = world.harness();
    let mut list = three_videos(&world);
    list.cut_short = false;
    list.entries.truncate(1);
    list.entries[0].page = Some("nothing-to-fetch".into());
    open(&mut h, world.url("talks"), Some(list));
    h.until("the list", |h| h.shows_part("Opening · 1:05"));
    h.key("enter", &[]);
    h.until("the refusal", |h| {
        h.texts().iter().any(|t| t.contains("dm can download"))
    });
    assert!(h.dialog_open("Playlist"), "the refusal closed it");
    assert!(h.rows().is_empty());
}
