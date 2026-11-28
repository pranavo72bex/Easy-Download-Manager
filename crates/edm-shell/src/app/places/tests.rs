// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn row(id: Id, url: &str, state: State, queue: &str) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": url,
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "",
            "queue": queue,
            "description": "a note",
        }))
        .unwrap(),
    )
}

#[test]
fn each_place_holds_its_rows_and_counts_them() {
    let rows = vec![
        row(1, "http://e.com/film.mp4", State::Running, "Main"),
        row(2, "http://e.com/song.mp3", State::Completed, "Main"),
        row(3, "http://e.com/blob.bin", State::Queued, "Night"),
        row(4, "http://e.com/report.pdf", State::Paused, "Main"),
    ];
    let categories = Categories::default();
    let mut places = Places::default();
    places.place_rows(&rows, &categories);
    let count = |p: Place| places.count(&p, &rows, &categories);
    assert_eq!(count(Place::All), 4);
    assert_eq!(
        count(Place::Unfinished),
        3,
        "running, queued and stopped: every row not finished"
    );
    assert_eq!(count(Place::Finished), 1);
    assert_eq!(
        count(Place::Unfinished) + count(Place::Finished),
        count(Place::All)
    );
    assert_eq!(count(Place::Kind(Category::Videos)), 1);
    assert_eq!(count(Place::Kind(Category::Audio)), 1);
    assert_eq!(count(Place::Kind(Category::Other)), 1, "a .bin is Other");
    assert_eq!(count(Place::Queue("Night".into())), 1);
}

#[test]
fn the_filter_finds_every_word_in_the_name_address_or_note() {
    let rows = vec![
        row(1, "http://e.com/Holiday-Film.mp4", State::Running, "Main"),
        row(2, "http://e.com/song.mp3", State::Running, "Main"),
    ];
    let mut places = Places::default();
    places.place_rows(&rows, &Categories::default());
    assert!(places.matches_filter("holiday film", &rows[0]));
    assert!(!places.matches_filter("holiday film", &rows[1]));
    assert!(
        places.matches_filter("A NOTE", &rows[1]),
        "the note, any case"
    );
    assert!(
        places.matches_filter("  ", &rows[1]),
        "nothing typed: every row"
    );
}

#[test]
fn a_row_unchanged_is_not_placed_again_and_a_gone_one_is_forgotten() {
    let one = row(1, "http://e.com/a.zip", State::Running, "Main");
    let mut places = Places::default();
    let categories = Categories::default();
    places.place_rows(&[Arc::clone(&one)], &categories);
    assert_eq!(places.placed(&one, &categories).0, Some(Category::Archives));
    places.place_rows(&[], &categories);
    assert!(places.placed.is_empty() && places.haystacks.is_empty());
}

#[test]
fn a_place_is_read_from_its_word_and_one_that_is_not_there_refused() {
    let queues = vec!["Main".to_string(), "Night".to_string()];
    let customs = vec!["Work".to_string()];
    let read = |w: &str| Place::from_word(w, &queues, &customs);
    assert_eq!(read("all"), Ok(Place::All));
    assert_eq!(read("queue:Night"), Ok(Place::Queue("Night".into())));
    assert_eq!(read("custom:Work"), Ok(Place::Custom("Work".into())));
    let videos = super::super::names::category_word(Category::Videos);
    assert_eq!(
        read(&format!("kind:{videos}")),
        Ok(Place::Kind(Category::Videos))
    );
    assert_eq!(read("queue:Day"), Err("no queue is called Day".into()));
    assert_eq!(
        read("custom:Play"),
        Err("no category is called Play".into())
    );
    assert_eq!(
        read("kind:nonsense"),
        Err("no category is called nonsense".into())
    );
    assert_eq!(
        read("somewhere"),
        Err("no place is called somewhere".into())
    );
}

/// gaps row 181: placing again only the rows that changed in place places and counts as
/// placing every row again does.
#[test]
fn placing_the_changed_rows_alone_counts_as_placing_every_row() {
    let mut rows = vec![
        row(1, "http://e.com/film.mp4", State::Running, "Main"),
        row(2, "http://e.com/song.mp3", State::Queued, "Main"),
        row(3, "http://e.com/blob.bin", State::Queued, "Night"),
    ];
    let categories = Categories::default();
    let mut some = Places::default();
    some.place_rows(&rows, &categories);
    // The film finishes, and the song is renamed a report.
    rows[0] = row(1, "http://e.com/film.mp4", State::Completed, "Main");
    rows[1] = row(2, "http://e.com/report.pdf", State::Queued, "Main");
    some.place_at(&rows, &[0, 1], &categories);
    let mut every = Places::default();
    every.place_rows(&rows, &categories);
    for place in [
        Place::All,
        Place::Unfinished,
        Place::Finished,
        Place::Kind(Category::Videos),
        Place::Kind(Category::Audio),
        Place::Kind(Category::Documents),
        Place::Queue("Night".into()),
    ] {
        assert_eq!(
            some.count(&place, &rows, &categories),
            every.count(&place, &rows, &categories),
            "{place:?}"
        );
    }
    assert!(some.matches_filter("report", &rows[1]));
    assert!(!some.matches_filter("song", &rows[1]));
}
