// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::app::snapshot::SavedSizes;

fn template() -> Entry {
    serde_json::from_value(serde_json::json!({
        "id": 0,
        "url": "http://example.com/f.bin",
        "state": serde_json::to_value(State::Completed).unwrap(),
        "progress": {"downloaded": 10, "total": 10, "speed": 0.0, "eta": null},
        "filename": "f.bin",
    }))
    .unwrap()
}

/// A history of `size` rows as a window reads it: most finished (their file on disk), the rest
/// waiting, and `running` downloads at the end, whose figures move at each read.
fn history(size: usize, running: usize, file: &std::path::Path) -> Vec<Arc<Entry>> {
    let template = template();
    let finished = size * 6 / 10;
    (0..size)
        .map(|i| {
            let mut row = template.clone();
            row.id = i as Id + 1;
            if i < finished {
                row.path = file.display().to_string();
                row.finished_at = Some(1_700_000_000 + i as u64);
            } else if i < size - running {
                row.state = State::Queued;
                row.progress.downloaded = 0;
            } else {
                row.state = State::Running;
                row.progress.downloaded = 0;
                row.progress.total = Some(1 << 30);
            }
            Arc::new(row)
        })
        .collect()
}

/// The next read: the same rows, the running ones' figures moved (new entries, as the queue
/// hands out a row it changed).
fn moved_on(rows: &[Arc<Entry>], running: usize) -> Vec<Arc<Entry>> {
    let mut next = rows.to_vec();
    let size = next.len();
    for row in &mut next[size - running..] {
        let mut copy = (**row).clone();
        copy.progress.downloaded += 1 << 16;
        copy.progress.speed = 1e6;
        *row = Arc::new(copy);
    }
    next
}

fn snapshot(rows: &[Arc<Entry>]) -> Snapshot {
    Snapshot {
        rows: rows.to_vec(),
        ..Snapshot::default()
    }
}

/// What a window's read costs beyond the queue's snapshot, each read, where `running` of `size`
/// rows changed: the session's read, the finished rows' sizes, and When done's dry look, as
/// the window does them (`Downloads::read_rows`).
fn wake_cost(size: usize, running: usize, reads: u32) -> Duration {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("f.bin");
    std::fs::write(&file, [0u8; 10]).unwrap();
    let settings = Settings::default();
    let mut session = Session::default();
    let mut sizes = SavedSizes::default();
    let mut rows = history(size, running, &file);
    // The first read seeds what follows: every row is new to it.
    let mut shown = session
        .read_from(snapshot(&rows), false, &settings)
        .snapshot
        .rows;
    sizes.apply(&mut shown);
    session.ran_dry(&shown);
    let mut spent = Duration::ZERO;
    for _ in 0..reads {
        rows = moved_on(&rows, running);
        let queue = snapshot(&rows);
        let started = Instant::now();
        let read = session.read_from(queue, false, &settings);
        let at = read.changes.at().expect("the same rows in the same order");
        assert_eq!(at.len(), running);
        for &i in at {
            shown[i] = Arc::clone(&read.snapshot.rows[i]);
        }
        sizes.apply_at(&mut shown, at);
        assert!(!session.ran_dry(&shown), "downloads still run");
        spent += started.elapsed();
    }
    spent / reads
}

/// A read where 4 of 10,000 rows changed looks at those 4, beyond a comparison of each row's
/// pointer (gaps row 181). A debug build, generous for a loaded machine: a read that looked
/// every row up in the detector, the finished rows' sizes and When done's watch took several
/// milliseconds there.
#[test]
fn a_read_of_ten_thousand_rows_where_four_changed() {
    for size in [100usize, 1_000, 10_000] {
        let each = wake_cost(size, 4, 20);
        println!("{size:>6} rows, 4 changed: a read beyond the snapshot {each:?}");
        assert!(each < Duration::from_millis(5), "{size} rows: {each:?}");
    }
}

/// Followed only where the rows changed, a read tells what a read of every row tells: the same
/// finishes and failures, the same states at the read before, and When done's watch fires at
/// the same read.
#[test]
fn a_read_of_the_rows_that_changed_tells_what_a_read_of_every_row_tells() {
    let settings = Settings::default();
    let mut session = Session::default();
    let (mut every, mut dry) = (Detector::default(), DryWatch::default());
    let template = template();
    let mut rows: Vec<Arc<Entry>> = (1..=50)
        .map(|id| {
            let mut row = template.clone();
            row.id = id;
            if id % 2 == 1 {
                row.state = State::Paused;
            }
            Arc::new(row)
        })
        .collect();
    // Two downloads (the rows at 0 and 2) started, one finished, then the other failed.
    let steps: [&[(usize, State)]; 6] = [
        &[],
        &[(0, State::Queued), (2, State::Running)],
        &[(0, State::Running), (2, State::Completed)],
        &[],
        &[(0, State::Failed("gone".into()))],
        &[(0, State::Queued)],
    ];
    let mut fired = Vec::new();
    for (step, changes) in steps.iter().enumerate() {
        for (at, state) in changes.iter() {
            let mut row = (*rows[*at]).clone();
            row.state = state.clone();
            rows[*at] = Arc::new(row);
        }
        let read = session.read_from(snapshot(&rows), false, &settings);
        assert_eq!(read.detected, every.read(&rows), "step {step}");
        if step > 0 {
            let at = changes.iter().map(|(at, _)| *at).collect();
            assert_eq!(read.changes, Changes::At(at), "step {step}");
        }
        for row in &rows {
            assert_eq!(session.before(row.id), every.before(row.id), "step {step}");
        }
        let watched = dry.look(&rows, &every.before_all());
        assert_eq!(session.ran_dry(&rows), watched, "step {step}");
        fired.push(watched);
    }
    assert_eq!(fired, [false, false, false, false, true, false]);
}
