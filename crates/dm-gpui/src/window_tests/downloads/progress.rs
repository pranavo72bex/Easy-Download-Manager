// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The progress window and the complete window: what they show, and when they close.

use super::*;

/// A download's progress window limits that download alone: ticked, it takes the KB/s shown;
/// unticked, the limit goes.
#[test]
fn the_progress_window_limits_its_own_download() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the row", |h| h.rows().len() == 1);
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    // Its line opens to the switch (gaps row E9).
    h.until_shown("No limit");
    h.click("Speed limit");
    h.until_shown("Limit this download to");
    h.click("Limit this download to");
    h.until("the limit", |h| h.rows()[0].speed_limit == 500);
    h.until_shown("500 KB/s");
    h.click("Limit this download to");
    h.until("the limit to go", |h| h.rows()[0].speed_limit == 0);
}

/// The progress window's Cancel download asks first; Keep downloading (and Enter) leaves it
/// running.
#[test]
fn cancel_download_in_the_progress_window_asks_first() {
    let world = world();
    let mut h = world.harness();
    h.view(|view| view.backend.add(&world.url("stalled.bin")).unwrap());
    h.until("it to run", running);
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    h.until_shown("Cancel download");
    h.click("Cancel download");
    h.until_shown("Cancel this download?");
    h.key("enter", &[]);
    h.until("the question to go", |h| !h.shows("Cancel this download?"));
    assert_ne!(h.rows()[0].state, State::Cancelled, "Enter cancelled it");
    h.click("Cancel download");
    h.until_shown("Yes, cancel");
    h.click("Yes, cancel");
    h.until("it to be cancelled", |h| {
        h.rows()[0].state == State::Cancelled
    });
}

#[test]
fn a_progress_window_shows_each_connection() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.general.show_progress = true));
    h.add_url(&world.url("stalled.bin"), "Start now");
    // Running comes a moment before the plan is written; until then the window says "Unknown
    // until the server answers", which is right.
    h.until("the download to be planned", |h| {
        h.view(|view| {
            view.rows
                .iter()
                .any(|r| r.state == State::Running && view.backend.resumable(r.id).is_some())
        })
    });
    h.until("its progress window", |h| {
        h.dialogs().iter().any(|d| d.contains("stalled.bin"))
    });
    h.until_shown("Can resume");
    // That server does not serve ranges, and the window says so rather than guessing "Yes"
    // from a known size.
    h.until("the answer", |h| {
        h.texts()
            .iter()
            .any(|t| t.contains("No — a stop starts it again"))
    });
    assert!(h.shows("Connections"), "{:?}", h.texts());
    h.click("Stop download");
    h.until("it to stop", |h| h.rows()[0].state == State::Paused);
}

/// A command run on each finished file, its path added as one argument, whatever characters
/// the name has.
#[cfg(unix)]
#[test]
fn a_finished_download_is_handed_to_the_command_set_to_run_after() {
    use std::os::unix::fs::PermissionsExt;
    let world = world();
    let scratch = tempfile::tempdir().unwrap();
    let record = scratch.path().join("checked");
    let script = scratch.path().join("checker with space");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s|%s' \"$1\" \"$2\" > '{}'\n",
            record.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut h = world.harness_with(
        world.settings_with(|s| s.downloads.on_complete = format!("'{}' --scan", script.display())),
    );
    // Added as a person adds it: a script's click a moment before it finished holds the
    // command back.
    h.view(|view| view.backend.add(&world.url("file.zip")).unwrap());
    h.until("the download to finish", |h| completed(h) == 1);
    let path = h.rows()[0].path.clone();
    h.until_within(
        "the command to be run on the file",
        Duration::from_secs(10),
        &mut |_| std::fs::read_to_string(&record).is_ok_and(|r| !r.is_empty()),
    );
    assert_eq!(
        std::fs::read_to_string(&record).unwrap(),
        format!("--scan|{path}"),
        "the command was not run on the file"
    );
}

