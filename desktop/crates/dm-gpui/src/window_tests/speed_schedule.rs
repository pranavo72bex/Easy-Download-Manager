// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The speed limiter, the Scheduler, When done and Exit, driven through the window. Ported
//! from the egui window's `tests/window/speed_schedule.rs`.

use std::time::Instant;

use dm_core::State;
use dm_shell::app::power::WhenDone;

use crate::harness::world::{world, World};
use crate::harness::{settings_file, Harness};
use crate::scheduler::Scheduler;

/// A time chosen from its pop-ups, hour then minute, as a person chooses it (gaps row U32):
/// `field` is the Scheduler's "Start at" or "Stop at", or the Speed limiter's.
fn set_time(h: &mut Harness, field: &str, time: &str) {
    use crate::popup::Pick;
    let (hour, minute) = dm_desktop_base::scheduler::time_parts(time);
    let (hour_field, minute_field): (&'static str, &'static str) = match field {
        "Start at" => ("from-hour", "from-minute"),
        "Stop at" => ("to-hour", "to-minute"),
        "Slow start at" => ("slow-from-hour", "slow-from-minute"),
        _ => ("slow-to-hour", "slow-to-minute"),
    };
    let picks = [(hour_field, hour), (minute_field, minute)];
    if h.dialog_open("Scheduler") {
        for (field, index) in picks {
            h.dialog(|s: &mut Scheduler, _, cx| {
                s.pick(
                    &Pick {
                        field,
                        item: 0,
                        index: index as usize,
                    },
                    cx,
                )
            });
        }
    } else {
        for (field, index) in picks {
            h.dialog(|l: &mut crate::limiter::Limiter, _, cx| {
                l.pick(
                    &Pick {
                        field,
                        item: 0,
                        index: index as usize,
                    },
                    cx,
                )
            });
        }
    }
}

/// The settings in force.
fn kept(h: &mut Harness) -> dm_shell::Settings {
    h.view(|view| view.backend.settings())
}

/// A few wakes of the window, for what must not happen in them.
fn wakes(h: &mut Harness) {
    for _ in 0..10 {
        h.until("a wake", |_| true);
    }
}

/// Exit was carried out.
fn quit(h: &mut Harness) -> bool {
    h.view(|view| view.quit_asked)
}

/// How many downloads run.
fn running(h: &mut Harness) -> usize {
    h.rows()
        .iter()
        .filter(|r| r.state == State::Running)
        .count()
}

/// Waits for Exit's countdown, says it has not quit yet, then runs it out.
fn quit_after_its_countdown(h: &mut Harness) {
    h.until("Exit's countdown", |h| {
        h.view(|view| {
            view.session
                .countdown
                .is_some_and(|(action, _)| action == WhenDone::Quit)
        })
    });
    assert!(!quit(h), "quit before its countdown");
    h.view(|view| view.session.countdown = Some((WhenDone::Quit, Instant::now())));
    h.until("dm to quit", quit);
}

/// Exit, from the Tasks menu, as a person asks for it.
fn ask_exit(h: &mut Harness) {
    h.click("Tasks");
    h.click("Exit");
}

/// Settings with the schedule shut now: open from two hours on, for an hour.
fn shut_schedule(world: &World) -> dm_shell::Settings {
    let (_, minute) = dm_core::weekday_now();
    let (hour, minute) = (minute / 60, minute % 60);
    let mut settings = world.settings();
    settings.schedule = dm_core::Schedule {
        enabled: true,
        from: format!("{:02}:{minute:02}", (hour + 2) % 24),
        to: format!("{:02}:{minute:02}", (hour + 3) % 24),
        days: Vec::new(),
        ..Default::default()
    };
    settings
}

