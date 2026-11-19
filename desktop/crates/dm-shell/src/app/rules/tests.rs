// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_stopped_row_resumes_a_failed_one_retries_and_a_held_one_is_asked_about() {
    assert_eq!(go_on(&State::Paused, false), GoOn::Resume);
    assert_eq!(go_on(&State::Paused, true), GoOn::Held);
    assert_eq!(go_on(&State::Failed("x".into()), false), GoOn::Retry);
    // Held says nothing of a row that is not stopped.
    assert_eq!(go_on(&State::Failed("x".into()), true), GoOn::Retry);
    assert_eq!(go_on(&State::Completed, false), GoOn::Settled);
    assert_eq!(go_on(&State::Cancelled, true), GoOn::Settled);
    assert_eq!(go_on(&State::Queued, false), GoOn::NotStopped);
    assert_eq!(go_on(&State::Running, false), GoOn::NotStopped);
}

/// `url` with a login in it, set through its parts rather than written
/// out: a login spelled in the source reads to a secret scanner as a leak.
fn with_login(url: &str, user: &str, word: &str) -> String {
    let mut parsed = url::Url::parse(url).unwrap();
    parsed.set_username(user).unwrap();
    parsed.set_password(Some(word)).unwrap();
    parsed.to_string()
}

/// A row as the queue file keeps one: the fields it needs, the rest
/// their defaults.
fn row(id: Id, url: &str, state: State, path: &str) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": url,
            "state": serde_json::to_value(&state).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "f",
            "path": path,
        }))
        .unwrap(),
    )
}

/// The same address with or without its login is one row; a cancelled
/// row is no longer in the list.
#[test]
fn an_address_is_listed_by_its_bare_form_and_not_once_cancelled() {
    let rows = vec![
        row(1, "https://a.example/f.zip", State::Cancelled, ""),
        row(
            2,
            &with_login("https://b.example/g.zip", "user", "pw"),
            State::Paused,
            "",
        ),
    ];
    assert_eq!(already_listed(&rows, "https://a.example/f.zip"), None);
    assert_eq!(already_listed(&rows, "https://b.example/g.zip"), Some(2));
}

/// A failed row, or a finished one whose file is gone, is not worth
/// keeping; a finished one whose file is there, or a stopped one, is.
#[test]
fn only_a_row_still_worth_having_is_a_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let kept = dir.path().join("kept.bin");
    std::fs::write(&kept, b"x").unwrap();
    let gone = dir.path().join("gone.bin").display().to_string();
    let rows = vec![
        row(1, "https://h.example/failed", State::Failed("x".into()), ""),
        row(2, "https://h.example/gone", State::Completed, &gone),
        row(
            3,
            "https://h.example/kept",
            State::Completed,
            &kept.display().to_string(),
        ),
        row(4, "https://h.example/stopped", State::Paused, ""),
    ];
    assert_eq!(worth_keeping(&rows, "https://h.example/failed"), None);
    assert_eq!(worth_keeping(&rows, "https://h.example/gone"), None);
    assert_eq!(worth_keeping(&rows, "https://h.example/kept"), Some(3));
    assert_eq!(worth_keeping(&rows, "https://h.example/stopped"), Some(4));
}

#[test]
fn a_kept_choice_is_read_back_from_its_word() {
    for dup in [Dup::Show, Dup::Again, Dup::Replace] {
        assert_eq!(Dup::from_word(dup.word()), Some(dup));
    }
    assert_eq!(Dup::from_word("ask"), None);
}

#[test]
fn only_a_finished_row_whose_file_is_there_can_be_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("old.zip");
    std::fs::write(&file, b"old").unwrap();
    let path = file.display().to_string();
    let done = row(1, "http://e.com/old.zip", State::Completed, &path);
    let with = replacing(&done).unwrap();
    assert_eq!(with.replaces.as_deref(), Some(path.as_str()));
    assert_eq!(
        with.save_dir.as_deref(),
        Some(&*dir.path().to_string_lossy())
    );
    assert_eq!(with.save_name.as_deref(), Some("old.zip"));
    assert!(replacing(&row(2, "http://e.com/old.zip", State::Running, &path)).is_none());
    std::fs::remove_file(&file).unwrap();
    assert!(replacing(&done).is_none(), "its file is gone");
}

#[test]
fn the_status_bar_counts_fetching_finishing_and_waiting_rows() {
    let with = |id: Id, state: State, downloaded: u64, total: Option<u64>, speed: f64| {
        Arc::new(
            serde_json::from_value::<Entry>(serde_json::json!({
                "id": id,
                "url": "http://e.com/f",
                "state": serde_json::to_value(state).unwrap(),
                "progress": {"downloaded": downloaded, "total": total, "speed": speed, "eta": null},
                "filename": "f",
                "queue": "Main queue",
            }))
            .unwrap(),
        )
    };
    let rows = vec![
        with(1, State::Running, 10, Some(100), 1000.0),
        with(2, State::Running, 5, None, 500.0),
        with(3, State::Running, 100, Some(100), 0.0),
        with(4, State::Paused, 0, None, 0.0),
        with(5, State::Paused, 40, Some(100), 0.0),
        with(6, State::Completed, 100, Some(100), 0.0),
    ];
    assert_eq!(
        tally(&rows),
        Tally {
            running: 2,
            finishing: 1,
            rate: 1500.0,
            later: 1
        }
    );
}

/// "Show it" for a download stopped as the same file (gaps row W18): the finished row of its
/// name whose file is there, else the file of its name in the folder it began in, else nothing.
#[test]
fn the_same_file_is_a_finished_row_or_a_file_in_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let kept = dir.path().join("setup.exe");
    std::fs::write(&kept, b"bytes").unwrap();
    let named = |id: Id, state: State, path: &str, began_in: &str| {
        let mut entry = (*row(id, &format!("http://e.com/{id}/setup.exe"), state, path)).clone();
        entry.filename = "setup.exe".into();
        entry.began_in = began_in.into();
        Arc::new(entry)
    };
    let folder = dir.path().display().to_string();
    let stopped = named(2, State::Failed("same".into()), "", &folder);
    let finished = named(1, State::Completed, &kept.display().to_string(), &folder);
    let categories = dm_core::Categories::default();
    assert_eq!(
        same_file_as(&[finished.clone(), stopped.clone()], 2, &categories),
        Some(SameAs::Row(1))
    );
    // Only in the folder, from before dm.
    assert_eq!(
        same_file_as(&[stopped.clone()], 2, &categories),
        Some(SameAs::File(kept.clone()))
    );
    std::fs::remove_file(&kept).unwrap();
    assert_eq!(same_file_as(&[finished, stopped], 2, &categories), None);
}
