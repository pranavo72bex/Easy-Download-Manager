// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The columns' sort, kept and read back, and the headings a sort groups rows under.

use super::*;

/// A heading or View › Sort by sorts up, then down, then back in the
/// order the downloads were added (gaps row U29).
#[test]
fn a_third_choice_puts_the_list_back_in_the_order_added() {
    let up = next_sort(None, Column::Size);
    assert_eq!(up, Some((Column::Size, true)));
    let down = next_sort(up, Column::Size);
    assert_eq!(down, Some((Column::Size, false)));
    assert_eq!(next_sort(down, Column::Size), None);
    // Another column starts at up, whichever way the list was sorted.
    assert_eq!(next_sort(down, Column::Name), Some((Column::Name, true)));
}

/// The sort is kept in the settings and read back as it was; settings
/// saved before it was kept load with the list in the order added.
#[test]
fn the_sort_kept_is_the_sort_read_back() {
    let mut settings = edm_shell::settings::Settings::default();
    assert_eq!(kept_sort(&settings), None);
    for sort in [
        Some((Column::Rate, false)),
        Some((Column::Name, true)),
        None,
    ] {
        keep_sort(&mut settings, sort);
        assert_eq!(kept_sort(&settings), sort);
    }
    let old: edm_shell::settings::Settings =
        serde_json::from_str("{}").expect("settings with nothing in them load");
    assert_eq!(kept_sort(&old), None);
    assert!(old.sort_ascending);
}

#[test]
fn columns_sort_by_what_they_show() {
    use edm_core::State;
    use std::cmp::Ordering::*;
    let named = |name: &str| -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "https://e.com/f.bin",
            "state": serde_json::to_value(State::Paused).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": name,
        }))
        .unwrap()
    };
    // Names: numbers by value.
    assert_eq!(
        compare(Column::Name, &named("part 2"), &named("part 10")),
        Less
    );
    // Size: what has arrived of a file of unknown length is its size.
    let (mut big, mut small) = (named("a"), named("b"));
    big.progress.downloaded = 5000;
    small.progress.total = Some(10);
    assert_eq!(compare(Column::Size, &big, &small), Greater);
    // Speed and time left: a stopped row's last figures are not shown,
    // and do not sort it ahead of one that is running.
    let (mut stopped, mut running) = (named("a"), named("b"));
    stopped.progress.speed = 9e9;
    stopped.progress.eta = Some(std::time::Duration::from_secs(1));
    running.state = State::Running;
    running.progress.speed = 100.0;
    running.progress.eta = Some(std::time::Duration::from_secs(500));
    assert_eq!(compare(Column::Rate, &stopped, &running), Less);
    assert_eq!(compare(Column::TimeLeft, &stopped, &running), Less);
}

/// A sorted list's headings: by state, by day and by queue, none by name; each a run of
/// rows under the sort, so a heading never splits what the sort puts together.
#[test]
fn a_sorted_list_is_grouped_by_what_it_is_sorted_by() {
    let row = |state: State, finished_at: Option<u64>, queue: &str| -> Entry {
        let mut e: Entry = serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "https://e.com/f.bin",
            "state": serde_json::to_value(State::Paused).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "f.bin",
        }))
        .unwrap();
        e.state = state;
        e.finished_at = finished_at;
        e.queue = queue.into();
        e
    };
    let now = edm_core::clock::unix_now();
    let today = edm_core::clock::local_now().date;
    let day = 86_400;
    let at = |days_ago: i64| Some(u64::try_from(now - days_ago * day).unwrap());
    let group = |column, e: &Entry| group_of(column, e, today);
    assert_eq!(
        group(Column::Status, &row(State::Running, None, "")).as_deref(),
        Some("Downloading")
    );
    assert_eq!(
        group(Column::Status, &row(State::Completed, at(0), "")).as_deref(),
        Some("Finished")
    );
    assert_eq!(
        group(Column::LastTry, &row(State::Queued, None, "")).as_deref(),
        Some("Unfinished")
    );
    assert_eq!(
        group(Column::LastTry, &row(State::Completed, at(1), "")).as_deref(),
        Some("Yesterday")
    );
    assert_eq!(
        group(Column::LastTry, &row(State::Completed, at(4), "")).as_deref(),
        Some("Previous 7 days")
    );
    assert_eq!(
        group(Column::LastTry, &row(State::Completed, at(90), "")).as_deref(),
        Some("Earlier")
    );
    assert_eq!(
        group(Column::Queue, &row(State::Queued, None, "Nightly")).as_deref(),
        Some("Nightly")
    );
    assert_eq!(
        group(Column::Queue, &row(State::Queued, None, "")).as_deref(),
        Some("No queue")
    );
    assert_eq!(group(Column::Name, &row(State::Queued, None, "")), None);
    // Sorted, each heading's rows stand together: no heading comes back after another.
    let mut rows: Vec<Entry> = [0, 40, 1, 9, 3, 0, 200]
        .iter()
        .map(|&d| row(State::Completed, at(d), ""))
        .chain([row(State::Running, None, "")])
        .collect();
    for up in [true, false] {
        rows.sort_by(|a, b| {
            let o = compare(Column::LastTry, a, b);
            if up {
                o
            } else {
                o.reverse()
            }
        });
        let mut seen: Vec<String> = Vec::new();
        for e in &rows {
            let g = group(Column::LastTry, e).unwrap();
            if seen.last() != Some(&g) {
                assert!(!seen.contains(&g), "{g} came back, sorted up: {up}");
                seen.push(g);
            }
        }
        assert_eq!(seen.len(), 6);
    }
}
