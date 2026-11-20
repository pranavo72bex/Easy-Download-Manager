// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::time::Duration;

use super::*;

/// 19 November 2026, noon UTC, in seconds since 1970.
const NOON: u64 = 1_795_089_600;

fn today() -> Date {
    Date::new(2026, 11, 19).unwrap()
}

fn row(id: u64, url: &str, name: &str, size: u64, days_ago: Option<u64>, state: &str) -> Entry {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "url": url,
        "state": {"kind": state},
        "progress": {"downloaded": size, "total": size, "speed": 0.0, "eta": null},
        "filename": name,
        "finished_at": days_ago.map(|d| NOON - d * 86_400),
    }))
    .unwrap()
}

fn rows() -> Vec<Entry> {
    vec![
        row(
            1,
            "https://www.a.com/film.mp4",
            "film.mp4",
            700,
            Some(0),
            "completed",
        ),
        row(
            2,
            "https://a.com/song.mp3",
            "song.mp3",
            300,
            Some(0),
            "completed",
        ),
        row(
            3,
            "https://b.org/pack.zip",
            "pack.zip",
            500,
            Some(2),
            "completed",
        ),
        row(
            4,
            "https://c.net/book.pdf",
            "book.pdf",
            50,
            Some(45),
            "completed",
        ),
        // Not finished well: not counted.
        row(
            5,
            "https://d.io/x.zip",
            "x.zip",
            9_000,
            Some(0),
            "cancelled",
        ),
        row(6, "https://d.io/y.zip", "y.zip", 9_000, None, "running"),
        // Finished before the time was kept: counted, on no day.
        row(
            7,
            "https://b.org/old.zip",
            "old.zip",
            100,
            None,
            "completed",
        ),
    ]
}

#[test]
fn the_finished_rows_are_counted_once_each() {
    let rows = rows();
    let s = Statistics::of(&rows, today(), Zone::UTC, &SpeedsSeen::default());
    assert_eq!(s.files, 5);
    assert_eq!(s.bytes, 700 + 300 + 500 + 50 + 100);
}

#[test]
fn by_day_holds_the_last_thirty_days_oldest_first() {
    let rows = rows();
    let s = Statistics::of(&rows, today(), Zone::UTC, &SpeedsSeen::default());
    assert_eq!(s.days.len(), DAYS);
    assert_eq!(s.days[DAYS - 1].date, today());
    assert_eq!(s.days[0].date, Date::new(2026, 10, 21).unwrap());
    assert_eq!(
        s.days[DAYS - 1].bytes,
        1_000,
        "today: the film and the song"
    );
    assert_eq!(s.days[DAYS - 3].bytes, 500, "two days ago: the zip");
    let counted: u64 = s.days.iter().map(|d| d.bytes).sum();
    assert_eq!(counted, 1_500, "45 days ago and no day are not on it");
    assert_eq!(s.busiest_day(), 1_000);
}

#[test]
fn a_day_is_the_computers_own() {
    // Finished at 23:00 UTC on the 18th is the 19th nine hours east.
    let mut late = row(1, "https://a.com/f.bin", "f.bin", 10, None, "completed");
    late.finished_at = Some(NOON - 13 * 3_600);
    let east = Zone::Fixed(9 * 3_600);
    let s = Statistics::of([&late], today(), east, &SpeedsSeen::default());
    assert_eq!(s.days[DAYS - 1].bytes, 10);
    let s = Statistics::of([&late], today(), Zone::UTC, &SpeedsSeen::default());
    assert_eq!(s.days[DAYS - 2].bytes, 10);
}

#[test]
fn by_site_puts_the_largest_first_and_www_is_the_same_site() {
    let rows = rows();
    let s = Statistics::of(&rows, today(), Zone::UTC, &SpeedsSeen::default());
    assert_eq!(
        s.sites,
        vec![
            Share {
                name: "a.com".into(),
                bytes: 1_000
            },
            Share {
                name: "b.org".into(),
                bytes: 600
            },
            Share {
                name: "c.net".into(),
                bytes: 50
            },
        ]
    );
}

#[test]
fn by_site_names_ten_at_most() {
    let many: Vec<Entry> = (0..15)
        .map(|n| {
            let url = format!("https://s{n}.com/f.bin");
            row(n, &url, "f.bin", 100 + n, Some(1), "completed")
        })
        .collect();
    let s = Statistics::of(&many, today(), Zone::UTC, &SpeedsSeen::default());
    assert_eq!(s.sites.len(), SITES);
    assert_eq!(s.sites[0].name, "s14.com");
    assert_eq!(s.files, 15, "every file still counted");
}

