// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_finished_row_takes_its_files_size_on_disk_read_once_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("joined.mp4");
    std::fs::write(&path, vec![0u8; 700]).unwrap();
    let row = |state: dm_core::State| -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": 1,
                "url": "http://example.com/v",
                "state": serde_json::to_value(state).unwrap(),
                "progress": {"downloaded": 900, "total": 900, "speed": 0.0, "eta": null},
                "filename": "joined.mp4",
                "path": path.display().to_string(),
            }))
            .unwrap(),
        )
    };
    let mut sizes = SavedSizes::default();
    let mut running = vec![row(dm_core::State::Running)];
    sizes.apply(&mut running);
    assert_eq!(
        running[0].progress.total,
        Some(900),
        "not finished: as counted"
    );
    let mut rows = vec![row(dm_core::State::Completed)];
    sizes.apply(&mut rows);
    assert_eq!(
        (rows[0].progress.downloaded, rows[0].progress.total),
        (700, Some(700))
    );
    // Read once: a change on disk is not seen until the sizes are cleared.
    std::fs::write(&path, vec![0u8; 1000]).unwrap();
    let mut again = vec![row(dm_core::State::Completed)];
    sizes.apply(&mut again);
    assert_eq!(again[0].progress.total, Some(700));
    sizes.clear();
    sizes.apply(&mut again);
    assert_eq!(again[0].progress.total, Some(1000));
    // The same figures: the row is not copied.
    let before = Arc::clone(&again[0]);
    sizes.apply(&mut again);
    assert!(Arc::ptr_eq(&before, &again[0]));
    // The queue's row, read again unchanged, is given the same copy:
    // a read that changed nothing hands out the same rows.
    let queues = row(dm_core::State::Completed);
    let mut first = vec![Arc::clone(&queues)];
    sizes.apply(&mut first);
    let mut second = vec![Arc::clone(&queues)];
    sizes.apply(&mut second);
    assert_eq!(second[0].progress.total, Some(1000));
    assert!(Arc::ptr_eq(&first[0], &second[0]));
}

/// A read costs each finished row once, not once per row of the list:
/// ten thousand finished rows were about 10^8 comparisons a read (gaps
/// row 181), seconds in a debug build; now a few milliseconds.
#[test]
fn ten_thousand_finished_rows_are_read_in_linear_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.bin");
    std::fs::write(&path, vec![0u8; 10]).unwrap();
    let mut rows: Vec<Arc<Entry>> = (1..=10_000u64)
        .map(|id| {
            Arc::new(
                serde_json::from_value(serde_json::json!({
                    "id": id,
                    "url": "http://example.com/f",
                    "state": serde_json::to_value(dm_core::State::Completed).unwrap(),
                    "progress": {"downloaded": 10, "total": 10, "speed": 0.0, "eta": null},
                    "filename": "f.bin",
                    "path": path.display().to_string(),
                }))
                .unwrap(),
            )
        })
        .collect();
    let mut sizes = SavedSizes::default();
    sizes.apply(&mut rows);
    let started = std::time::Instant::now();
    for _ in 0..5 {
        sizes.apply(&mut rows);
    }
    let each = started.elapsed() / 5;
    assert!(
        each < std::time::Duration::from_millis(100),
        "a read of 10,000 finished rows took {each:?}"
    );
}

fn finished(id: Id, path: &std::path::Path) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": "http://example.com/f",
            "state": serde_json::to_value(dm_core::State::Completed).unwrap(),
            "progress": {"downloaded": 10, "total": 10, "speed": 0.0, "eta": null},
            "filename": "f.bin",
            "path": path.display().to_string(),
        }))
        .unwrap(),
    )
}

