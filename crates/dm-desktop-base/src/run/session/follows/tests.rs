// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The followed ones in the tick every driver runs (gaps row W10): a reading that came back
//! is handed on once, and Check now's reading comes back with a later tick. Nothing here is
//! due by the clock, so nothing is saved: outside dm-shell's own tests the settings are the
//! real home's.

use super::super::tests::{backend, window};
use super::*;
use dm_core::youtube::PlaylistEntry;

/// A followed feed of one item.
fn one_item() -> Playlist {
    Playlist {
        id: String::new(),
        title: "Weekly".into(),
        entries: vec![PlaylistEntry {
            id: "1".into(),
            title: "Episode 1".into(),
            seconds: None,
            page: Some("http://127.0.0.1:9/episode-1.mp3".into()),
        }],
        cut_short: false,
    }
}

/// The followed ones' effects among a tick's.
fn followed(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Followed { .. }))
        .collect()
}

#[test]
fn a_reading_that_came_back_is_handed_to_the_driver_once_in_the_order_read() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&runtime, dir.path(), Default::default());
    let mut session = Session::default();
    // Nothing followed, nothing read: the tick asks nothing of it.
    assert!(followed(&session.tick(&backend, &[], window())).is_empty());
    {
        let mut results = dm_core::lock(&session.follows.results);
        results.push(("http://a.example/feed.xml".into(), Ok(one_item())));
        results.push(("http://b.example/feed.xml".into(), Err("gone".into())));
    }
    let effects = session.tick(&backend, &[], window());
    assert_eq!(
        followed(&effects),
        vec![
            &Effect::Followed {
                address: "http://a.example/feed.xml".into(),
                read: Ok(one_item()),
            },
            &Effect::Followed {
                address: "http://b.example/feed.xml".into(),
                read: Err("gone".into()),
            },
        ]
    );
    assert!(
        followed(&session.tick(&backend, &[], window())).is_empty(),
        "each handed on once"
    );
}

#[test]
fn check_now_reads_in_the_background_and_a_later_tick_brings_what_it_read() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&runtime, dir.path(), Default::default());
    let mut session = Session::default();
    // A feed at a port nothing listens on: read, and refused.
    let address = "http://127.0.0.1:9/feed.xml".to_string();
    let follow = Follow {
        address: address.clone(),
        every_hours: 24,
        ..Default::default()
    };
    session.check_follow(&backend, &follow);
    let deadline = Instant::now() + Duration::from_secs(60);
    let read = loop {
        let effects = session.tick(&backend, &[], window());
        if let Some(Effect::Followed { address: a, read }) = followed(&effects).first().copied() {
            assert_eq!(a, &address);
            break read.clone();
        }
        assert!(Instant::now() < deadline, "the reading never came back");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(read.is_err(), "{read:?}");
}