#[test]
fn by_category_names_each_kind_as_the_side_panel_does() {
    let rows = rows();
    let s = Statistics::of(&rows, today(), Zone::UTC, &SpeedsSeen::default());
    let named: Vec<(&str, u64)> = s
        .categories
        .iter()
        .map(|c| (c.name.as_str(), c.bytes))
        .collect();
    assert_eq!(
        named,
        vec![
            ("Videos", 700),
            ("Archives", 600),
            ("Audio", 300),
            ("Documents", 50),
        ]
    );
}

#[test]
fn nothing_finished_is_all_nothing() {
    let s = Statistics::of(
        std::iter::empty::<&Entry>(),
        today(),
        Zone::UTC,
        &SpeedsSeen::default(),
    );
    assert_eq!(s.files, 0);
    assert!(s.sites.is_empty() && s.categories.is_empty());
    assert_eq!(s.days.len(), DAYS);
    assert_eq!(s.busiest_day(), 0);
    assert_eq!(s.average_speed, None);
    assert_eq!(s.best_speed, None);
}

#[test]
fn a_day_is_said_as_the_list_says_one() {
    assert_eq!(day_words(today()), "19 Nov");
    assert_eq!(fraction(50, 200), 0.25);
    assert_eq!(fraction(5, 0), 0.0);
}

#[test]
fn the_speeds_seen_average_over_the_time_something_ran() {
    let t0 = Instant::now();
    let mut seen = SpeedsSeen::default();
    assert_eq!(seen.average(), None);
    seen.sample(t0, 1_000.0);
    seen.sample(t0 + Duration::from_secs(1), 1_000.0);
    seen.sample(t0 + Duration::from_secs(2), 3_000.0);
    // Nothing running for a while: not part of the average.
    seen.sample(t0 + Duration::from_secs(4), 0.0);
    assert_eq!(seen.average(), Some(2_000.0));
    assert_eq!(seen.best(), Some(3_000.0));
    // A computer asleep for an hour counts five seconds at most.
    seen.sample(t0 + Duration::from_secs(3_604), 4_000.0);
    assert_eq!(seen.average(), Some((1_000.0 + 3_000.0 + 20_000.0) / 7.0));
    let s = Statistics::of(std::iter::empty::<&Entry>(), today(), Zone::UTC, &seen);
    assert_eq!(s.best_speed, Some(4_000.0));
}

#[test]
fn the_speed_line_keeps_a_second_a_sample_for_five_minutes() {
    let t0 = Instant::now();
    let mut line = SpeedLine::default();
    assert!(line.is_empty());
    line.sample(t0, 100.0);
    // Within the second: the latest stands for it.
    line.sample(t0 + Duration::from_millis(400), 200.0);
    assert_eq!(line.len(), 1);
    line.sample(t0 + Duration::from_millis(1_100), 300.0);
    assert_eq!(line.len(), 2);
    // Three seconds with no frame: nothing for the two missed.
    line.sample(t0 + Duration::from_millis(4_200), 400.0);
    assert_eq!(line.len(), 5);
    assert_eq!(line.best(), 400.0);
    // Ten minutes on, five are held.
    for s in 5..600 {
        line.sample(t0 + Duration::from_millis(s * 1_000 + 200), s as f64);
    }
    assert_eq!(line.len(), SpeedLine::SECONDS);
    assert_eq!(line.best(), 599.0);
}

#[test]
fn the_speed_line_is_drawn_as_bars_against_the_tallest() {
    let t0 = Instant::now();
    let mut line = SpeedLine::default();
    assert_eq!(line.bars(60), vec![0.0; 60], "nothing yet");
    for s in 0..10u64 {
        line.sample(
            t0 + Duration::from_secs(s),
            if s < 5 { 100.0 } else { 200.0 },
        );
    }
    let bars = line.bars(60);
    assert_eq!(bars.len(), 60);
    // Five seconds a bar: the last two hold the ten seconds, the rest are before them.
    assert_eq!(bars[58], 0.5);
    assert_eq!(bars[59], 1.0);
    assert!(bars[..58].iter().all(|b| *b == 0.0));
}