/// "When done" waited for nothing Running or Queued, so a queue a person stopped, or one the
/// schedule had put down at the window's close, counted as done — dm quit (or the computer
/// slept) with the download half fetched and nothing to resume it.
#[test]
fn when_done_does_not_fire_on_a_stopped_or_schedule_held_queue() {
    let world = world();
    let mut h = world.harness();

    // Stop all: nothing finished, so nothing is done; the choice stays.
    h.command("downloads.when_done.exit");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the download to start", |h| running(h) == 1);
    h.command("downloads.stop_all");
    h.until("the download to be stopped", |h| {
        h.rows().iter().all(|r| r.state == State::Paused)
    });
    wakes(&mut h);
    assert!(!quit(&mut h), "quit on a queue a person stopped");
    assert_eq!(h.view(|view| view.session.when_done), WhenDone::Quit);

    // Something else finishes; the stopped row is left alone, not waited for.
    h.add_url(&world.url("file.zip"), "Start now");
    quit_after_its_countdown(&mut h);

    // The schedule's window closes on a running download: it is held, not done, and dm stays
    // to resume it when the window reopens. (dm quit above: the next launch's window.)
    let world = crate::harness::world::world();
    let mut h = world.harness();
    h.command("downloads.when_done.sleep");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the download to start", |h| running(h) == 1);
    let stalled = h.rows()[0].id;
    // Another finishes meanwhile, so "something finished" holds as well.
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("one to finish and one to run", |h| {
        let rows = h.rows();
        rows.iter()
            .any(|r| r.id == stalled && r.state == State::Running)
            && rows.iter().any(|r| r.state == State::Completed)
    });
    let tomorrow = (dm_core::weekday_now().0 + 1) % 7;
    h.view(|view| {
        view.backend.queue.set_schedule(dm_core::Schedule {
            enabled: true,
            from: "00:00".into(),
            to: "23:59".into(),
            days: vec![tomorrow],
            ..Default::default()
        })
    });
    h.until("the schedule to hold the download", |h| {
        h.rows()
            .iter()
            .any(|r| r.id == stalled && r.state == State::Paused && r.paused_by_schedule)
    });
    wakes(&mut h);
    assert!(!quit(&mut h));
    assert!(
        h.view(|view| view.session.countdown.is_none()),
        "counted down to sleep with a download the schedule holds"
    );
    assert_eq!(h.view(|view| view.session.when_done), WhenDone::Sleep);
}

#[test]
fn a_sleep_the_system_refuses_is_said_in_the_problems() {
    let world = world();
    *world.fake.power_fails.lock().unwrap() = true;
    let mut h = world.harness();
    h.view(|view| view.session.countdown = Some((WhenDone::Sleep, Instant::now())));
    h.until("the refusal in the problems", |h| {
        h.view(|view| {
            view.problems
                .iter()
                .any(|p| p.contains("could not put the computer to sleep"))
        })
    });
    h.until("the refusal on screen", |h| {
        h.shows_part("ould not put the computer to sleep")
    });
}

#[test]
fn the_scheduler_holds_only_times_that_are_times_and_says_why_otherwise() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.command("downloads.scheduler");
    h.until("the Scheduler", |h| h.dialog_open("Scheduler"));
    h.click("Only download between set hours");
    // A time is chosen, hour and minute, so none can be held that is not one (gaps row U32):
    // typed, "25:00" and "1am" were refused only at OK.
    let hour12 = dm_desktop_base::region::system().is_some_and(|r| r.hour12);
    assert_eq!(dm_desktop_base::scheduler::hour_choices(hour12).len(), 24);

    // No day ticked is refused where it was unticked: every day is shown, and kept, as seven
    // ticks.
    set_time(&mut h, "Start at", "01:00");
    set_time(&mut h, "Stop at", "07:00");
    let days = dm_desktop_base::held::DAYS.map(dm_desktop_base::i18n::t);
    assert_eq!(
        h.dialog(|s: &mut Scheduler, _, _| s.draft.schedule.days.len()),
        7,
        "every day ticked"
    );
    for day in days {
        h.click(day);
    }
    h.click("OK");
    h.until_shown("Tick at least one day.");
    assert!(!kept(&mut h).schedule.enabled);
    for day in days {
        h.click(day);
    }
    h.click("OK");
    h.until("the Scheduler to close", |h| !h.dialog_open("Scheduler"));
    let held = kept(&mut h).schedule;
    assert!(held.enabled);
    assert!(held.open_at(0, 2 * 60) && held.open_at(3, 2 * 60));
}

