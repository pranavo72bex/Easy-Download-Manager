// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list and what is said of it: the side panel's places, the columns and their sort, the
//! toolbar, the status bar, the rows' names, sizes and folders, and what a command acts on.
//! Ported from the egui window's `tests/window/lists.rs`.

use std::time::Instant;

use dm_core::State;
use dm_desktop_base::columns::Column;
use dm_shell::app::places::Place;
use dm_shell::app::power::WhenDone;

use crate::harness::world::{world, World, PAYLOAD};
use crate::harness::{settings_file, Harness};

/// The settings in force.
fn kept(h: &mut Harness) -> dm_shell::Settings {
    h.view(|view| view.backend.settings())
}

/// `name` added for later, fetching nothing.
fn add_later(h: &mut Harness, world: &World, name: &str) {
    let url = world.url(name);
    h.view(|view| {
        view.backend.add_with(
            &url,
            dm_core::AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            },
        )
    })
    .unwrap();
}

/// The names of the rows the list shows, in its order.
fn shown(h: &mut Harness) -> Vec<String> {
    h.view(|view| {
        view.shown
            .iter()
            .filter_map(|&i| view.rows.get(i))
            .map(|r| dm_shell::app::names::name_of(r))
            .collect()
    })
}

/// The names of the rows a command acts on, in the list's order.
fn picked(h: &mut Harness) -> Vec<String> {
    h.view(|view| {
        view.chosen()
            .into_iter()
            .map(dm_shell::app::names::name_of)
            .collect()
    })
}

/// Selects a row by clicking its name in the list.
fn select(h: &mut Harness, name: &str) {
    h.click(name);
    h.until(&format!("{name} chosen"), |h| picked(h) == [name]);
}

