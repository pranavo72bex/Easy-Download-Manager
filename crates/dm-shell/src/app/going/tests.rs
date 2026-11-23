// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn row(id: Id, state: State, name: &str, done: u64, total: Option<u64>, speed: f64) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id, "url": "https://e.com/files/from-the-address.bin",
            "state": serde_json::to_value(&state).unwrap(),
            "progress": {"downloaded": done, "total": total, "speed": speed, "eta": null},
            "filename": name,
        }))
        .unwrap(),
    )
}

#[test]
fn the_running_are_listed_fastest_first_and_the_rest_counted() {
    let mut rows = vec![
        row(1, State::Completed, "done.zip", 10, Some(10), 0.0),
        row(2, State::Running, "half.iso", 50, Some(100), 2_000.0),
        row(3, State::Running, "", 7, None, 9_000.0),
        row(4, State::Queued, "next.bin", 0, None, 0.0),
    ];
    for id in 5..12 {
        rows.push(row(
            id,
            State::Running,
            &format!("r{id}.bin"),
            0,
            Some(0),
            1.0,
        ));
    }
    let (going, count) = running(&rows);
    assert_eq!(count, 9);
    assert_eq!(going.len(), MOST);
    // A row with no name yet goes by its address's; a size of nothing is no percent.
    assert_eq!(
        going[0],
        Going {
            name: "from-the-address.bin".into(),
            percent: None,
            speed: 9_000,
            id: Some(3),
        }
    );
    assert_eq!(going[1].name, "half.iso");
    assert_eq!(going[1].percent, Some(50));
    assert_eq!(going[2].percent, None);
}

#[test]
fn written_every_second_while_running_once_as_they_stop_and_at_once_on_a_failure() {
    let mut publisher = Publisher::default();
    let start = Instant::now();
    let running = vec![row(1, State::Running, "a.bin", 1, Some(2), 5.0)];
    assert!(
        publisher.next(&running, &[], start, 1).is_some(),
        "the first read"
    );
    assert!(publisher
        .next(&running, &[], start + Duration::from_millis(400), 2)
        .is_none());
    let again = publisher.next(&running, &[], start + EVERY, 3).unwrap();
    assert_eq!(again.at_ms, 3, "stamped afresh while it runs");
    // It failed: said at once, and kept.
    let failed = vec![row(
        1,
        State::Failed("the server refused it".into()),
        "a.bin",
        1,
        Some(2),
        0.0,
    )];
    let said = publisher
        .next(&failed, &[1], start + EVERY + Duration::from_millis(10), 4)
        .unwrap();
    assert_eq!(said.running, 0);
    assert_eq!(
        said.failed,
        Some(LastFailure {
            name: "a.bin".into(),
            why: "the server refused it".into(),
        })
    );
    // Nothing running and nothing new: not written again, however long.
    assert!(publisher
        .next(&failed, &[], start + EVERY * 60, 5)
        .is_none());
    // One starts: at once.
    let started = vec![row(2, State::Running, "b.bin", 0, None, 0.0)];
    let now = publisher
        .next(&started, &[], start + EVERY * 60, 6)
        .unwrap();
    assert_eq!(now.running, 1);
    assert!(now.failed.is_some(), "the last failure is still said");
    // Stopped: once more, so the file no longer says it runs.
    let stopped = vec![row(2, State::Paused, "b.bin", 0, None, 0.0)];
    assert_eq!(
        publisher
            .next(
                &stopped,
                &[],
                start + EVERY * 60 + Duration::from_millis(5),
                7
            )
            .unwrap()
            .running,
        0
    );
}

#[test]
fn the_host_reads_what_was_written_and_nothing_running_once_stale_or_dm_is_gone() {
    let home = tempfile::tempdir().unwrap();
    // Nothing written: nothing running.
    assert_eq!(read(home.path(), 1_000, true), Published::default());
    let mut publisher = Publisher::default();
    let rows = vec![row(1, State::Running, "a.bin", 1, Some(4), 10.0)];
    // No browser has talked to this dm: nothing written.
    publisher.publish(home.path(), &rows, &Changes::Every, &[], || false);
    assert!(!file(home.path()).exists());
    let mut publisher = Publisher::default();
    publisher.publish(home.path(), &rows, &Changes::Every, &[], || true);
    let bytes = std::fs::read(file(home.path())).unwrap();
    assert_eq!(dm_core::saved::format_of(&bytes), FORMAT);
    let written: Published = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(written.running, 1);
    let at = written.at_ms;
    assert_eq!(read(home.path(), at + 10, true), written);
    assert_eq!(read(home.path(), at + STALE_MS + 1, true).running, 0);
    assert!(read(home.path(), at, false).downloads.is_empty());
    // Nothing left beside it.
    assert!(!file(home.path()).with_extension("json.new").exists());
}