#[test]
fn the_speed_limiter_turns_off_and_back_on_to_its_last_value() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.command("downloads.speed_limiter.toggle");
    assert_eq!(kept(&mut h).speed_limit, 512);
    h.command("downloads.speed_limiter.toggle");
    assert_eq!(kept(&mut h).speed_limit, 0);
    h.command("downloads.speed_limiter.toggle");
    assert_eq!(kept(&mut h).speed_limit, 512);
}

#[test]
fn the_download_limit_is_set_in_the_speed_limiter_and_reaches_the_queue() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.command("downloads.speed_limiter.settings");
    h.until("the Speed limiter", |h| h.dialog_open("Speed limiter"));
    assert!(h.shows("Download at most this many MB (0 for no limit)"));
    assert!(h.shows("in each period of this many hours"));
    h.fill("Download at most this many MB (0 for no limit)", "500");
    h.fill("in each period of this many hours", "4");
    h.click("OK");
    h.until("the Speed limiter to close", |h| {
        !h.dialog_open("Speed limiter")
    });
    let settings = kept(&mut h);
    assert_eq!((settings.limit_mb, settings.limit_hours), (500, 4));
    let queue = h.view(|view| view.backend.queue.clone());
    assert!(queue.allowance_spent().is_none(), "nothing downloaded yet");
    assert!(queue.may_run());
}

#[test]
fn the_weekly_update_check_asks_nothing_when_off_or_not_yet_due() {
    let world = world();
    let asked = |h: &mut Harness| {
        let checked = h.view(|view| view.checked.clone());
        let not_asked = matches!(
            *dm_core::lock(&checked),
            dm_shell::app::update::Update::NotAsked
        );
        !not_asked || h.view(|view| view.weekly_pending)
    };
    let mut h = world.harness();
    wakes(&mut h);
    assert!(!asked(&mut h), "off by default: nothing is asked");
    // On, but checked a moment ago: not due for a week.
    let mut wanted = world.settings();
    wanted.check_updates = true;
    wanted.update_checked = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut h = world.harness_with(wanted);
    wakes(&mut h);
    assert!(!asked(&mut h));
}

/// Limit speed turns back on at the limit last turned off, after a restart too (it came back
/// on at 1 KB/s in the GPUI window).
#[test]
fn the_speed_limit_last_turned_off_comes_back_after_a_restart() {
    let _file = settings_file();
    let world = world();
    let mut wanted = world.settings();
    wanted.speed_limit = 300;
    let mut h = world.harness_with(wanted);
    h.command("downloads.speed_limiter.toggle");
    let (saved, _) = dm_shell::Settings::load();
    assert_eq!((saved.speed_limit, saved.last_speed_limit), (0, 300));
    // The next launch, from what was saved.
    let mut next = world.harness_with(saved);
    next.command("downloads.speed_limiter.toggle");
    assert_eq!(kept(&mut next).speed_limit, 300);
}

/// Exit while downloading asks first; a script's exit does not.
#[test]
fn exit_while_downloading_asks_first() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("it to run", |h| running(h) > 0);
    ask_exit(&mut h);
    h.until("Exit dm?", |h| h.dialog_open("Exit dm?"));
    assert!(!quit(&mut h), "quit without asking");
    h.click("Keep downloading");
    h.until("the question to go", |h| !h.dialog_open("Exit dm?"));
    assert!(!quit(&mut h));

    ask_exit(&mut h);
    h.until("Exit dm?", |h| h.dialog_open("Exit dm?"));
    h.click("Exit");
    h.until("Exit in the question", quit);

    h.view(|view| view.quit_asked = false);
    h.command("program.exit");
    assert!(quit(&mut h), "a script cannot answer a question");
}