/// Several downloads finishing make one "Download complete" window listing each, with its own
/// Open and Open folder.
#[test]
fn downloads_finishing_together_share_one_complete_window() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.general.show_complete = true));
    let urls = vec![
        world.url("file.zip"),
        world.url("other.pdf"),
        world.url("deep.pdf"),
    ];
    h.update(|view, cx| view.add(urls, Default::default(), cx))
        .unwrap();
    h.until("all three to finish", |h| completed(h) == 3);
    h.until_shown("3 downloads finished.");
    let completes = h
        .dialogs()
        .iter()
        .filter(|d| *d == "Download complete")
        .count();
    assert_eq!(completes, 1);
    // Each with its own Open: the one pressed opens its file and leaves the window, the rest
    // stay.
    h.click_as_person("Open", 0);
    h.until("a file to be opened", |h| !opened(h).is_empty());
    let first = opened(&h)[0].clone();
    assert!(
        h.rows().iter().any(|r| r.path == first),
        "{first} is no row's file"
    );
    h.until("the rest to stay", |h| h.shows("2 downloads finished."));
    h.click("Open folder");
    h.until("a folder to be shown", |h| opened(h).len() == 2);
    assert!(opened(&h)[1].starts_with("reveal "));
    h.click("Close");
    h.until("the window to close", |h| {
        !h.dialog_open("Download complete")
    });
}

/// Download complete can be turned off from itself ("Don't show this window again"), and offers
/// Open and Open folder for each file; Open with… is on the row's menu (unslop review).
#[test]
fn download_complete_offers_open_and_open_folder_and_not_again() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.general.show_complete = true));
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    h.until("Download complete", |h| h.dialog_open("Download complete"));
    assert!(h.shows("Open folder"), "{:?}", h.texts());
    assert!(!h.shows("Open with…"), "{:?}", h.texts());
    // Ticking marks the box and leaves the window open; nothing is saved yet.
    h.click("Don't show this window again");
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(h.dialog_open("Download complete"));
    assert!(
        h.view(|view| view.backend.settings()).general.show_complete,
        "saved before it was closed"
    );
    // Closing the window saves it.
    h.click("Close");
    h.until("the window to close", |h| {
        !h.dialog_open("Download complete")
    });
    assert!(
        !h.view(|view| view.backend.settings()).general.show_complete,
        "the setting stays on"
    );
    // The next download that finishes opens nothing.
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("the second download to complete", |h| completed(h) == 2);
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert!(!h.dialog_open("Download complete"));
}

/// A download of known size that has no rate to go on says "Calculating…" in Time left, not
/// nothing.
#[test]
fn time_left_says_calculating_until_there_is_a_rate() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("to run", running);
    h.until_shown("Calculating…");
}

/// A finished row says when it finished, beside "Complete", so a file from last week is found
/// by its day.
#[test]
fn a_finished_row_says_when_it_finished() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("it to finish", |h| completed(h) == 1);
    let finished = h.row("file.zip").finished_at;
    let when = dm_desktop_base::format::when(finished);
    assert!(!when.is_empty(), "a finished row has no finishing time");
    h.until_shown(&when);
    assert!(h.shows("Complete"), "Complete is no longer on its own");
}

/// With the progress window off, a started download says once, ever, that its row's Details
/// shows its progress; with something else being said, it waits its turn.
#[test]
fn a_started_download_says_once_where_its_progress_is() {
    let world = world();
    let mut h = world.harness_with(Settings {
        hinted_details: false,
        ..world.settings()
    });
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("it to run", running);
    h.until("the hint", |h| h.said().contains("Double-click a download"));
    assert!(h.view(|view| view.backend.settings()).hinted_details);
    // Said, so not said again.
    h.view(|view| {
        view.status.said = None;
        view.details_hint();
    });
    assert!(h.said().is_empty(), "said a second time");
}

/// The progress window of a download that finishes goes with it, whether or not the complete
/// window is on (it is not, by default): left open it said "Complete" with no button to press,
/// one more for each download after it.
#[test]
fn the_progress_window_of_a_finished_download_closes() {
    let world = world();
    let mut h = world.harness();
    assert!(
        !world.settings().general.show_complete,
        "this checks the default, where there is no complete window"
    );
    h.view(|view| view.backend.add(&world.url("stalled.bin")).unwrap());
    h.until("it to run", running);
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    h.until("its progress window", |h| h.dialogs().len() == 1);
    // A fresh address that finishes it.
    h.view(|view| view.backend.set_url(id, &world.url("file.zip")).unwrap());
    h.until("it to finish", |h| completed(h) == 1);
    // (A new address can ask about the name its server sent: that question is not this
    // test's.)
    h.until("its progress window to close", |h| {
        h.dialogs().iter().all(|d| d == "Name sent by the server")
    });
}