/// Gaps row C12: the stopped first, the latest added first; then the finished and failed, the
/// latest to end first; none waiting, running or cancelled; ten at most.
#[test]
fn the_recent_are_the_stopped_then_the_latest_to_end() {
    let ended = |id: Id, state: State, name: &str, at: Option<u64>| {
        let mut entry = Entry::clone(&row(id, state, name, 1, Some(1), 0.0));
        entry.finished_at = at;
        Arc::new(entry)
    };
    let mut rows = vec![
        ended(1, State::Completed, "old.zip", Some(100)),
        ended(2, State::Paused, "paused-first.bin", None),
        ended(3, State::Failed("refused".into()), "failed.iso", Some(300)),
        ended(4, State::Running, "running.bin", None),
        ended(5, State::Queued, "waiting.bin", None),
        ended(6, State::Cancelled, "cancelled.bin", Some(400)),
        ended(7, State::Completed, "new.pdf", Some(500)),
        ended(8, State::Paused, "paused-later.bin", None),
    ];
    let names: Vec<(String, String)> = recent(&rows)
        .into_iter()
        .map(|r| (r.name, r.state))
        .collect();
    let pair = |n: &str, s: &str| (n.to_string(), s.to_string());
    assert_eq!(
        names,
        [
            pair("paused-later.bin", recent_state::STOPPED),
            pair("paused-first.bin", recent_state::STOPPED),
            pair("new.pdf", recent_state::FINISHED),
            pair("failed.iso", recent_state::FAILED),
            pair("old.zip", recent_state::FINISHED),
        ]
    );
    for id in 100..120 {
        rows.push(ended(id, State::Completed, "many.bin", Some(1_000 + id)));
    }
    let listed = recent(&rows);
    assert_eq!(listed.len(), MOST_RECENT);
    assert_eq!(listed[0].id, 8);
    assert_eq!(listed[2].id, 119);
    assert_eq!(listed[9].id, 112);
}

/// A download that finishes is written at once for the popup's recent list, though nothing runs.
#[test]
fn a_download_that_ends_is_written_for_the_recent_list() {
    let mut publisher = Publisher::default();
    let start = Instant::now();
    let running = vec![row(1, State::Running, "a.bin", 1, Some(2), 5.0)];
    assert!(publisher.next(&running, &[], start, 1).is_some());
    let done = vec![row(1, State::Completed, "a.bin", 2, Some(2), 0.0)];
    let wrote = publisher
        .next(&done, &[], start, 2)
        .expect("written as it stops");
    assert_eq!(wrote.recent.len(), 1);
    assert_eq!(wrote.recent[0].state, recent_state::FINISHED);
    // The same again: nothing new to write.
    assert!(publisher.next(&done, &[], start, 3).is_none());
    // Cleared from the list: written again, with nothing recent.
    let wrote = publisher
        .next(&[], &[], start, 4)
        .expect("written as it goes");
    assert!(wrote.recent.is_empty());
}

/// The popup's ask is the window's own command for the row, done only where it applies now.
#[test]
fn the_popups_ask_is_the_windows_own_command_where_it_applies() {
    use dm_protocol::act_does;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("done.zip");
    std::fs::write(&file, b"x").unwrap();
    let mut finished = Entry::clone(&row(1, State::Completed, "done.zip", 1, Some(1), 0.0));
    finished.path = file.to_string_lossy().into_owned();
    let rows = vec![
        Arc::new(finished),
        row(2, State::Paused, "paused.bin", 1, Some(2), 0.0),
        row(3, State::Running, "running.bin", 1, Some(2), 9.0),
        row(
            4,
            State::Failed("refused".into()),
            "failed.bin",
            1,
            Some(2),
            0.0,
        ),
    ];
    let act = |does: &str, id: Option<u64>| Act {
        does: does.into(),
        id,
    };
    let command = |does: &str, id: Option<u64>| command_for(&act(does, id), &rows).map(|(c, _)| c);
    assert_eq!(command(act_does::OPEN, Some(1)), Some(Command::OpenFile));
    assert_eq!(
        command(act_does::FOLDER, Some(1)),
        Some(Command::OpenFolder)
    );
    assert_eq!(
        command(act_does::RESUME, Some(2)),
        Some(Command::DownloadNow)
    );
    assert_eq!(
        command(act_does::RESUME, Some(4)),
        Some(Command::DownloadNow)
    );
    assert_eq!(
        command(act_does::STOP, Some(3)),
        Some(Command::StopDownload)
    );
    assert_eq!(
        command(act_does::CLEAR_FINISHED, None),
        Some(Command::DeleteCompleted)
    );
    // Where the window would grey it out: nothing is done.
    assert_eq!(command(act_does::OPEN, Some(2)), None);
    assert_eq!(command(act_does::RESUME, Some(3)), None);
    assert_eq!(command(act_does::STOP, Some(1)), None);
    assert_eq!(command(act_does::OPEN, Some(99)), None, "a row gone");
    assert_eq!(
        command_for(&act(act_does::CLEAR_FINISHED, None), &rows[1..]).map(|(c, _)| c),
        None,
        "nothing finished to clear"
    );
    // A finished file no longer on disk is not opened.
    std::fs::remove_file(&file).unwrap();
    assert_eq!(command(act_does::OPEN, Some(1)), None);
    // The row comes with the command.
    let (_, row) = command_for(&act(act_does::RESUME, Some(2)), &rows).unwrap();
    assert_eq!(row.map(|r| r.id), Some(2));
}

#[test]
fn an_inbox_name_carries_the_ask_it_was_left_under() {
    use dm_protocol::act_does;
    assert_eq!(
        asked_by("browser.open.12"),
        Some(Act {
            does: act_does::OPEN.into(),
            id: Some(12),
        })
    );
    assert_eq!(
        asked_by("browser.clear-finished"),
        Some(Act {
            does: act_does::CLEAR_FINISHED.into(),
            id: None,
        })
    );
    for other in [
        "browser.open",
        "browser.open.x",
        "browser.erase.1",
        "show",
        "a.json",
        "retry-3",
    ] {
        assert_eq!(asked_by(other), None, "{other}");
    }
}