/// Downloads finishing while the Exit question is open never quit dm by themselves; the
/// question changes and waits for the person, its buttons stay where they were, and once
/// "Keep downloading" is clicked nothing that finishes afterwards quits either.
#[test]
fn downloads_finishing_under_the_exit_question_never_quit_dm() {
    let world = world();
    let mut h = world.harness();
    for name in ["stalled.bin?one", "stalled.bin?two"] {
        let url = world.url(name);
        h.view(|view| view.backend.add_with(&url, Default::default()))
            .unwrap();
    }
    h.until("both to run", |h| running(h) == 2);
    ask_exit(&mut h);
    h.until("Exit dm?", |h| h.dialog_open("Exit dm?"));
    let exit_at = |h: &mut Harness| h.dialog_words("Exit").expect("Exit in the question");
    h.until("2 downloads", |h| h.shows_part("2 downloads"));
    let before = exit_at(&mut h);

    // One finishes: fewer words, and Exit has not moved.
    let first = h.rows()[0].id;
    h.view(|view| view.backend.pause(first));
    h.until("one to stop", |h| running(h) == 1);
    h.until("the words for one", |h| {
        h.shows_part("Exiting pauses it. It picks up")
    });
    assert_eq!(exit_at(&mut h), before, "Exit moved under the pointer");

    // The rest finish: the question changes and waits, frame after frame.
    h.view(|view| view.backend.pause_all());
    h.until("all to stop", |h| running(h) == 0);
    wakes(&mut h);
    assert!(!quit(&mut h), "quit by itself");
    h.until_shown("Nothing is downloading any more. Exit dm?");
    assert_eq!(exit_at(&mut h), before, "Exit moved under the pointer");
    h.click("Stay");
    h.until("the question to go", |h| !h.dialog_open("Exit dm?"));
    wakes(&mut h);
    assert!(!quit(&mut h), "Stay quit");

    // Keep downloading, then everything finishes: still running.
    h.view(|view| view.backend.resume_all());
    h.until("both to run again", |h| running(h) == 2);
    ask_exit(&mut h);
    h.until("Exit dm?", |h| h.dialog_open("Exit dm?"));
    h.click("Keep downloading");
    h.until("the question to go", |h| !h.dialog_open("Exit dm?"));
    h.view(|view| view.backend.pause_all());
    h.until("all to stop", |h| running(h) == 0);
    wakes(&mut h);
    assert!(!quit(&mut h), "quit after Keep downloading");
    assert!(!h.dialog_open("Exit dm?"));
}

/// The speed limiter's menu item is ticked while a limit is on.
#[test]
fn the_speed_limiter_item_shows_whether_it_is_on() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let ticked = |h: &mut Harness| {
        h.menu_item(&["Downloads", "Speed limiter", "Limit speed"])
            .0
    };
    assert!(!ticked(&mut h));
    h.command("downloads.speed_limiter.toggle");
    assert!(ticked(&mut h));
    h.command("downloads.speed_limiter.toggle");
    assert!(!ticked(&mut h));
    assert_eq!(kept(&mut h).speed_limit, 0);
}