/// What a read changed is found by pointer: the same entries are nothing, another entry in a
/// place is that place, and rows that came, went or moved are every row (gaps row 181).
#[test]
fn what_a_read_changed_is_found_by_pointer() {
    let path = std::path::Path::new("f.bin");
    let rows: Vec<Arc<Entry>> = (1..=4).map(|id| finished(id, path)).collect();
    assert_eq!(Changes::between(&rows, &rows.clone()), Changes::At(vec![]));
    let mut one = rows.clone();
    one[2] = Arc::new((*one[2]).clone());
    assert_eq!(Changes::between(&rows, &one), Changes::At(vec![2]));
    let mut moved = rows.clone();
    moved.swap(0, 3);
    assert_eq!(Changes::between(&rows, &moved), Changes::Every);
    assert_eq!(Changes::between(&rows, &rows[1..]), Changes::Every);
    assert_eq!(Changes::between(&[], &rows), Changes::Every);
}

/// Where a few rows changed, only they are given their sizes, as a read of every row gives
/// them; a finished row whose file was missing is looked for again at each read and given its
/// size once it is there; a row no longer finished is forgotten.
#[test]
fn only_the_rows_that_changed_are_given_their_sizes_and_missing_files_are_looked_for_again() {
    let dir = tempfile::tempdir().unwrap();
    let here = dir.path().join("here.bin");
    std::fs::write(&here, [0u8; 7]).unwrap();
    let later = dir.path().join("later.bin");
    let queue = vec![finished(1, &here), finished(2, &later), finished(3, &here)];
    let mut sizes = SavedSizes::default();
    let mut shown = queue.clone();
    sizes.apply(&mut shown);
    assert_eq!(shown[0].progress.total, Some(7));
    assert!(Arc::ptr_eq(&shown[1], &queue[1]), "its file is missing");
    // Nothing changed and the file is still missing: nothing found.
    assert!(sizes.apply_at(&mut shown, &[]).is_empty());
    // The third row changed (as the queue copies a row it changes).
    let changed = Arc::new((*queue[2]).clone());
    shown[2] = Arc::clone(&changed);
    assert!(sizes.apply_at(&mut shown, &[2]).is_empty());
    assert_eq!(shown[2].progress.total, Some(7));
    // The missing file arrives: found at the next read, though its row did not change.
    std::fs::write(&later, [0u8; 3]).unwrap();
    assert_eq!(sizes.apply_at(&mut shown, &[]), vec![1]);
    assert_eq!(shown[1].progress.total, Some(3));
    assert!(sizes.apply_at(&mut shown, &[]).is_empty(), "found once");
    // The first row is downloaded again: its size is forgotten, and read again once finished.
    let mut again = (*queue[0]).clone();
    again.state = dm_core::State::Running;
    shown[0] = Arc::new(again);
    sizes.apply_at(&mut shown, &[0]);
    assert_eq!(shown[0].progress.total, Some(10), "as counted while it runs");
    std::fs::write(&here, [0u8; 9]).unwrap();
    shown[0] = finished(1, &here);
    sizes.apply_at(&mut shown, &[0]);
    assert_eq!(shown[0].progress.total, Some(9));
}

/// A read of 10,000 finished rows where four changed looks at those four, not at every row
/// (gaps row 181): well under a millisecond, where a read of every row takes several in a
/// debug build.
#[test]
fn ten_thousand_finished_rows_where_four_changed_are_read_in_time_of_the_four() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.bin");
    std::fs::write(&path, [0u8; 10]).unwrap();
    let mut rows: Vec<Arc<Entry>> = (1..=10_000u64).map(|id| finished(id, &path)).collect();
    let mut sizes = SavedSizes::default();
    sizes.apply(&mut rows);
    let at = [10usize, 2_000, 5_000, 9_999];
    let started = std::time::Instant::now();
    for _ in 0..20 {
        for &i in &at {
            rows[i] = Arc::new((*rows[i]).clone());
        }
        sizes.apply_at(&mut rows, &at);
    }
    let each = started.elapsed() / 20;
    assert!(
        each < std::time::Duration::from_millis(2),
        "a read of 10,000 finished rows, 4 changed, took {each:?}"
    );
}
