use crate::support::*;

/// "When done" waited for nothing Running or Queued, so a queue a
/// person stopped, or one the schedule had put down at the window's close,
/// counted as done — dm quit (or the computer slept) with the download half
/// fetched and nothing to resume it.
#[test]
fn when_done_does_not_fire_on_a_stopped_or_schedule_held_queue() {
    use chrono::Datelike;
    use dm_desktop::power::WhenDone;
    let world = world();
    let mut harness = world.harness();

    // Stop all: nothing finished, so nothing is done; the choice stays.
    harness.state_mut().run(Command::WhenDoneQuit);
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "the download to start", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    harness.state_mut().run(Command::StopAll);
    until(&mut harness, "the download to be stopped", |app| {
        app.rows().iter().all(|r| r.state == State::Paused)
    });
    for _ in 0..10 {
        harness.state_mut().refresh();
        harness.step();
    }
    assert!(
        !harness.state().quit_asked,
        "quit on a queue a person stopped"
    );
    assert_eq!(harness.state().when_done, WhenDone::Quit);

    // Something else finishes; the stopped row is left alone, not waited for.
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    quit_after_its_countdown(&mut harness);
    harness.state_mut().quit_asked = false;

    // The schedule's window closes on a running download: it is held, not
    // done, and dm stays to resume it when the window reopens.
    harness.state_mut().run(Command::WhenDoneSleep);
    let stalled = harness.state().rows()[0].id;
    harness.state().backend().resume(stalled);
    // Another finishes meanwhile, so "something finished" holds as well.
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "one to finish and one to run", |app| {
        app.row(stalled).is_some_and(|r| r.state == State::Running)
            && app
                .rows()
                .iter()
                .filter(|r| r.state == State::Completed)
                .count()
                == 2
    });
    let tomorrow = (chrono::Local::now().weekday().num_days_from_sunday() as u8 + 1) % 7;
    harness
        .state()
        .backend()
        .queue
        .set_schedule(dm_core::Schedule {
            wake: false,
            enabled: true,
            from: "00:00".into(),
            to: "23:59".into(),
            days: vec![tomorrow],
        });
    until(&mut harness, "the schedule to hold the download", |app| {
        app.row(stalled)
            .is_some_and(|r| r.state == State::Paused && r.paused_by_schedule)
    });
    for _ in 0..10 {
        harness.state_mut().refresh();
        harness.step();
    }
    assert!(!harness.state().quit_asked);
    assert!(
        harness.state().countdown.is_none(),
        "counted down to sleep with a download the schedule holds"
    );
    assert_eq!(harness.state().when_done, WhenDone::Sleep);
}

#[test]
fn a_sleep_the_system_refuses_is_said_in_the_problems() {
    let world = world();
    *world.fake.power_fails.lock().unwrap() = true;
    let mut harness = world.harness();
    harness.state_mut().countdown = Some((
        dm_desktop::power::WhenDone::Sleep,
        std::time::Instant::now(),
    ));
    harness.state_mut().countdown_tick();
    until(&mut harness, "the refusal in the problems", |app| {
        app.problems
            .iter()
            .any(|p| p.contains("could not put the computer to sleep"))
    });
}

#[test]
fn the_scheduler_refuses_a_time_that_is_not_one_and_says_why() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::Scheduler);
    harness.run_steps(2);
    if let Some((schedule, _)) = harness.state_mut().dialogs.scheduler.as_mut() {
        schedule.enabled = true;
        schedule.from = "25:00".into();
        schedule.to = "1am".into();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.scheduler.is_some(),
        "closed as if taken"
    );
    assert!(harness
        .query_by_label_contains("times are HH:MM in 24 hours")
        .is_some());
    assert!(!harness.state().backend().settings().schedule.enabled);

    // No day ticked is refused where it was unticked: every day is shown,
    // and kept, as seven ticks.
    if let Some((schedule, _)) = harness.state_mut().dialogs.scheduler.as_mut() {
        schedule.from = "01:00".into();
        schedule.to = "07:00".into();
        schedule.days.clear();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness.query_by_label("Tick at least one day.").is_some());
    assert!(!harness.state().backend().settings().schedule.enabled);
    if let Some((schedule, _)) = harness.state_mut().dialogs.scheduler.as_mut() {
        schedule.days = (0..=6).collect();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    let held = harness.state().backend().settings().schedule;
    assert!(held.enabled);
    assert!(held.open_at(0, 2 * 60) && held.open_at(3, 2 * 60));
    let mut back = harness.state().backend().settings();
    back.schedule.enabled = false;
    harness.state_mut().apply_settings(back).unwrap();
}