/// Reference A's "Hang up modem when done", in the Scheduler on Windows after the program to
/// run: kept with the hours, and once the scheduled downloads are done (nothing left to run in
/// them) the connection Options › Advanced › Dial Up / VPN sets is hung up; with none set
/// there, the status line says so. Not offered where there are no dial-up connections.
#[test]
fn the_scheduler_hangs_up_the_dial_up_connection_when_its_downloads_are_done() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.command("downloads.scheduler");
    h.until("the Scheduler", |h| h.dialog_open("Scheduler"));
    // Start and stop the same: open all day.
    h.click("Only download between set hours");
    set_time(&mut h, "Start at", "00:00");
    set_time(&mut h, "Stop at", "00:00");
    let offered = h.shows("Hang up modem when done");
    assert_eq!(offered, cfg!(windows), "offered on Windows alone");
    if !cfg!(windows) {
        h.click("Cancel");
        return;
    }
    h.click("Hang up modem when done");
    h.click("OK");
    h.until("the Scheduler to close", |h| !h.dialog_open("Scheduler"));
    assert!(kept(&mut h).schedule.hang_up);
    h.command("downloads.scheduler");
    h.until("the Scheduler", |h| h.dialog_open("Scheduler"));
    assert!(
        h.dialog(|s: &mut Scheduler, _, _| s.draft.schedule.hang_up),
        "shown as kept when opened again"
    );
    h.click("Cancel");

    // Something runs in the hours, then nothing is left to run: done.
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the download to run", |h| running(h) == 1);
    h.command("downloads.stop_all");
    h.until("the word that nothing was hung up", |h| {
        h.view(|view| {
            view.said.as_ref().is_some_and(|(said, _)| {
                said == "Hang up modem when done: no connection is set in Options › Advanced › Dial Up / VPN."
            })
        })
    });
}

/// Start now in Download details while the schedule is shut asks whether to start it outside
/// the schedule, as Resume does.
#[test]
fn start_now_under_a_shut_schedule_offers_to_start_outside_it() {
    let world = world();
    let mut h = world.harness_with(shut_schedule(&world));
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the question", |h| h.shows("Start outside the schedule?"));
    h.click("Start now");
    h.until("it to finish outside the schedule", |h| {
        let rows = h.rows();
        !rows.is_empty() && rows.iter().all(|r| r.state == State::Completed)
    });
}

/// The speed presets set the one limit every download shares, and Full speed lifts it; each
/// is in Downloads › Speed limiter.
#[test]
fn the_speed_presets_set_and_lift_the_limit() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let queue = h.view(|view| view.backend.queue.clone());
    for (item, kb) in [
        ("Low speed (256 KB/s)", 256),
        ("Medium speed (1 MB/s)", 1024),
        ("High speed (4 MB/s)", 4096),
        ("Full speed", 0),
    ] {
        h.click("Downloads");
        h.click("Speed limiter");
        h.click(item);
        h.until(item, |h| kept(h).speed_limit == kb);
        let in_force = world.runtime.block_on(queue.speed_limit());
        assert_eq!(in_force, kb * dm_core::KB, "{item} not in force");
    }
}

