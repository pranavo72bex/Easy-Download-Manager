// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn row(state: State, held: bool) -> Entry {
    serde_json::from_value(serde_json::json!({
        "id": 1,
        "url": "http://e.com/f",
        "state": serde_json::to_value(state).unwrap(),
        "progress": {"downloaded": 10, "total": 100, "speed": 0.0, "eta": null},
        "filename": "f",
        "paused_by_schedule": held,
    }))
    .unwrap()
}

/// Where a window looks for files off its thread, a finished row says
/// "File missing" from what the look found, not from the disk as it is
/// drawn; until it is looked for again, and for its own path only.
#[test]
fn a_missing_file_is_said_from_the_last_look_not_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.bin");
    std::fs::write(&path, b"x").unwrap();
    let mut finished = row(State::Completed, false);
    finished.path = path.display().to_string();
    set_looked(Vec::new());
    assert!(!gone(&finished));
    // Deleted: not said until it is looked for.
    std::fs::remove_file(&path).unwrap();
    assert!(!gone(&finished), "the disk was asked as the row was drawn");
    let found = look_for(vec![(1, finished.path.clone())]);
    assert!(set_looked(found.clone()), "a change of word is told");
    assert!(gone(&finished));
    assert!(!set_looked(found), "the same look again changes nothing");
    // Moved since: its new place is not the one found missing.
    let mut moved = finished.clone();
    moved.path = dir.path().join("g.bin").display().to_string();
    assert!(!gone(&moved));
    // Back: said so at the next look.
    std::fs::write(&path, b"x").unwrap();
    assert!(set_looked(look_for(vec![(1, finished.path.clone())])));
    assert!(!gone(&finished));
    LOOKED.with(|l| *l.borrow_mut() = None);
}

/// A row held by the download limit or the schedule says until when (gaps row U7).
#[test]
fn a_held_row_says_until_when() {
    use chrono::Datelike;
    let held = row(State::Paused, true);
    set_limit_holds(true);
    set_hold_times(
        Some(std::time::Duration::from_secs(90)),
        &dm_core::Schedule::default(),
    );
    let said = status_in_english(&held);
    assert!(
        said.starts_with("Waiting for the download limit · more in "),
        "{said}"
    );
    set_limit_holds(false);
    // Open only three days on, an hour after midnight: closed now.
    let today = chrono::Local::now().weekday().num_days_from_sunday();
    let schedule = dm_core::Schedule {
        enabled: true,
        from: "01:00".into(),
        to: "02:00".into(),
        days: vec![((today + 3) % 7) as u8],
        ..Default::default()
    };
    set_hold_times(None, &schedule);
    let said = status_in_english(&held);
    assert!(
        said.starts_with("Waiting for the schedule · starts "),
        "{said}"
    );
    assert!(said.ends_with(" 01:00"), "{said}");
    // Where nothing says until when, the words as before.
    set_hold_times(None, &dm_core::Schedule::default());
    assert_eq!(status_in_english(&held), "Waiting for the schedule");
}

/// An estimated size is shown as one while the download runs (gaps row Y22).
#[test]
fn an_estimated_size_is_shown_as_about() {
    let mut running = row(State::Running, false);
    running.progress.total = Some(10 * 1024 * 1024);
    running.progress.estimated = true;
    assert!(size_of(&running).starts_with("≈ "), "{}", size_of(&running));
    running.progress.estimated = false;
    assert!(!size_of(&running).starts_with('≈'));
}

/// A download's note follows its state, its number kept (gaps rows Y19, Y23).
#[test]
fn a_downloads_note_follows_its_state() {
    let mut done = row(State::Completed, false);
    done.said = dm_core::NOTES[0].into();
    assert_eq!(
        status_in_english(&done),
        format!("Complete · {}", dm_core::NOTES[0])
    );
    done.said = dm_core::NOTES[1].replace("{}", "3");
    assert!(status_in_english(&done).ends_with(&dm_core::NOTES[1].replace("{}", "3")));
    done.said = "something no template says".into();
    assert_eq!(status_in_english(&done), "Complete");
}

#[test]
fn resume_says_retry_for_failures_and_start_now_for_held_rows() {
    let resume = dm_shell::app::commands::Command::DownloadNow.label();
    let failed = row(State::Failed("gone".into()), false);
    let held = row(State::Paused, true);
    let paused = row(State::Paused, false);
    assert_eq!(resume_word(&[]), resume);
    assert_eq!(resume_word(&[&failed]), "Retry");
    assert_eq!(resume_word(&[&held]), "Start now");
    assert_eq!(resume_word(&[&paused]), resume);
    assert_eq!(resume_word(&[&failed, &held]), resume, "a mix is resumed");
}

#[test]
fn an_empty_list_offers_the_next_step_only_while_nothing_is_listed() {
    let (heading, line, offered) = empty_words(false, true);
    assert_eq!(heading, "Nothing queued");
    assert_eq!(
        line,
        "Add URL, paste a link, or send one from your browser."
    );
    assert!(offered);
    assert_eq!(
        empty_words(true, true),
        (
            "Nothing here",
            "No download here matches the filter.",
            false
        )
    );
    assert_eq!(
        empty_words(true, false),
        ("Nothing here", "Nothing here yet.", false)
    );
}