#[test]
fn the_speed_limiter_turns_off_and_back_on_to_its_last_value() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::SpeedLimiterToggle);
    assert_eq!(harness.state().backend().settings().speed_limit, 512);
    harness.state_mut().run(Command::SpeedLimiterToggle);
    assert_eq!(harness.state().backend().settings().speed_limit, 0);
    harness.state_mut().run(Command::SpeedLimiterToggle);
    assert_eq!(harness.state().backend().settings().speed_limit, 512);
}

#[test]
fn the_download_limit_is_set_in_the_speed_limiter_and_reaches_the_queue() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::SpeedLimiterSettings);
    harness.run_steps(3);
    assert!(harness
        .query_by_label("Download at most this many MB (0 for no limit)")
        .is_some());
    assert!(harness
        .query_by_label("in each period of this many hours")
        .is_some());
    if let Some(limiter) = harness.state_mut().dialogs.limiter.as_mut() {
        limiter.2 = 500;
        limiter.3 = 4;
    }
    harness.get_by_label("OK").click();
    harness.run_steps(3);
    let settings = harness.state().backend().settings();
    assert_eq!((settings.limit_mb, settings.limit_hours), (500, 4));
    assert!(
        harness.state().backend().queue.allowance_spent().is_none(),
        "nothing downloaded yet"
    );
    assert!(harness.state().backend().queue.may_run());
    // Put back, so the settings file the other tests read has no limit.
    if let Some(limiter) = {
        harness.state_mut().run(Command::SpeedLimiterSettings);
        harness.state_mut().dialogs.limiter.as_mut()
    } {
        limiter.2 = 0;
    }
    harness.run_steps(3);
    harness.get_by_label("OK").click();
    harness.run_steps(3);
    assert_eq!(harness.state().backend().settings().limit_mb, 0);
}

#[test]
fn the_weekly_update_check_asks_nothing_when_off_or_not_yet_due() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(3);
    assert_eq!(
        *harness.state().update.lock().unwrap(),
        dm_desktop::update::Update::NotAsked,
        "off by default: nothing is asked"
    );
    // On, but checked a moment ago: not due for a week.
    let mut wanted = harness.state().applied.clone();
    wanted.check_updates = true;
    wanted.update_checked = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    harness.state_mut().apply_settings(wanted).unwrap();
    harness.run_steps(3);
    assert_eq!(
        *harness.state().update.lock().unwrap(),
        dm_desktop::update::Update::NotAsked
    );
    let mut off = harness.state().applied.clone();
    off.check_updates = false;
    harness.state_mut().apply_settings(off).unwrap();
}

#[test]
fn the_speed_limit_last_turned_off_comes_back_after_a_restart() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let mut wanted = harness.state().applied.clone();
    wanted.speed_limit = 300;
    harness.state_mut().apply_settings(wanted).unwrap();
    harness.state_mut().run(Command::SpeedLimiterToggle);
    let (saved, _) = Settings::load();
    assert_eq!((saved.speed_limit, saved.last_speed_limit), (0, 300));
    // The next launch, from what was saved.
    let mut next = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app_with(saved));
    next.state_mut().run(Command::SpeedLimiterToggle);
    assert_eq!(next.state().backend().settings().speed_limit, 300);
}

/// Exit while downloading asks first; a script's exit does not.
#[test]
fn exit_while_downloading_asks_first() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "it to run", |app| app.downloading() > 0);
    harness.state_mut().run(Command::Quit);
    harness.run_steps(3);
    assert!(!harness.state().quit_asked, "quit without asking");
    assert!(harness.query_by_label("Exit dm?").is_some());
    click(&mut harness, "Keep downloading");
    assert!(!harness.state().quit_asked);
    assert!(harness.query_by_label("Exit dm?").is_none());

    harness.state_mut().run(Command::Quit);
    harness.run_steps(3);
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Exit")
        .click();
    harness.run_steps(3);
    assert!(harness.state().quit_asked, "Exit in the question");

    harness.state_mut().quit_asked = false;
    harness
        .state_mut()
        .control_call("command.run", &serde_json::json!({ "id": "program.exit" }))
        .unwrap();
    assert!(
        harness.state().quit_asked,
        "a script cannot answer a question"
    );
}

