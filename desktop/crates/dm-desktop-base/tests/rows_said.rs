// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a row says of itself, in every window: its state's word, a retry, a stall, a refusal
//! that wants a sign-in, a file due to go. Moved from the egui window's tests
//! (`tests/app_cover.rs`).

use std::collections::HashMap;
use std::time::Instant;

use dm_core::{Entry, State};
use dm_desktop_base::rows::{kind, set_retrying, stalled_said, status_in_english};

/// A row added for later, as the queue keeps one.
fn a_row() -> Entry {
    serde_json::from_value(serde_json::json!({
        "id": 7,
        "url": "http://files.example/f.bin",
        "state": serde_json::to_value(State::Paused).unwrap(),
        "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
        "filename": "f.bin",
    }))
    .unwrap()
}

#[test]
fn every_state_has_its_word_and_every_kind_its_name() {
    let states = [
        (State::Queued, "queued"),
        (State::Running, "running"),
        (State::Paused, "paused"),
        (State::Completed, "completed"),
        (State::Failed("x".into()), "failed"),
        (State::Cancelled, "cancelled"),
    ];
    for (state, word) in states {
        assert_eq!(kind(&state), word);
    }
    use dm_shell::app::names::category_word;
    assert_eq!(category_word(dm_core::Category::Videos), "videos");
    assert_eq!(category_word(dm_core::Category::Apps), "apps");
}

#[test]
fn what_a_row_says_of_retries_stalls_sign_ins_and_a_file_due_to_go() {
    dm_desktop_base::i18n::set(dm_desktop_base::i18n::chosen("en"));
    let base = a_row();

    // Retrying, with and without an end, with and without a cause.
    let mut running = base.clone();
    running.state = State::Running;
    let retry = |attempt, of, why: &str| {
        let mut now = HashMap::new();
        now.insert(running.id, (attempt, of, why.to_string(), Instant::now()));
        set_retrying(now);
        status_in_english(&running)
    };
    assert_eq!(retry(2, 5, "something odd"), "Retrying (attempt 2 of 5)");
    assert_eq!(
        retry(3, 0, "connection reset by peer"),
        "Connection dropped (retry 3)"
    );
    assert_eq!(retry(4, 0, "something odd"), "Retrying (attempt 4)");
    set_retrying(HashMap::new());

    // Some bytes of a file of unknown length, nothing arriving now.
    running.progress.downloaded = 2048;
    running.progress.speed = 0.0;
    assert_eq!(status_in_english(&running), "2.0 KB");

    // Not failed: not a link that ran out.
    use dm_shell::app::rules::link_expired;
    assert!(!link_expired(&base));
    let mut expired = base.clone();
    expired.state = State::Failed("the server answered 403 Forbidden".into());
    expired.progress.downloaded = 10;
    assert!(link_expired(&expired));

    // Stopped by itself: a full disk names its folder where it knows one.
    let mut stalled = base.clone();
    stalled.stalled = "No space left on device (os error 28)".into();
    stalled.save_dir = Some("/Volumes/Big".into());
    assert_eq!(stalled_said(&stalled), "the disk is full in /Volumes/Big");
    stalled.save_dir = None;
    stalled.began_in = String::new();
    assert_eq!(stalled_said(&stalled), "the disk is full");
    stalled.stalled = "the drive was taken away".into();
    assert_eq!(stalled_said(&stalled), "the drive was taken away");

    // A refusal that wants a sign-in, in words dm has none plainer for.
    let mut refused = base.clone();
    refused.state = State::Failed("the ftp server said 401 not logged in".into());
    assert_eq!(
        status_in_english(&refused),
        "Error: the ftp server said 401 not logged in — it needs a sign-in: Properties › Login"
    );

    // Finished, its time to go come: "Complete" until it has gone.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("kept.bin");
    std::fs::write(&file, b"x").unwrap();
    let mut due = base.clone();
    due.state = State::Completed;
    due.path = file.display().to_string();
    due.finished_at = Some(1);
    due.delete_after = Some(1);
    assert_eq!(status_in_english(&due), "Complete");
}