/// A stopped row with nothing yet, held by the schedule where `by_schedule`.
fn paused(by_schedule: bool) -> Entry {
    let mut row = row(State::Paused, by_schedule);
    row.url = "https://e.com/f.bin".into();
    row.filename = "f.bin".into();
    row.progress.downloaded = 0;
    row.progress.total = None;
    row
}

#[test]
fn a_sign_in_is_asked_for_by_a_401_alone() {
    let failed = |said: &str| row(State::Failed(said.into()), false);
    assert!(needs_sign_in(&failed(
        "the server answered 401 Unauthorized for x"
    )));
    assert!(!needs_sign_in(&failed(
        "the server answered 404 Not Found for /4013"
    )));
    let mut refused = failed("words that name no status");
    refused.fail_kind = Some(dm_core::FailKind::Refused(403));
    assert!(needs_sign_in(&refused));
    assert!(!needs_sign_in(&row(State::Queued, false)));
}

#[test]
fn a_row_held_by_the_schedule_or_the_limit_is_waiting_not_stopped() {
    // Saying what it waits for.
    set_limit_holds(false);
    assert_eq!(status_in_english(&paused(true)), "Waiting for the schedule");
    set_limit_holds(true);
    assert_eq!(
        status_in_english(&paused(true)),
        "Waiting for the download limit"
    );
    set_limit_holds(false);
    assert_eq!(status_in_english(&paused(false)), "Stopped");
    let mut part_way = paused(false);
    part_way.progress.downloaded = 45;
    part_way.progress.total = Some(100);
    assert_eq!(status_in_english(&part_way), "Stopped · 45%");
}

#[test]
fn a_download_of_unknown_length_says_what_has_arrived_and_how_fast() {
    let mut row = paused(false);
    row.state = State::Running;
    assert_eq!(status_in_english(&row), "Starting…");
    row.progress.downloaded = 3 * dm_core::KB * dm_core::KB;
    row.progress.speed = 2.0 * dm_core::KB as f64;
    assert_eq!(status_in_english(&row), "3.0 MB · 2.0 KB/s");
    // Nothing arriving just now: what has arrived alone.
    row.progress.speed = 0.0;
    assert_eq!(status_in_english(&row), "3.0 MB");
    // A torrent with nothing yet is finding its peers.
    let mut magnet = paused(false);
    magnet.state = State::Running;
    magnet.url = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into();
    assert_eq!(status_in_english(&magnet), "Finding peers…");
    // Known, but nothing arriving: waiting for peers.
    magnet.progress.total = Some(100);
    magnet.progress.downloaded = 10;
    assert_eq!(status_in_english(&magnet), "10% · waiting for peers");
    magnet.progress.speed = 5000.0;
    assert_eq!(status_in_english(&magnet), "10%");
    // Every byte in, its parts being joined: said so, not "100%".
    let mut joining = paused(false);
    joining.state = State::Running;
    joining.progress.total = Some(100);
    joining.progress.downloaded = 100;
    assert_eq!(status_in_english(&joining), "Joining parts…");
}

/// A row whose file another, later row wrote again is
/// gone, not Complete; the later one is not.
#[test]
fn a_file_written_again_by_a_later_row_is_that_rows() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("same.bin");
    std::fs::write(&file, b"x").unwrap();
    let row = |id: Id, at: u64| {
        let mut row = paused(false);
        row.id = id;
        row.state = State::Completed;
        row.path = file.display().to_string();
        row.finished_at = Some(at);
        row
    };
    let (old, new) = (row(1, 10), row(2, 20));
    set_superseded(&[old.clone(), new.clone()]);
    assert!(gone(&old));
    assert!(!gone(&new));
    // Finished in the same second: the later row's.
    let twin = row(3, 20);
    set_superseded(&[new.clone(), twin.clone()]);
    assert!(gone(&new) && !gone(&twin));
    set_superseded(std::slice::from_ref(&old));
    assert!(!gone(&old), "alone, its file is its own");
}

/// A video page that failed says why after the sentence that was all a
/// person had; the specific failures keep their own words.
#[test]
fn a_video_page_that_failed_says_why_after_the_sentence() {
    let mut row = paused(false);
    row.state = State::Failed("no stream at that quality".into());
    assert_eq!(
        status_in_english(&row),
        "Error: dm couldn't get the video from this page (no stream at that quality)"
    );
    // A failure with its own plain words is not given a second one.
    row.state = State::Failed("YouTube says: Video unavailable".into());
    assert_eq!(
        status_in_english(&row),
        "Error: this video isn't available. It may be private, removed, or not shown in your country"
    );
}

/// A full disk says whose, the folder the file was going to.
#[test]
fn a_full_disk_says_which_folder() {
    let mut row = paused(false);
    row.state = State::Failed("disk: No space left on device (os error 28)".into());
    row.began_in = "/media/usb/Downloads".into();
    assert_eq!(
        status_in_english(&row),
        "Error: the disk is full in /media/usb/Downloads"
    );
    row.save_dir = Some("/mnt/big".into());
    assert_eq!(
        status_in_english(&row),
        "Error: the disk is full in /mnt/big"
    );
    row.save_dir = None;
    row.began_in.clear();
    assert_eq!(status_in_english(&row), "Error: the disk is full");
}