/// Where the words `words` start across the main window.
fn left_of(h: &mut Harness, words: &str) -> f32 {
    let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
    texts["texts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["text"] == words && t["window"] == "main")
        .and_then(|t| t["rect"][0].as_f64())
        .unwrap_or_else(|| panic!("{words:?} is not on screen")) as f32
}

/// Settings with the schedule shut now: open from two hours on, for an hour; and its start.
fn shut_schedule(world: &World) -> (dm_shell::Settings, String) {
    let (_, minute) = dm_core::weekday_now();
    let (hour, minute) = (minute / 60, minute % 60);
    let from = format!("{:02}:{minute:02}", (hour + 2) % 24);
    let mut settings = world.settings();
    settings.schedule = dm_core::Schedule {
        enabled: true,
        from: from.clone(),
        to: format!("{:02}:{minute:02}", (hour + 3) % 24),
        days: Vec::new(),
        ..Default::default()
    };
    (settings, from)
}

/// Start now under a shut schedule asks at once whether to start outside it: kept waiting.
fn keep_waiting(h: &mut Harness) {
    h.until("Start outside the schedule?", |h| {
        h.shows("Start outside the schedule?")
    });
    h.click("Keep waiting");
    h.until("the question to go", |h| {
        !h.shows("Start outside the schedule?")
    });
}

/// Exit, from the Tasks menu, as a person asks for it.
fn ask_exit(h: &mut Harness) {
    h.click("Tasks");
    h.click("Exit");
}

#[test]
fn when_the_queue_runs_dry_dm_chimes_and_counts_down_to_what_was_chosen() {
    let world = world();
    let mut settings = world.settings();
    settings.sound_on_complete = true;
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
    assert!(!h.view(|view| view.quit_asked), "quit before its countdown");
    h.view(|view| view.session.countdown = Some((WhenDone::Quit, Instant::now())));
    h.until("dm to quit", |h| h.view(|view| view.quit_asked));
}

/// A row not saved yet shows the name chosen for it, not its address's.
#[test]
fn a_row_shows_the_name_chosen_for_it_before_it_is_saved() {
    let world = world();
    let mut h = world.harness();
    for (name, chosen) in [
        ("download.php?id=5", Some("mine.zip")),
        ("other.php?id=6", None),
    ] {
        let url = world.url(name);
        h.view(|view| {
            view.backend.add_with(
                &url,
                dm_core::AddOptions {
                    later: true,
                    queue: dm_core::MAIN_QUEUE.into(),
                    save_name: chosen.map(Into::into),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    }
    h.until("both rows", |h| h.rows().len() == 2);
    let names = shown(&mut h);
    assert!(names.contains(&"mine.zip".to_string()), "{names:?}");
    assert!(names.contains(&"other.php".to_string()), "{names:?}");
    h.until_shown("mine.zip");
}

#[test]
fn the_toolbar_acts_on_the_selected_row() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the row to start", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    select(&mut h, "stalled.bin");
    let state = |h: &mut Harness| h.rows()[0].state.clone();

    h.click("Stop");
    h.until("Stop", |h| state(h) == State::Paused);
    h.click("Resume");
    h.until("Resume", |h| {
        matches!(state(h), State::Queued | State::Running)
    });
    h.click("Stop All");
    h.until("Stop All", |h| state(h) == State::Paused);
    // Unfinished, so Delete asks first, in a window named for what it asks.
    h.click("Delete");
    h.until("the question", |h| {
        h.dialog_open("Remove an unfinished download")
    });
    h.click("Keep it");
    h.until("the question to go", |h| {
        !h.dialog_open("Remove an unfinished download")
    });
    assert_eq!(h.rows().len(), 1, "Keep deleted it");
    h.click("Delete");
    h.until("the question", |h| {
        h.dialog_open("Remove an unfinished download")
    });
    h.click("Yes, remove");
    h.until("the row to go", |h| h.rows().is_empty());
}

#[test]
fn the_category_tree_filters_the_one_list() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("one to finish and one to fail", |h| {
        let rows = h.rows();
        rows.iter().any(|r| r.state == State::Completed)
            && rows.iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    let sorted = |h: &mut Harness| {
        let mut names = shown(h);
        names.sort();
        names
    };

    // Unfinished is every download not finished: the stalled one and the one that failed
    // (gaps row U1).
    h.click("Finished");
    h.until("Finished", |h| h.view(|view| view.place == Place::Finished));
    assert_eq!(sorted(&mut h), ["file.zip"]);
    h.click("Unfinished");
    h.until("Unfinished", |h| {
        h.view(|view| view.place == Place::Unfinished)
    });
    assert_eq!(sorted(&mut h), ["secret.zip", "stalled.bin"]);
    // Both archives, the failed one too, known by its name though the server never said what
    // it was.
    h.click("Archives");
    h.until("Archives", |h| {
        h.view(|view| view.place == Place::Kind(dm_core::Category::Archives))
    });
    assert_eq!(sorted(&mut h), ["file.zip", "secret.zip"]);
}

#[test]
fn clicking_a_heading_sorts_and_clicking_again_reverses() {
    // A sort is saved.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    for name in ["b.bin", "c.bin", "a.bin"] {
        add_later(&mut h, &world, name);
    }
    h.until("three rows", |h| h.rows().len() == 3);
    h.click("File Name");
    h.until("the sort", |h| {
        h.view(|view| view.sort) == Some((Column::Name, true))
    });
    assert_eq!(shown(&mut h), ["a.bin", "b.bin", "c.bin"]);
    h.click("File Name");
    h.until("the sort reversed", |h| {
        h.view(|view| view.sort) == Some((Column::Name, false))
    });
    assert_eq!(shown(&mut h), ["c.bin", "b.bin", "a.bin"]);
}

/// A column turned off in View › Columns is gone and comes back; one moved (a heading dragged
/// onto another, as the GPUI window moves a column, where the egui window had Move items in
/// the menu) is drawn and kept in its new place.
#[test]
fn a_column_turned_off_is_gone_and_comes_back() {
    // Turning one off is saved in the settings file other tests read.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, "x.bin");
    h.until("the row", |h| h.rows().len() == 1);
    assert!(h.shows("Transfer rate"));
    h.click("View");
    h.click("Columns");
    h.click("Transfer rate");
    h.until("the column gone", |h| !h.shows("Transfer rate"));
    assert_eq!(
        kept(&mut h).columns_hidden,
        ["rate"],
        "not kept for the next launch"
    );
    assert!(
        !h.menu_item(&["View", "Columns", "Transfer rate"]).0,
        "still ticked"
    );
    h.click("View");
    h.click("Columns");
    h.click("Transfer rate");
    h.until("the column back", |h| h.shows("Transfer rate"));

    // Moved: Size after Status.
    assert!(left_of(&mut h, "Size") < left_of(&mut h, "Status"));
    h.update(|view, cx| view.move_column(Column::Size, Column::Status, cx));
    h.until("the headings in the new order", |h| {
        left_of(h, "Size") > left_of(h, "Status")
    });
    let saved = kept(&mut h);
    assert_eq!(
        Column::ordered(&saved.column_order)[..4],
        [Column::Name, Column::Queue, Column::Status, Column::Size]
    );
}

/// Kept to the column: a row added for later shows the folder it will go to, without that
/// guess written into the row a command reads.
#[test]
fn a_row_added_for_later_shows_its_folder_without_owning_it() {
    let world = world();
    let mut settings = world.settings();
    settings.categorize = true;
    let mut h = world.harness_with(settings);
    add_later(&mut h, &world, "file.zip");
    add_later(&mut h, &world, "download.php?id=5");
    h.until("both rows", |h| h.rows().len() == 2);
    let zip = h
        .rows()
        .into_iter()
        .find(|r| r.url.ends_with("file.zip"))
        .unwrap();
    assert_eq!(
        zip.save_dir, None,
        "the planned folder was written into the row"
    );
    let out = world.dir.path().join("out");
    let planned = out.join("Archives").display().to_string();
    h.until("the Folder column to show the kind's folder", |h| {
        h.shows(&planned)
    });
    // A name that does not say its kind shows none: the server decides.
    let php = h
        .rows()
        .into_iter()
        .find(|r| r.url.contains("download.php"))
        .unwrap();
    let mut categories = kept(&mut h).categories();
    assert_eq!(
        dm_shell::app::names::planned_folder(&php, &out, &mut categories),
        None
    );
}

#[test]
fn find_next_selects_the_next_row_with_the_name() {
    let world = world();
    let mut h = world.harness();
    for name in ["alpha.bin", "beta.bin", "alphabet.bin"] {
        add_later(&mut h, &world, name);
    }
    h.until("three rows", |h| h.rows().len() == 3);
    // Find is the search box (gaps row U30): its words narrow the list, and Find next steps
    // through what it shows.
    h.command("downloads.find");
    h.type_text("alpha");
    h.until("the box to narrow", |h| shown(h).len() == 2);
    h.command("downloads.find_next");
    assert_eq!(picked(&mut h), ["alpha.bin"]);
    h.command("downloads.find_next");
    assert_eq!(picked(&mut h), ["alphabet.bin"]);
}

/// Find previous goes back from the selection and wraps round; Shift-F3 is its key, and the
/// Downloads menu has it beside Find next.
#[test]
fn find_previous_selects_the_row_before_and_wraps_round() {
    use dm_shell::app::commands::Command;
    let world = world();
    let mut h = world.harness();
    for name in ["alpha.bin", "beta.bin", "alphabet.bin"] {
        add_later(&mut h, &world, name);
    }
    h.until("three rows", |h| h.rows().len() == 3);
    h.command("downloads.find");
    h.type_text("alpha");
    h.until("the box to narrow", |h| shown(h).len() == 2);
    // With nothing selected it starts at the bottom of the list.
    h.command("downloads.find_previous");
    assert_eq!(picked(&mut h), ["alphabet.bin"]);
    h.command("downloads.find_previous");
    assert_eq!(picked(&mut h), ["alpha.bin"]);
    h.command("downloads.find_previous");
    assert_eq!(picked(&mut h), ["alphabet.bin"], "wraps round");
    // Its key, from the search box's own field: Shift-F3 types nothing and goes back.
    h.key("f3", &["shift"]);
    assert_eq!(picked(&mut h), ["alpha.bin"]);
    // And F3 forward again.
    h.key("f3", &[]);
    assert_eq!(picked(&mut h), ["alphabet.bin"]);
    // In the Downloads menu, with its key shown.
    assert!(!Command::FindPrevious.chords().is_empty());
    assert_eq!(Command::FindPrevious.label(), "Find previous");
    h.menu_item(&["Downloads", "Find previous"]);
}

#[test]
fn the_toolbar_is_customized_from_the_view_menu_and_remembered() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    assert!(h.shows("Grabber"));
    h.click("View");
    h.click("Customize toolbar");
    h.click("Grabber");
    h.until("Grabber gone", |h| !h.shows("Grabber"));
    h.click("View");
    h.click("Customize toolbar");
    h.click("Small buttons");
    h.until("small buttons", |h| kept(h).toolbar_style == "icons");
    let saved = kept(&mut h);
    assert_eq!(saved.toolbar_hidden, ["program.site_files"]);

    // Add URL moved one place right (dropped on the button after it, as the GPUI window
    // moves a button), and the style set to text only.
    use dm_shell::app::commands::Command;
    h.update(|view, cx| view.move_toolbar_button(Command::AddUrl, Command::DownloadNow, cx));
    h.click("View");
    h.click("Customize toolbar");
    h.click("Text only");
    h.until("text only", |h| kept(h).toolbar_style == "text");
    let saved = kept(&mut h);
    let order: Vec<&str> = dm_shell::app::commands::toolbar_in(&saved.toolbar_order)
        .iter()
        .map(|(_, label)| *label)
        .take(3)
        .collect();
    assert_eq!(order, ["Resume", "Add URL", "Stop"]);
    h.until_shown("Add URL");
}

#[test]
fn a_rows_handed_over_sign_in_is_never_read_back_out() {
    let session = crate::test_secrets::secret("session");
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.view(|view| {
        view.backend.add_with(
            &url,
            dm_core::AddOptions {
                later: true,
                cookies: format!("session={session}"),
                // The browser's own request headers: a bearer token among them.
                headers: vec![("Authorization".into(), format!("Bearer {session}-token"))],
                ..Default::default()
            },
        )
    })
    .unwrap();
    h.until("the row", |h| h.rows().len() == 1);
    // What a script, or an AI client through MCP, reads of the list.
    let listed = h.call("queue.list", serde_json::json!({})).unwrap();
    let text = listed.to_string();
    assert!(!text.contains(&session), "{text}");
    assert_eq!(listed[0]["signed_in"], true, "{text}");
    let seen = h.call("ui.inspect", serde_json::json!({})).unwrap();
    assert!(!seen.to_string().contains(&session));
    assert!(!h.texts().iter().any(|t| t.contains(&session)));
}

#[test]
fn the_status_bars_folder_never_covers_what_it_says() {
    let world = world();
    let deep = world.dir.path().join("a-long-folder-name-".repeat(12));
    let mut settings = world.settings();
    settings.download_dir = deep;
    let mut h = world.harness_with(settings);
    h.view(|view| {
        view.said = Some((
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

/// Clear finished takes completed and cancelled rows, so with only a failed row there is
/// nothing for it to do.
#[test]
fn clear_finished_is_greyed_with_only_failed_rows() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("gone.bin"), "Start now");
    h.until("the missing file to fail", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    if h.shows("The file doesn't exist") {
        h.click("OK");
    }
    assert!(
        h.menu_item(&["Downloads", "Delete Completed"]).1,
        "Delete Completed is offered for a failed row"
    );
    assert!(h
        .call(
            "command.run",
            serde_json::json!({ "id": "downloads.clear_finished" })
        )
        .is_err());
    assert_eq!(h.rows().len(), 1, "the failed row went");
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

/// A row added for later is queued, not stopped; Exit counts the running alone; counts take
/// real plurals.
#[test]
fn statuses_and_counts_say_what_is_so() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    let row = h.rows()[0].clone();
    assert_eq!(
        dm_desktop_base::rows::status_of(&row),
        "Waiting for Start queue"
    );
    let url = world.url("stalled.bin");
    h.view(|view| view.backend.add(&url)).unwrap();
    h.until("the stalled one to run", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    ask_exit(&mut h);
    h.until("the Exit question", |h| h.dialog_open("Exit dm?"));
    assert!(
        h.shows_part("1 download is still running."),
        "the Exit question counts the running one, singular"
    );
    assert!(!h.shows_part("(s)"));
}

/// A row added for later says it waits for Start queue, is counted in the status bar, and
/// keeps the size and folder Download details knew.
#[test]
fn a_row_added_for_later_says_what_it_waits_for_with_its_size_and_folder() {
    let world = world();
    let mut h = world.harness();
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&world.url("file.zip"));
    h.click("OK");
    h.until("Download details' size", |h| {
        h.dialog_open("Download details")
            && h.texts()
                .iter()
                .any(|t| t.starts_with(&dm_desktop_base::format::bytes(256 * 1024)))
    });
    h.click("Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    let row = h.rows()[0].clone();
    assert_eq!(row.state, State::Paused);
    assert_eq!(
        dm_desktop_base::rows::status_of(&row),
        "Waiting for Start queue"
    );
    assert_eq!(
        dm_desktop_base::rows::shown_size(&row),
        Some(PAYLOAD as u64)
    );
    let out = kept(&mut h).download_dir.display().to_string();
    h.until("the folder shown", |h| h.shows(&out));
    h.until_shown("Waiting for Start queue");
    h.until_shown("· 1 waiting for Start queue");
}

/// A finished row's size is its file's on disk, as a stream's MP4 is once remuxed.
#[test]
fn a_finished_rows_size_is_its_files() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.view(|view| view.backend.add(&url)).unwrap();
    h.until("the file to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    let path = h.rows()[0].path.clone();
    std::fs::write(&path, vec![0u8; 1000]).unwrap();
    // The sizes read once from the disk, read again.
    h.view(|view| view.saved = Default::default());
    h.until("the file's own size", |h| {
        h.rows()[0].progress.total == Some(1000)
    });
}

/// Rows added for later are counted under their kind by their names, before the server has
/// said what they are: a hundred queued videos showed one under Videos.
#[test]
fn queued_rows_are_sorted_into_their_kind_by_name() {
    let world = world();
    let mut h = world.harness();
    for name in ["clip.mp4", "song.mp3", "notes.txt"] {
        add_later(&mut h, &world, name);
    }
    h.until("three rows", |h| h.rows().len() == 3);
    assert!(h.rows().iter().all(|r| r.category.is_none()), "the premise");
    h.click("Videos");
    h.until("Videos", |h| {
        h.view(|view| view.place == Place::Kind(dm_core::Category::Videos))
    });
    assert_eq!(shown(&mut h), ["clip.mp4"]);
}

/// Properties of a row not yet started says its kind and where it will be saved, as the list
/// does, rather than dashes.
#[test]
fn properties_of_a_row_added_for_later_says_where_it_will_go() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("other.pdf"), "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    select(&mut h, "other.pdf");
    h.command("file.properties");
    h.until("its kind", |h| {
        h.dialog_open("other.pdf") || h.shows("Documents")
    });
    h.until("its kind", |h| {
        h.texts().iter().filter(|t| *t == "Documents").count() >= 2
    });
    h.until("the place it will be saved", |h| {
        h.texts()
            .iter()
            .any(|t| t.ends_with("/other.pdf") || t.ends_with("\\other.pdf"))
    });
}

/// A row whose kind is not known yet is under Other, so the kinds' counts add up to All's.
#[test]
fn a_row_of_no_known_kind_is_under_other() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    h.click("Other");
    h.until("Other", |h| {
        h.view(|view| view.place == Place::Kind(dm_core::Category::Other))
    });
    assert_eq!(
        shown(&mut h),
        ["stalled.bin"],
        "the queued .bin is in no place"
    );
}

/// A queue has one limit; Add picks a queue without one and is greyed once every queue has
/// one.
#[test]
fn each_queue_has_at_most_one_limit() {
    let world = world();
    let mut h = world.harness();
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.click("Downloads");
    h.reveal("Add a queue limit");
    let queues = h.dialog(|o: &mut crate::options::Options, _, _| o.queues.len());
    for _ in 0..queues + 1 {
        // Scrolled to first, as a person scrolls to what is below the fold.
        h.reveal("Add a queue limit");
        h.click("Add a queue limit");
    }
    let limits: Vec<String> = h.dialog(|o: &mut crate::options::Options, _, _| {
        o.draft
            .queue_limits
            .iter()
            .map(|l| l.queue.clone())
            .collect()
    });
    let mut unique = limits.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(limits.len(), queues, "{limits:?}");
    assert_eq!(
        unique.len(),
        limits.len(),
        "two limits for one queue: {limits:?}"
    );
    h.click("Cancel");
}

/// Removing finished and unfinished rows together says so, and what happens to the finished
/// files; it said none had fetched anything.
#[test]
fn remove_says_when_finished_and_unfinished_rows_are_mixed() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("it to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    h.add_url(&world.url("other.pdf"), "Add for later");
    h.until("both rows", |h| h.rows().len() == 2);
    h.command("file.select_all");
    h.command("file.remove");
    h.until("the question", |h| {
        h.shows_part("Some downloads aren't finished")
    });
    assert!(h.shows("The finished file is kept."));
    assert!(!h.shows_part("Nothing was downloaded yet"));
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

/// A new install sorts finished files into folders by kind and starts with the computer, in
/// the tray, so a browser's download has dm to land in.
#[test]
fn new_installs_sort_by_kind_and_start_at_login() {
    let fresh = dm_shell::Settings::default();
    assert!(fresh.categorize);
    assert!(fresh.start_on_login);
}

/// What a command acts on is the selection as the table shows it: the same rows in the same
/// order as filtering what is shown by the selection, under every sort.
#[test]
fn the_rows_a_command_acts_on_are_the_selected_ones_in_table_order() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    for name in ["file.zip", "other.pdf", "deep.pdf", "report.pdf"] {
        add_later(&mut h, &world, name);
    }
    h.until("four rows", |h| h.rows().len() == 4);
    let ids: Vec<_> = h.rows().iter().map(|r| r.id).collect();
    let picks = [
        vec![],
        vec![ids[0]],
        vec![ids[3], ids[1]],
        vec![ids[0], ids[1], ids[2], ids[3]],
    ];
    for picked in picks {
        h.call("ui.select", serde_json::json!({ "ids": picked }))
            .unwrap();
        for column in Column::ALL {
            // Ascending, then descending.
            for _ in 0..2 {
                h.update(|view, cx| view.sort_by(column, cx));
                let (expected, got) = h.view(|view| {
                    let expected: Vec<_> = view
                        .shown
                        .iter()
                        .filter_map(|&i| view.rows.get(i))
                        .filter(|r| picked.contains(&r.id))
                        .map(|r| r.id)
                        .collect();
                    let got: Vec<_> = view.chosen().into_iter().map(|r| r.id).collect();
                    (expected, got)
                });
                assert_eq!(got, expected, "{picked:?} under {column:?}");
            }
            // And back to the order added.
            h.update(|view, cx| view.sort_by(column, cx));
        }
    }
}