/// Downloads finishing while the Exit question is open never quit
/// dm by themselves; the question changes and waits for the person, its
/// buttons stay where they were, and once "Keep downloading" is clicked
/// nothing that finishes afterwards quits either.
#[test]
fn downloads_finishing_under_the_exit_question_never_quit_dm() {
    let world = world();
    let mut harness = world.harness();
    for name in ["stalled.bin?one", "stalled.bin?two"] {
        harness
            .state()
            .backend()
            .add_with(&world.url(name), Default::default())
            .unwrap();
    }
    until(&mut harness, "both to run", |app| app.downloading() == 2);
    harness.state_mut().run(Command::Quit);
    harness.run_steps(3);
    let at = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Exit")
            .rect()
    };
    let before = at(&harness);
    // Given the frames to say it on a busy machine.
    for _ in 0..50 {
        if harness.query_by_label_contains("2 downloads").is_some() {
            break;
        }
        harness.step();
    }
    assert!(harness.query_by_label_contains("2 downloads").is_some());

    // One finishes: fewer words, and Exit has not moved.
    let first = harness.state().rows()[0].id;
    harness.state().backend().pause(first);
    until(&mut harness, "one to stop", |app| app.downloading() == 1);
    harness.run_steps(3);
    assert!(harness
        .query_by_label_contains("Exiting stops it; it goes on")
        .is_some());
    assert_eq!(at(&harness), before, "Exit moved under the pointer");

    // The rest finish: the question changes and waits, frame after frame.
    harness.state().backend().pause_all();
    until(&mut harness, "all to stop", |app| app.downloading() == 0);
    harness.run_steps(10);
    assert!(!harness.state().quit_asked, "quit by itself");
    assert!(harness
        .query_by_label("Nothing is downloading any more. Exit dm?")
        .is_some());
    assert_eq!(at(&harness), before, "Exit moved under the pointer");
    click(&mut harness, "Stay");
    harness.run_steps(5);
    assert!(!harness.state().quit_asked, "Stay quit");
    assert!(harness.query_by_label("Exit dm?").is_none());

    // Keep downloading, then everything finishes: still running.
    harness.state().backend().resume_all();
    until(&mut harness, "both to run again", |app| {
        app.downloading() == 2
    });
    harness.state_mut().run(Command::Quit);
    harness.run_steps(3);
    click(&mut harness, "Keep downloading");
    harness.state().backend().pause_all();
    until(&mut harness, "all to stop", |app| app.downloading() == 0);
    harness.run_steps(10);
    assert!(!harness.state().quit_asked, "quit after Keep downloading");
    assert!(harness.query_by_label("Exit dm?").is_none());
}

/// The speed limiter's menu item is ticked while a limit is on.
#[test]
fn the_speed_limiter_item_shows_whether_it_is_on() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let state = |h: &mut Harness<'_, DmApp>| {
        h.run_steps(2);
        h.query_all_by_label("Downloads").last().unwrap().click();
        h.run_steps(3);
        h.query_all_by_label_contains("Speed limiter")
            .rfind(|n| n.accesskit_node().role() != egui::accesskit::Role::Label)
            .unwrap()
            .click();
        h.run_steps(3);
        let ticked = h.get_by_label("Limit speed").accesskit_node().toggled();
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        ticked
    };
    assert_eq!(state(&mut h), Some(egui::accesskit::Toggled::False));
    h.state_mut().run(Command::SpeedLimiterToggle);
    assert_eq!(state(&mut h), Some(egui::accesskit::Toggled::True));
    h.state_mut().run(Command::SpeedLimiterToggle);
    assert_eq!(h.state().applied.speed_limit, 0);
}

/// Start now in Download details while the schedule is shut asks
/// whether to start it outside the schedule, as Resume does.
#[test]
fn start_now_under_a_shut_schedule_offers_to_start_outside_it() {
    let _file = settings_file();
    use chrono::Timelike;
    let world = world();
    let mut harness = world.harness();
    let now = chrono::Local::now();
    let mut wanted = harness.state().applied.clone();
    wanted.schedule = dm_core::Schedule {
        wake: false,
        enabled: true,
        from: format!("{:02}:{:02}", (now.hour() + 2) % 24, now.minute()),
        to: format!("{:02}:{:02}", (now.hour() + 3) % 24, now.minute()),
        days: Vec::new(),
    };
    harness.state_mut().apply_settings(wanted).unwrap();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    assert!(
        harness
            .query_by_label("Start outside the schedule?")
            .is_some(),
        "Start now asked nothing under a shut schedule"
    );
    click(&mut harness, "Start now");
    until(&mut harness, "it to finish outside the schedule", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
    let mut off = harness.state().applied.clone();
    off.schedule.enabled = false;
    harness.state_mut().apply_settings(off).unwrap();
}

/// The speed presets set the one limit every download shares, and Full
/// speed lifts it; each is in Downloads › Speed limiter.
#[test]
fn the_speed_presets_set_and_lift_the_limit() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    for (item, kb) in [
        ("Low speed (256 KB/s)", 256),
        ("Medium speed (1 MB/s)", 1024),
        ("High speed (4 MB/s)", 4096),
        ("Full speed", 0),
    ] {
        press(&mut harness, &["Downloads", "Speed limiter", item]);
        assert_eq!(harness.state().applied.speed_limit, kb, "{item}");
        let in_force = world
            .runtime
            .block_on(harness.state().backend().queue.speed_limit());
        assert_eq!(in_force, kb * 1024, "{item} not in force");
    }
}
