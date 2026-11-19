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
