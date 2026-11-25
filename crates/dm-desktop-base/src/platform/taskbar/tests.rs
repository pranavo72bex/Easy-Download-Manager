// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use dm_core::Progress;

fn row(state: State, downloaded: u64, total: Option<u64>, finished_at: Option<u64>) -> Entry {
    Entry {
        id: 1,
        url: "https://e.com/f.bin".into(),
        state,
        progress: Progress {
            downloaded,
            total,
            speed: 0.0,
            eta: None,
            estimated: false,
        },
        filename: "f.bin".into(),
        finished_at,
        category: None,
        checksum: None,
        verified: false,
        disguised: None,
        kept_sha256: None,
        mirrors: 0,
        path: String::new(),
        referrer: String::new(),
        paused_by_schedule: false,
        description: String::new(),
        queue: String::new(),
        save_dir: None,
        save_name: None,
        accept_page: false,
        validator: None,
        quality: None,
        sound: None,
        span: None,
        shape: None,
        mp3: false,
        gif: false,
        delete_after: None,
        unpack: false,
        parts_dir: String::new(),
        began_in: String::new(),
        cookies: String::new(),
        cookies_once: false,
        headers: Vec::new(),
        replaces: None,
        speed_limit: 0,
        stalled: String::new(),
        fresh_link: false,
        title_hint: None,
        torrent_files: Default::default(),
        said: String::new(),
        post: None,
        posted: false,
        fail_kind: None,
        keep_no_record: false,
        accept_same: false,
    }
}

/// The Dock and a launcher show a bar in whole hundredths while sized
/// downloads run, and the count of those running (gaps row U16).
#[test]
fn outside_the_window_a_bar_in_hundredths_and_a_count() {
    let rows = [
        row(State::Running, 50, Some(100), None),
        row(State::Running, 30, Some(300), None),
        row(State::Paused, 0, Some(10), None),
    ];
    let now = outside_of(mode_of(&rows, 0), running_of(&rows));
    assert_eq!(
        now,
        Outside {
            percent: Some(20),
            running: 2
        }
    );
    let unknown = [row(State::Running, 5, None, None)];
    assert_eq!(
        outside_of(mode_of(&unknown, 0), running_of(&unknown)),
        Outside {
            percent: None,
            running: 1
        }
    );
    let paused = [row(State::Paused, 1, Some(2), None)];
    assert_eq!(
        outside_of(mode_of(&paused, 0), running_of(&paused)),
        Outside {
            percent: None,
            running: 0
        }
    );
    // Past its size (a server's wrong length), the bar stops full.
    assert_eq!(
        outside_of(Mode::Running { done: 9, total: 4 }, 1).percent,
        Some(100)
    );
}

#[test]
fn running_rows_add_up() {
    let rows = [
        row(State::Running, 50, Some(100), None),
        row(State::Running, 30, Some(300), None),
        row(State::Paused, 0, Some(10), None),
    ];
    assert_eq!(
        mode_of(&rows, 0),
        Mode::Running {
            done: 80,
            total: 400
        }
    );
}

#[test]
fn a_running_row_of_unknown_size_is_indeterminate() {
    let rows = [row(State::Running, 5, None, None)];
    assert_eq!(mode_of(&rows, 0), Mode::Unknown);
}

#[test]
fn paused_when_nothing_runs() {
    let rows = [row(State::Paused, 1, Some(2), None)];
    assert_eq!(mode_of(&rows, 0), Mode::Paused);
    assert_eq!(
        mode_of(&[row(State::Completed, 2, Some(2), Some(1))], 9),
        Mode::Nothing
    );
}

#[test]
fn a_failure_is_red_for_a_minute_only() {
    let rows = [row(State::Failed("x".into()), 0, None, Some(1000))];
    assert_eq!(mode_of(&rows, 1030), Mode::Failed);
    assert_eq!(mode_of(&rows, 1100), Mode::Nothing);
}

/// The main window's title says how far and how fast the downloads go, before " — dm"; "dm"
/// while nothing runs (gaps row U16).
#[test]
fn the_title_says_how_far_and_how_fast() {
    let said = |r: f64| format!("{r} B/s");
    let at = Mode::Running {
        done: 37,
        total: 100,
    };
    assert_eq!(titled(at, 4100.0, said), "37% · 4100 B/s — dm");
    assert_eq!(titled(at, 0.0, said), "37% — dm", "no speed measured yet");
    assert_eq!(
        titled(Mode::Unknown, 4100.0, said),
        "4100 B/s — dm",
        "no size, no percentage"
    );
    assert_eq!(titled(Mode::Unknown, 0.0, said), MAIN_TITLE);
    for idle in [Mode::Nothing, Mode::Paused, Mode::Failed] {
        assert_eq!(titled(idle, 4100.0, said), MAIN_TITLE, "{idle:?}");
    }
    // In the numbers the list says them in.
    let title = window_title(Mode::Running { done: 1, total: 4 }, 3.0 * 1024.0 * 1024.0);
    assert!(
        title.starts_with("25% · ") && title.ends_with("/s — dm"),
        "{title}"
    );
}

/// Scripts and the control port know the main window by its title: "dm", or one ending " — dm",
/// which no dialog's title does.
#[test]
fn the_main_window_is_known_by_its_title() {
    assert!(is_main_title("dm"));
    assert!(is_main_title("37% · 4.1 MB/s — dm"));
    assert!(is_main_title("4.1 MB/s — dm"));
    assert!(!is_main_title("About dm"));
    assert!(!is_main_title("Exit dm"));
    assert!(!is_main_title("37% Download progress — a.bin"));
}

#[test]
fn the_speed_is_the_running_rows_together() {
    let mut rows = [
        row(State::Running, 1, Some(2), None),
        row(State::Running, 1, None, None),
        row(State::Paused, 1, Some(2), None),
    ];
    for (row, speed) in rows.iter_mut().zip([100.0, 50.0, 900.0]) {
        row.progress.speed = speed;
    }
    assert_eq!(rate_of(&rows), 150.0);
    assert_eq!(rate_of::<Entry>(&[]), 0.0);
}
