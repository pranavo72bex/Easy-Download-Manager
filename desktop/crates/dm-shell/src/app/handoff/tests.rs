// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn request(url: &str, caught: bool) -> Request {
    Request {
        url: url.into(),
        later: true,
        caught,
        ..Request::default()
    }
}

fn settings(ask: bool) -> Settings {
    Settings {
        ask_caught_downloads: ask,
        ask_panel_downloads: ask,
        ..Settings::default()
    }
}

fn hand(backend: &Backend, r: Request) {
    dm_core::lock(&backend.handed).push_back(r);
}

fn row(url: &str) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": 7,
            "url": url,
            "state": serde_json::to_value(dm_core::State::Paused).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "f",
        }))
        .unwrap(),
    )
}

#[test]
fn a_hand_off_is_added_with_nobody_to_ask_and_the_same_click_dropped() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let mut hands = Hands::default();
    let now = Instant::now();
    let s = settings(true);
    assert!(matches!(
        hands.next(&backend, &s, &[], Asking::Nobody, now),
        Route::Nothing
    ));

    hand(&backend, request("http://127.0.0.1:9/a.bin", false));
    assert!(matches!(
        hands.next(&backend, &s, &[], Asking::Nobody, now),
        Route::Added(Ok(_))
    ));
    hand(&backend, request("http://127.0.0.1:9/a.bin", false));
    assert!(matches!(
        hands.next(&backend, &s, &[], Asking::Nobody, now),
        Route::Again
    ));
    // A few seconds on, it is meant.
    hand(&backend, request("http://127.0.0.1:9/a.bin", false));
    let later = now + HANDED_TWICE_WITHIN;
    assert!(matches!(
        hands.next(&backend, &s, &[], Asking::Nobody, later),
        Route::Added(Ok(_))
    ));
}

#[test]
fn a_part_typed_on_the_panel_is_asked_about_whatever_the_settings_say() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let mut hands = Hands::default();
    let now = Instant::now();
    let part = || Request {
        to: "0:30".into(),
        ..request("http://127.0.0.1:9/v.mp4", false)
    };
    hand(&backend, part());
    assert!(matches!(
        hands.next(&backend, &settings(false), &[], Asking::Free, now),
        Route::Ask(_)
    ));
    // With no window it is added, its part kept (`shell::add_options`).
    hand(&backend, part());
    assert!(matches!(
        hands.next(&backend, &settings(false), &[], Asking::Nobody, now),
        Route::Added(Ok(_))
    ));
}

#[test]
fn a_window_asks_or_waits_for_its_open_dialog() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let mut hands = Hands::default();
    let now = Instant::now();
    hand(&backend, request("http://127.0.0.1:9/b.bin", false));
    assert!(matches!(
        hands.next(&backend, &settings(true), &[], Asking::Busy, now),
        Route::Nothing
    ));
    assert_eq!(dm_core::lock(&backend.handed).len(), 1, "it waits");
    assert!(matches!(
        hands.next(&backend, &settings(true), &[], Asking::Free, now),
        Route::Ask(r) if r.url == "http://127.0.0.1:9/b.bin"
    ));
    // Not asked by the person's setting: added though a dialog is open.
    hand(&backend, request("http://127.0.0.1:9/c.bin", false));
    assert!(matches!(
        hands.next(&backend, &settings(false), &[], Asking::Busy, now),
        Route::Added(Ok(_))
    ));
}

#[test]
fn a_pages_links_are_asked_about_or_added_with_their_words() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let mut hands = Hands::default();
    let now = Instant::now();
    let page = || {
        let mut r = request("http://127.0.0.1:9/page", false);
        r.links = vec!["http://127.0.0.1:9/1".into(), "http://127.0.0.1:9/2".into()];
        r.texts
            .insert("http://127.0.0.1:9/1".into(), "First file".into());
        r.names
            .insert("http://127.0.0.1:9/2".into(), "second.zip".into());
        r.referrer = "http://127.0.0.1:9/page".into();
        r
    };
    hand(&backend, page());
    assert!(matches!(
        hands.next(&backend, &settings(true), &[], Asking::Free, now),
        Route::AskLinks(r) if r.links.len() == 2
    ));
    hand(&backend, page());
    let Route::AddLinks(items) = hands.next(&backend, &settings(true), &[], Asking::Nobody, now)
    else {
        panic!("not added straight in");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].1.description, "First file");
    assert_eq!(items[1].1.save_name.as_deref(), Some("second.zip"));
    assert!(items.iter().all(|(_, w)| w.later
        && w.queue == dm_core::MAIN_QUEUE
        && w.referrer == "http://127.0.0.1:9/page"));
}

#[test]
fn a_caught_address_already_listed_is_shown_not_fetched_again() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let mut hands = Hands::default();
    let now = Instant::now();
    let rows = [row("http://127.0.0.1:9/d.bin")];
    let mut s = settings(false);
    s.duplicate_choice = Dup::Again.word().into();
    hand(&backend, request("http://127.0.0.1:9/d.bin", true));
    assert!(matches!(
        hands.next(&backend, &s, &rows, Asking::Free, now),
        Route::Listed {
            id: 7,
            how: Some(Dup::Again),
            ..
        }
    ));
    // With no window it is simply added.
    hand(&backend, request("http://127.0.0.1:9/d.bin", true));
    let later = now + HANDED_TWICE_WITHIN;
    assert!(matches!(
        hands.next(&backend, &s, &rows, Asking::Nobody, later),
        Route::Added(Ok(_))
    ));
}

#[test]
fn a_row_awaiting_a_fresh_link_takes_the_next_download_until_it_gives_up() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let mut hands = Hands::default();
    let now = Instant::now();
    hands.await_link(3, now);
    hand(&backend, request("http://127.0.0.1:9/fresh", true));
    assert!(matches!(
        hands.next(&backend, &settings(false), &[], Asking::Free, now),
        Route::FreshLink { id: 3, url } if url == "http://127.0.0.1:9/fresh"
    ));
    assert!(hands.awaiting_link.is_none(), "taken once");

    hands.await_link(3, now);
    hand(&backend, request("http://127.0.0.1:9/late", false));
    let gone = now + AWAIT_LINK_FOR + Duration::from_secs(1);
    assert!(matches!(
        hands.next(&backend, &settings(false), &[], Asking::Free, gone),
        Route::Added(Ok(_))
    ));
    assert!(hands.awaiting_link.is_none(), "given up");
}

#[test]
fn a_site_is_asked_about_after_two_cancels_and_not_once_left_with_the_browser() {
    let left = vec!["kept.example".to_string()];
    let mut cancels = Cancels::default();
    assert!(!cancels.cancelled("kept.example", &left));
    assert!(
        !cancels.cancelled("kept.example", &left),
        "a site already left there"
    );
    assert!(
        !cancels.cancelled("files.example", &left),
        "once is not twice"
    );
    assert!(cancels.cancelled("files.example", &left));
    assert!(
        !cancels.cancelled("files.example", &left),
        "counted again from none"
    );
    assert!(cancels.cancelled("files.example", &left));
    assert!(
        !cancels.cancelled("other.example", &left),
        "each site its own count"
    );
}