/// Each queue keeps hours of its own, chosen in the Scheduler's queue list as reference A's:
/// the Main queue's are the ones every other queue follows until it is given its own; a queue
/// can start once on a date; and Start now starts the queue chosen there.
#[test]
fn two_queues_keep_their_own_hours_in_the_scheduler() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let id = h.rows()[0].id;
    h.view(|view| view.backend.set_queue(id, "Night"));
    h.until("the row to be in the Night queue", |h| {
        h.rows().iter().any(|r| r.queue == "Night")
    });
    let draft = |h: &mut Harness| h.dialog(|s: &mut Scheduler, _, _| s.draft.clone());

    h.command("downloads.scheduler");
    h.until("the Scheduler", |h| h.dialog_open("Scheduler"));
    assert_eq!(
        draft(&mut h).queue,
        dm_core::MAIN_QUEUE,
        "opened on the list's queue"
    );
    h.click("Only download between set hours");
    set_time(&mut h, "Start at", "01:00");
    set_time(&mut h, "Stop at", "07:00");
    h.click("Night");
    h.until("Night chosen", |h| draft(h).queue == "Night");
    assert!(
        h.shows("This queue follows the Main queue's hours."),
        "a queue with no hours of its own says whose it follows"
    );
    h.click("Hours of its own for this queue");
    assert!(draft(&mut h).own);
    if !draft(&mut h).schedule.enabled {
        h.click("Only download between set hours");
    }
    set_time(&mut h, "Start at", "22:00");
    set_time(&mut h, "Stop at", "23:00");
    h.click("OK");
    h.until("OK to keep them", |h| !h.dialog_open("Scheduler"));
    let settings = kept(&mut h);
    assert!(settings.schedule.enabled);
    assert_eq!(settings.schedule.from, "01:00");
    assert_eq!(settings.queue_schedules["Night"].from, "22:00");
    assert_eq!(settings.schedule_for("Night").to, "23:00");
    assert_eq!(settings.schedule_for(dm_core::MAIN_QUEUE).to, "07:00");

    // Opened again: each queue shows what it kept.
    h.command("downloads.scheduler");
    h.until("the Scheduler", |h| h.dialog_open("Scheduler"));
    assert_eq!(draft(&mut h).schedule.from, "01:00");
    h.click("Night");
    h.until("Night chosen", |h| draft(h).queue == "Night");
    let night = draft(&mut h);
    assert!(night.own && night.schedule.from == "22:00");
    assert!(h.shows("22"), "its hours shown");
    // Once, on a date chosen from its pop-ups, which hold only dates (gaps row U32): the last
    // day of this year.
    h.click("Once, on this date");
    for (field, index) in [("date-year", 0), ("date-month", 11), ("date-day", 30)] {
        h.dialog(|s: &mut Scheduler, _, cx| {
            s.pick(
                &crate::popup::Pick {
                    field,
                    item: 0,
                    index,
                },
                cx,
            )
        });
    }
    let last_day = format!("{}-12-31", dm_desktop_base::scheduler::today().0);
    // Start now acts on the queue chosen in the list, not the list's.
    h.click("Start now");
    let said = h.view(|view| view.said.clone().map(|(said, _)| said).unwrap_or_default());
    assert!(said.contains("Night"), "Start now: {said:?}");
    h.click("OK");
    h.until("OK to keep them", |h| !h.dialog_open("Scheduler"));
    let night = kept(&mut h).queue_schedules["Night"].clone();
    assert_eq!(night.once_at, Some(format!("{last_day} 22:00")));

    // A queue given back to the Main queue's hours loses its own.
    h.command("downloads.scheduler");
    h.until("the Scheduler", |h| h.dialog_open("Scheduler"));
    h.click("Night");
    h.until("Night chosen", |h| draft(h).queue == "Night");
    h.click("Hours of its own for this queue");
    h.click("OK");
    h.until("OK to keep them", |h| !h.dialog_open("Scheduler"));
    assert!(!kept(&mut h).queue_schedules.contains_key("Night"));
}

/// The Speed limiter's settings keep the slow hours, chosen from pop-ups that cannot hold a time
/// that is not one (gaps row U32), and the window puts the slow limit in force in its hours.
#[test]
fn the_speed_limiter_keeps_slower_hours_and_puts_them_in_force() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.command("downloads.speed_limiter.settings");
    h.until("the Speed limiter", |h| h.dialog_open("Speed limiter"));
    h.click("Slower between set hours");
    // Hours that hold every minute of the day, so the test is in them.
    set_time(&mut h, "Slow start at", "00:00");
    set_time(&mut h, "Slow stop at", "23:59");
    h.fill("Speed limit in those hours (KB/s)", "64");
    h.click("OK");
    h.until("the Speed limiter to close", |h| {
        !h.dialog_open("Speed limiter")
    });
    let slow = kept(&mut h).slow_hours;
    assert!(slow.enabled && slow.from == "00:00" && slow.limit == 64);
    let (weekday, minute) = dm_core::weekday_now();
    if minute < 23 * 60 + 59 {
        h.until(
            &format!("the slow limit in force in its hours (day {weekday})"),
            |h| h.view(|view| view.session.limit_in_force()) == Some(64 * dm_core::KB),
        );
    }
}
