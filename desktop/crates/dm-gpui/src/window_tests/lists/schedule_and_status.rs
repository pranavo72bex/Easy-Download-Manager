// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The schedule and the status bar: the queue run dry, rows held, what the bar and the tray say.

use super::*;

#[test]
fn when_the_queue_runs_dry_dm_chimes_and_counts_down_to_what_was_chosen() {
    let world = world();
    let mut settings = world.settings();
    settings.general.sound_on_complete = true;
    let mut h = world.harness_with(settings);
    let opened = |world: &World| world.fake.opened.lock().unwrap().clone();
    h.command("downloads.when_done.sleep");
    assert_eq!(h.view(|view| view.session.when_done), WhenDone::Sleep);

    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the countdown", |h| {
        h.view(|view| view.session.countdown.is_some())
    });
    assert!(opened(&world).contains(&"chime".to_string()));
    assert_eq!(
        h.view(|view| view.session.when_done),
        WhenDone::Nothing,
        "once only"
    );
    h.until("the countdown's words", |h| {
        h.shows_part("put the computer to sleep in")
    });
    h.click("Cancel");
    h.until("the countdown to stop", |h| {
        h.view(|view| view.session.countdown.is_none())
    });
    assert!(!opened(&world).iter().any(|o| o.starts_with("power")));

    // Again, and this time it is let happen.
    h.command("downloads.when_done.turn_off");
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the countdown's words", |h| {
        h.shows_part("shut the computer down in")
    });
    h.click("Now");
    h.until("the computer asked", |h| {
        h.fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.starts_with("power"))
    });
    assert_eq!(
        opened(&world)
            .iter()
            .filter(|o| o.starts_with("power"))
            .collect::<Vec<_>>(),
        ["power ShutDown"]
    );

    // Exit counts down as the others do, then quits (it quit the moment the last download
    // ended, with no word).
    h.command("downloads.when_done.exit");
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("Exit's countdown", |h| {
        h.view(|view| {
            view.session
                .countdown
                .is_some_and(|(action, _)| action == WhenDone::Quit)
        })
    });
    assert!(
        !h.view(|view| view.questions.quit_asked),
        "quit before its countdown"
    );
    h.view(|view| view.session.countdown = Some((WhenDone::Quit, Instant::now())));
    h.until("dm to quit", |h| h.view(|view| view.questions.quit_asked));
}

#[test]
fn the_status_bars_folder_never_covers_what_it_says() {
    let world = world();
    let deep = world.dir.path().join("a-long-folder-name-".repeat(12));
    let mut settings = world.settings();
    settings.folders.download_dir = deep;
    let mut h = world.harness_with(settings);
    h.view(|view| {
        view.status.said = Some((
            "Something long was said here, long enough to fill much of the status bar.".into(),
            Instant::now(),
        ))
    });
    h.until("what was said", |h| {
        h.shows_part("Something long was said here")
    });
    let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
    let rect = |part: &str| {
        texts["texts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["text"].as_str().is_some_and(|s| s.contains(part)))
            .map(|t| {
                let r = &t["rect"];
                [0, 1, 2, 3].map(|i| r[i].as_f64().unwrap() as f32)
            })
    };
    let words = rect("Nothing downloading").expect("the status");
    let said = rect("Something long was said").expect("what was said");
    let width = h.call("ui.windows", serde_json::Value::Null).unwrap()["main"][2]
        .as_f64()
        .unwrap() as f32
        - h.call("ui.windows", serde_json::Value::Null).unwrap()["main"][0]
            .as_f64()
            .unwrap() as f32;
    if let Some(folder) = rect("a-long-folder-name-") {
        assert!(
            folder[0] >= said[2].max(words[2]) - 0.5,
            "the folder {folder:?} is drawn over the status {said:?}"
        );
        assert!(folder[2] <= width + 0.5);
    }
}

/// A closed schedule is said in the status bar with when it opens, and a held row says why
/// it waits.
#[test]
fn the_status_bar_says_the_schedule_holds_downloads_and_until_when() {
    let world = world();
    let (settings, from) = shut_schedule(&world);
    let mut h = world.harness_with(settings);
    h.add_url(&world.url("file.zip"), "Start now");
    keep_waiting(&mut h);
    h.until("the schedule to hold the row", |h| {
        h.rows()
            .iter()
            .any(|r| r.state == State::Queued || r.paused_by_schedule)
    });
    // "01:00", or "Sun 01:00" when that is tomorrow.
    h.until("the status bar to say the schedule holds downloads", |h| {
        h.texts().iter().any(|t| {
            t.contains("waiting for the schedule (opens at ") && t.contains(&format!("{from})"))
        })
    });
    let settings = kept(&mut h);
    let why = dm_desktop_base::held::held_because(&settings, None).unwrap_or_default();
    assert!(
        why.contains("starts by itself at ") && why.contains(&from),
        "{why}"
    );
}

/// Resume on a row the schedule holds offers to start it now, outside the schedule, and it
/// then downloads while the schedule stays shut.
#[test]
fn resume_on_a_held_row_offers_to_start_it_outside_the_schedule() {
    let world = world();
    let (settings, _) = shut_schedule(&world);
    let mut h = world.harness_with(settings);
    h.add_url(&world.url("file.zip"), "Start now");
    // Start now under a shut schedule asks at once; kept waiting here, to be asked again by
    // Resume.
    keep_waiting(&mut h);
    h.until("the schedule to hold it", |h| {
        h.rows().iter().any(|r| r.paused_by_schedule)
    });
    select(&mut h, "file.zip");
    h.command("file.download_now");
    h.until("Resume on a held row to ask", |h| {
        h.shows("Start outside the schedule?")
    });
    // The question's Start now, which a click finds before the toolbar's of the same words.
    h.click("Start now");
    h.until("it to finish outside the schedule", |h| {
        h.rows().iter().all(|r| r.state == State::Completed)
    });
    // Nothing is said about waiting once nothing waits.
    h.until("no word of waiting", |h| {
        !h.shows_part("waiting for the schedule")
    });
}

/// The tray says how many downloads run and how fast; nothing to say when none do.
#[test]
fn the_tray_says_how_many_downloads_run_and_how_fast() {
    let world = world();
    let mut h = world.harness();
    let said = |h: &mut Harness| h.view(|view| dm_desktop_base::tray::activity(&view.rows));
    assert_eq!(said(&mut h), None);
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the download to run", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    let said = said(&mut h).unwrap_or_default();
    assert!(said.starts_with("1 downloading at "), "{said}");
}
