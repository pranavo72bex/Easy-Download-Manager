use crate::support::*;
use crate::test_secrets;

#[test]
fn when_the_queue_runs_dry_dm_chimes_and_counts_down_to_what_was_chosen() {
    use dm_desktop::power::WhenDone;
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.sound_on_complete = true;
    harness.state_mut().run(Command::WhenDoneSleep);
    assert_eq!(harness.state().when_done, WhenDone::Sleep);

    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the countdown", |app| app.countdown.is_some());
    assert!(world
        .fake
        .opened
        .lock()
        .unwrap()
        .contains(&"chime".to_string()));
    assert_eq!(harness.state().when_done, WhenDone::Nothing, "once only");
    harness.run_steps(2);
    assert!(harness
        .query_by_label_contains("put the computer to sleep in")
        .is_some());
    click(&mut harness, "Cancel");
    assert!(harness.state().countdown.is_none());
    assert!(!world
        .fake
        .opened
        .lock()
        .unwrap()
        .iter()
        .any(|o| o.starts_with("power")));

    // Again, and this time it is let happen.
    harness.state_mut().run(Command::WhenDoneShutDown);
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the countdown", |app| app.countdown.is_some());
    harness.run_steps(2);
    click(&mut harness, "Now");
    assert_eq!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.starts_with("power"))
            .collect::<Vec<_>>(),
        ["power ShutDown"]
    );

    // Exit counts down as the others do, then quits (it quit
    // the moment the last download ended, with no word).
    harness.state_mut().run(Command::WhenDoneQuit);
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    quit_after_its_countdown(&mut harness);
}

/// The places' groups open and shut from their own row, as a navigation
/// view's expandable items do.
#[test]
fn a_group_of_places_shuts_and_opens_from_its_row() {
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(3);
    assert!(harness.query_by_label_contains("Videos (").is_some());
    click(&mut harness, "Everything");
    assert!(
        harness.query_by_label_contains("Videos (").is_none(),
        "Everything did not shut"
    );
    assert!(harness.query_by_label_contains("In progress (").is_some());
    click(&mut harness, "Everything");
    assert!(
        harness.query_by_label_contains("Videos (").is_some(),
        "Everything did not open again"
    );
}

/// A row not saved yet shows the name chosen for it, not its address's.
#[test]
fn a_row_shows_the_name_chosen_for_it_before_it_is_saved() {
    let world = world();
    let mut harness = world.harness();
    let later = |name: Option<&str>| dm_core::AddOptions {
        later: true,
        queue: dm_core::MAIN_QUEUE.into(),
        save_name: name.map(Into::into),
        ..Default::default()
    };
    harness
        .state_mut()
        .add(&[world.url("download.php?id=5")], later(Some("mine.zip")))
        .unwrap();
    harness
        .state_mut()
        .add(&[world.url("other.php?id=6")], later(None))
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(3);
    let names: Vec<String> = harness
        .state()
        .rows()
        .iter()
        .map(|r| dm_desktop::app::name_of(r))
        .collect();
    assert!(names.contains(&"mine.zip".to_string()), "{names:?}");
    assert!(names.contains(&"other.php".to_string()), "{names:?}");
    assert!(harness.query_by_label("mine.zip").is_some());
}

#[test]
fn the_toolbar_acts_on_the_selected_row() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "the row to start", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    select(&mut harness, "stalled.bin");

    click(&mut harness, "Stop");
    assert_eq!(harness.state().rows()[0].state, State::Paused);
    click(&mut harness, "Resume");
    assert!(matches!(
        harness.state().rows()[0].state,
        State::Queued | State::Running
    ));
    click(&mut harness, "Stop all");
    assert_eq!(harness.state().rows()[0].state, State::Paused);
    // Unfinished, so Delete asks first.
    click(&mut harness, "Remove");
    assert!(harness
        .query_by_label("Remove an unfinished download")
        .is_some());
    // And the control protocol names it by the title it shows.
    assert!(harness
        .state()
        .dialogs
        .open_names()
        .contains(&"Remove an unfinished download".to_string()));
    click(&mut harness, "Keep it");
    assert_eq!(harness.state().rows().len(), 1, "Keep deleted it");
    click(&mut harness, "Remove");
    click(&mut harness, "Yes, remove");
    assert!(harness.state().rows().is_empty());
}

#[test]
fn the_category_tree_filters_the_one_list() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    until(&mut harness, "one to finish and one to fail", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
            && app
                .rows()
                .iter()
                .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(2);

    // In progress is what is under way: the stalled download,
    // not the one that failed.
    harness.get_by_label_contains("Done (1)").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("file.zip").is_some());
    assert!(harness.query_by_label("stalled.bin").is_none());
    harness.get_by_label_contains("In progress (1)").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("stalled.bin").is_some());
    assert!(harness.query_by_label("file.zip").is_none());
    assert!(harness.query_by_label("secret.zip").is_none());
    // Both archives, the failed one too, known by its name though the
    // server never said what it was.
    harness.get_by_label_contains("Archives (2)").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("file.zip").is_some());
    assert!(harness.query_by_label("secret.zip").is_some());
}

#[test]
fn clicking_a_heading_sorts_and_clicking_again_reverses() {
    let world = world();
    let mut harness = world.harness();
    for name in ["b.bin", "c.bin", "a.bin"] {
        harness
            .state()
            .backend()
            .add_with(
                &world.url(name),
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.run_steps(2);
    click(&mut harness, "Name");
    let names = |h: &Harness<'_, DmApp>| -> Vec<String> {
        h.state()
            .visible()
            .iter()
            .map(|r| dm_desktop::app::name_of(r))
            .collect()
    };
    assert_eq!(names(&harness), ["a.bin", "b.bin", "c.bin"]);
    assert_eq!(harness.state().sort, Some((Column::Name, true)));
    click(&mut harness, "Name");
    assert_eq!(names(&harness), ["c.bin", "b.bin", "a.bin"]);
}

#[test]
fn a_column_turned_off_is_gone_and_comes_back() {
    // Turning one off is saved in the settings file other tests read.
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add_with(
            &world.url("x.bin"),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    assert!(harness.query_by_label("Speed").is_some());
    click(&mut harness, "View");
    // A sub-menu's name carries its arrow: "Columns ⏵".
    harness.get_by_label_contains("Columns").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(egui::accesskit::Role::CheckBox, "Speed")
        .click();
    harness.run_steps(3);
    // Still open after a tick: a second column is a second click, not a
    // second trip through View.
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::CheckBox, "Speed")
            .is_some(),
        "the Columns menu closed at a tick"
    );
    harness
        .get_by_role_and_label(egui::accesskit::Role::CheckBox, "Note")
        .click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(egui::accesskit::Role::CheckBox, "Note")
        .click();
    harness.run_steps(3);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().hidden_columns.contains(&Column::Rate));
    assert!(
        harness.query_by_label("Speed").is_none(),
        "the column is still drawn"
    );
    assert_eq!(
        harness.state().backend().settings().columns_hidden,
        ["rate"],
        "not kept for the next launch"
    );
    harness.state_mut().hidden_columns.clear();
    harness.run_steps(2);
    assert!(harness.query_by_label("Speed").is_some());

    // Moved: Size after Status.
    let x = |harness: &egui_kittest::Harness<'_, dm_desktop::app::DmApp>, name: &str| {
        harness.get_by_label(name).rect().min.x
    };
    assert!(x(&harness, "Size") < x(&harness, "Status"));
    click(&mut harness, "View");
    harness.get_by_label_contains("Columns").click();
    harness.run_steps(3);
    harness.get_by_label("Move Size right").click();
    harness.run_steps(3);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(
        x(&harness, "Size") > x(&harness, "Status"),
        "the headings were not redrawn in the new order"
    );
    let saved = harness.state().backend().settings();
    assert_eq!(
        Column::ordered(&saved.column_order)[..4],
        [Column::Name, Column::Status, Column::Size, Column::TimeLeft]
    );
    // Put back for the other tests.
    let mut wanted = saved;
    wanted.column_order.clear();
    wanted.columns_hidden.clear();
    harness.state_mut().apply_settings(wanted).unwrap();
}

/// Kept to the column: a row added for later shows the folder it
/// will go to, without that guess written into the row a command reads; and
/// a name that does not say its kind shows none, the server deciding.
#[test]
fn a_row_added_for_later_shows_its_folder_without_owning_it() {
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        categorize: true,
        show_progress: false,
        show_complete: false,
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1400.0, 700.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    harness.run_steps(2);
    let later = dm_core::AddOptions {
        later: true,
        queue: dm_core::MAIN_QUEUE.into(),
        ..Default::default()
    };
    harness
        .state_mut()
        .add(
            &[world.url("file.zip"), world.url("download.php?id=5")],
            later,
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(3);
    let app = harness.state();
    let mut categories = app.applied.categories();
    let rows = app.visible();
    let zip = rows.iter().find(|r| r.url.ends_with("file.zip")).unwrap();
    let php = rows
        .iter()
        .find(|r| r.url.contains("download.php"))
        .unwrap();
    assert_eq!(
        zip.save_dir, None,
        "the planned folder was written into the row"
    );
    let planned = app.planned_folder(zip, &mut categories).unwrap();
    assert!(planned.ends_with("Archives"), "{planned}");
    assert_eq!(app.planned_folder(php, &mut categories), None);
    assert!(
        harness.query_all_by_label(&planned).next().is_some(),
        "the Folder column does not show {planned}"
    );
}

#[test]
fn find_next_selects_the_next_row_with_the_name() {
    let world = world();
    let mut harness = world.harness();
    for name in ["alpha.bin", "beta.bin", "alphabet.bin"] {
        harness
            .state()
            .backend()
            .add_with(
                &world.url(name),
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.state_mut().run(Command::Find);
    harness.run_steps(2);
    type_into(&mut harness, "Find what", "alpha");
    click(&mut harness, "Find next");
    let picked = |h: &Harness<'_, DmApp>| {
        h.state()
            .targets()
            .iter()
            .map(dm_desktop::app::name_of)
            .collect::<Vec<_>>()
    };
    assert_eq!(picked(&harness), ["alpha.bin"]);
    click(&mut harness, "Find next");
    assert_eq!(picked(&harness), ["alphabet.bin"]);
}

#[test]
fn the_toolbar_is_customized_from_the_view_menu_and_remembered() {
    use egui::accesskit::Role;
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let site_files_buttons = |h: &Harness<'_, DmApp>| {
        h.query_all_by_role_and_label(Role::Button, "Site files")
            .count()
    };
    assert_eq!(site_files_buttons(&harness), 1);

    harness.get_by_label("View").click();
    harness.run_steps(3);
    harness.get_by_label_contains("Customize toolbar").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::CheckBox, "Site files")
        .click();
    harness.run_steps(3);
    // A click in a menu closes it, so it is opened again.
    harness.get_by_label("View").click();
    harness.run_steps(3);
    harness.get_by_label_contains("Customize toolbar").click();
    harness.run_steps(3);
    harness.get_by_label("Small buttons").click();
    harness.run_steps(3);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);

    let saved = harness.state().backend().settings();
    assert_eq!(saved.toolbar_style, "icons");
    assert_eq!(saved.toolbar_hidden, ["program.site_files"]);
    assert_eq!(
        site_files_buttons(&harness),
        0,
        "the hidden button is still drawn"
    );
    // Small buttons keep their names for a screen reader.
    assert_eq!(
        harness
            .query_all_by_role_and_label(Role::Button, "Add URL")
            .count(),
        1
    );

    // Add URL moved one place right, and the style set to text only.
    harness.get_by_label("View").click();
    harness.run_steps(3);
    harness.get_by_label_contains("Customize toolbar").click();
    harness.run_steps(3);
    harness.get_by_label("Move Add URL right").click();
    harness.run_steps(3);
    harness.get_by_label("View").click();
    harness.run_steps(3);
    harness.get_by_label_contains("Customize toolbar").click();
    harness.run_steps(3);
    harness.get_by_label("Text only").click();
    harness.run_steps(3);
    let saved = harness.state().backend().settings();
    assert_eq!(saved.toolbar_style, "text");
    let order: Vec<&str> = dm_desktop::app::toolbar_in(&saved.toolbar_order)
        .iter()
        .map(|(_, _, label)| *label)
        .take(3)
        .collect();
    assert_eq!(order, ["Resume", "Add URL", "Stop"]);
}

#[test]
fn a_rows_handed_over_sign_in_is_never_read_back_out() {
    let session = test_secrets::secret("session");
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                later: true,
                cookies: format!("session={session}"),
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    // What a script, or an AI client through MCP, reads of the list.
    let listed = harness
        .state_mut()
        .control_call("queue.list", &serde_json::json!({}))
        .unwrap();
    let text = listed.to_string();
    assert!(!text.contains(&session), "{text}");
    assert_eq!(listed[0]["signed_in"], true, "{text}");
    let seen = harness
        .state_mut()
        .control_call("ui.inspect", &serde_json::json!({}))
        .unwrap();
    assert!(!seen.to_string().contains(&session));
}

#[test]
fn the_status_bars_folder_never_covers_what_it_says() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(960.0, 660.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    let deep = world.dir.path().join("a-long-folder-name-".repeat(12));
    harness.state_mut().applied.download_dir = deep.clone();
    harness.state_mut().said =
        "Something long was said here, long enough to fill much of the status bar.".into();
    harness.run_steps(3);
    let words = harness.get_by_label_contains("Nothing downloading").rect();
    let said = harness
        .get_by_label_contains("Something long was said")
        .rect();
    if let Some(folder) = harness.query_by_label_contains("a-long-folder-name-") {
        let at = folder.rect();
        assert!(
            at.min.x >= said.max.x.max(words.max.x) - 0.5,
            "the folder {at:?} is drawn over the status {said:?}"
        );
        assert!(at.max.x <= 960.0);
    }
}

/// A closed schedule is said in the status bar with when it
/// opens, and a held row says why it waits.
#[test]
fn the_status_bar_says_the_schedule_holds_downloads_and_until_when() {
    let _file = settings_file();
    use chrono::Timelike;
    let world = world();
    let mut harness = world.harness();
    let now = chrono::Local::now();
    let from = format!("{:02}:{:02}", (now.hour() + 2) % 24, now.minute());
    let to = format!("{:02}:{:02}", (now.hour() + 3) % 24, now.minute());
    let mut wanted = harness.state().applied.clone();
    wanted.schedule = dm_core::Schedule {
        wake: false,
        enabled: true,
        from: from.clone(),
        to,
        days: Vec::new(),
    };
    harness.state_mut().apply_settings(wanted).unwrap();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the schedule to hold the row", |app| {
        app.rows()
            .iter()
            .any(|r| r.state == State::Queued || r.paused_by_schedule)
    });
    harness.run_steps(2);
    // "01:00", or "Sun 01:00" when that is tomorrow.
    let said = harness
        .query_by_label_contains("waiting for the schedule (opens at ")
        .map(|n| {
            let node = n.accesskit_node();
            node.label()
                .or(node.value())
                .unwrap_or_default()
                .to_string()
        })
        .expect("the status bar does not say the schedule holds downloads");
    assert!(said.contains(&format!("{from})")), "{said}");
    let why = harness.state().held_because().unwrap_or_default();
    assert!(
        why.contains("starts by itself at ") && why.contains(&from),
        "{why}"
    );
    let mut off = harness.state().applied.clone();
    off.schedule.enabled = false;
    harness.state_mut().apply_settings(off).unwrap();
}

/// In the window: Clear finished takes completed and cancelled
/// rows, so with only a failed row there is nothing for it to do.
#[test]
fn clear_finished_is_greyed_with_only_failed_rows() {
    let world = world();
    let mut h = world.harness();
    add_url(&mut h, &world.url("gone.bin"), "Start now");
    until(&mut h, "the missing file to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    assert!(!h.state().enabled(Command::DeleteCompleted));
    assert!(
        greyed(&mut h, &["Downloads", "Clear finished"]),
        "Clear all finished is offered for a failed row"
    );
    h.state_mut().run(Command::DeleteCompleted);
    assert_eq!(h.state().rows().len(), 1, "the failed row went");
}

/// Resume on a row the schedule holds offers to start it now,
/// outside the schedule, and it then downloads while the schedule stays shut.
#[test]
fn resume_on_a_held_row_offers_to_start_it_outside_the_schedule() {
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
    until(&mut harness, "the schedule to hold it", |app| {
        app.rows().iter().any(|r| r.paused_by_schedule)
    });
    // Start now under a shut schedule asks at once; kept waiting here, to be
    // asked again by Resume.
    if harness.query_by_label("Keep waiting").is_some() {
        click(&mut harness, "Keep waiting");
    }
    select(&mut harness, "file.zip");
    harness.state_mut().run(Command::DownloadNow);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Start outside the schedule?")
            .is_some(),
        "Resume on a held row asked nothing"
    );
    // The question's Start now; the toolbar's reads the same for a held row.
    let question = harness.get_by_label("Start outside the schedule?").rect();
    harness
        .get_all_by_label("Start now")
        .find(|n| question.contains_rect(n.rect()))
        .expect("the question's Start now")
        .click();
    harness.run_steps(3);
    until(&mut harness, "it to finish outside the schedule", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
    // Nothing is said about waiting once nothing waits.
    harness.state_mut().said =
        dm_desktop::i18n::Said::new("Waiting for the schedule or speed limit.", &[]);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains("Waiting for the schedule or speed limit")
            .is_none(),
        "a stale 'Waiting for the schedule' with nothing held"
    );
    let mut off = harness.state().applied.clone();
    off.schedule.enabled = false;
    harness.state_mut().apply_settings(off).unwrap();
}

/// Find never covers the row it found.
#[test]
fn find_stays_clear_of_the_row_it_found() {
    let world = world();
    let mut harness = world.harness();
    for n in 1..=25 {
        add_named(&mut harness, &world, &format!("a{n:02}.zip"));
    }
    harness.run_steps(3);
    harness.state_mut().run(Command::Find);
    harness.run_steps(3);
    for wanted in ["a07", "a05", "a09", "a20"] {
        harness.state_mut().dialogs.find_text = wanted.into();
        harness.state_mut().find_next();
        harness.run_steps(6);
        let id = harness.state().cursor.expect("found");
        let row = *harness.state().row_rects.get(&id).expect("drawn");
        let find = harness
            .ctx
            .memory(|m| m.area_rect(egui::Id::unique(("dm-dialog", "Find"))))
            .expect("Find is open");
        assert!(
            !find.intersects(row),
            "Find {find:?} covers {wanted} at {row:?}"
        );
    }
}

/// The progress window's bar is laid out as the file is, a piece for each
/// connection, and says how far it has come and in how many pieces.
#[test]
fn the_pieces_bar_says_how_far_and_in_how_many_pieces() {
    let mut harness = Harness::new_ui(|ui| {
        dm_desktop::dialogs::pieces_bar(
            ui,
            1000,
            &[(0, 400, 400), (400, 300, 100), (700, 300, 0)],
            300.0,
        );
    });
    harness.run();
    assert!(harness
        .query_by_label("50% downloaded, in 3 pieces")
        .is_some());
}

/// A row added for later is queued, not stopped; Exit counts the
/// running alone; counts take real plurals.
#[test]
fn statuses_and_counts_say_what_is_so() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    let row = harness.state().rows()[0].clone();
    assert_eq!(dm_desktop::app::status_of(&row), "Waiting for Start queue");
    harness
        .state()
        .backend()
        .add(&world.url("stalled.bin"))
        .unwrap();
    until(&mut harness, "the stalled one to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    harness.state_mut().ask_quit();
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains("1 download is still running.")
            .is_some(),
        "the Exit question counts the running one, singular"
    );
    assert!(harness.query_by_label_contains("(s)").is_none());
}

/// A row added for later says it waits for Start queue, is counted
/// in the status bar, and keeps the size and folder Download details knew.
#[test]
fn a_row_added_for_later_says_what_it_waits_for_with_its_size_and_folder() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.query_by_label("256.0 KB").is_none() {
        assert!(Instant::now() < deadline, "no size in Download details");
        harness.run_steps(2);
        std::thread::sleep(Duration::from_millis(25));
    }
    click(&mut harness, "Add for later");
    harness.state_mut().refresh();
    harness.run_steps(3);
    let row = harness.state().visible()[0].clone();
    assert_eq!(row.state, State::Paused);
    assert_eq!(dm_desktop::app::status_of(&row), "Waiting for Start queue");
    assert_eq!(dm_desktop::table::shown_size(&row), Some(PAYLOAD as u64));
    let out = harness.state().applied.download_dir.display().to_string();
    let mut categories = harness.state().applied.categories();
    assert_eq!(
        harness.state().planned_folder(&row, &mut categories),
        Some(out.clone())
    );
    assert!(
        harness.query_all_by_label(&out).next().is_some(),
        "no folder shown"
    );
    assert!(harness.query_by_label("Waiting for Start queue").is_some());
    assert!(harness
        .query_by_label("· 1 waiting for Start queue")
        .is_some());
}

/// A finished row's size is its file's on disk, as a stream's
/// MP4 is once remuxed.
#[test]
fn a_finished_rows_size_is_its_files() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("file.zip"))
        .unwrap();
    until(&mut harness, "the file to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let path = harness.state().rows()[0].path.clone();
    std::fs::write(&path, vec![0u8; 1000]).unwrap();
    harness.state_mut().saved.clear();
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows()[0].progress.total, Some(1000));
}

/// The bar under a running download's status is read by a screen
/// reader as its progress, with its value; it was paint alone.
#[test]
fn a_rows_progress_bar_says_how_far_it_has_come() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "the download to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    harness.run_steps(3);
    let bar = harness
        .query_all_by_role(egui::accesskit::Role::ProgressIndicator)
        .find(|n| n.accesskit_node().label().as_deref() == Some("Progress"))
        .expect("the row has no progress bar a screen reader finds");
    assert_eq!(bar.accesskit_node().numeric_value(), Some(0.0));
}

/// Rows added for later are counted under their kind by their
/// names, before the server has said what they are: a hundred queued
/// videos showed one under Videos.
#[test]
fn queued_rows_are_sorted_into_their_kind_by_name() {
    let world = world();
    let mut harness = world.harness();
    for name in ["clip.mp4", "song.mp3", "notes.txt"] {
        harness
            .state()
            .backend()
            .add_with(
                &world.url(name),
                dm_core::AddOptions {
                    later: true,
                    queue: dm_core::MAIN_QUEUE.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.run_steps(2);
    assert!(
        harness.state().rows().iter().all(|r| r.category.is_none()),
        "the premise"
    );
    harness.state_mut().place = dm_desktop::app::Place::Kind(dm_core::Category::Videos);
    let shown: Vec<String> = harness
        .state()
        .visible()
        .iter()
        .map(|r| dm_desktop::app::name_of(r))
        .collect();
    assert_eq!(shown, ["clip.mp4"]);
}

/// Properties of a row not yet started says its kind and
/// where it will be saved, as the list does, rather than dashes.
#[test]
fn properties_of_a_row_added_for_later_says_where_it_will_go() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    select(&mut harness, "other.pdf");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(3);
    assert!(harness.query_by_label("Documents").is_some(), "no kind");

    assert!(
        harness.query_all_by_label_contains("other.pdf").any(|n| {
            n.accesskit_node()
                .value()
                .is_some_and(|v| v.ends_with("/other.pdf") || v.ends_with("\\other.pdf"))
        }),
        "no place it will be saved"
    );
}

/// A row whose kind is not known yet is under Other, so the
/// kinds' counts add up to All's.
#[test]
fn a_row_of_no_known_kind_is_under_other() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Add for later");
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Other (1)").is_some(),
        "the queued .bin is in no place"
    );
}

/// A queue has one limit; Add picks a queue without one and
/// is greyed once every queue has one.
#[test]
fn each_queue_has_at_most_one_limit() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    harness.state_mut().dialogs.options.as_mut().unwrap().tab = dm_desktop::dialogs::Tab::Downloads;
    harness.run_steps(3);
    let queues = harness.state().backend().queues();
    for _ in 0..queues.len() + 1 {
        // Scrolled to first, as a person scrolls to what is below the fold.
        click(&mut harness, "Add a queue limit");
    }
    let limits: Vec<String> = harness
        .state()
        .dialogs
        .options
        .as_ref()
        .unwrap()
        .draft
        .queue_limits
        .iter()
        .map(|l| l.queue.clone())
        .collect();
    let mut unique = limits.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(limits.len(), queues.len(), "{limits:?}");
    assert_eq!(
        unique.len(),
        limits.len(),
        "two limits for one queue: {limits:?}"
    );
    click(&mut harness, "Cancel");
}

/// Removing finished and unfinished rows together says so,
/// and what happens to the finished files; it said none had fetched
/// anything.
#[test]
fn remove_says_when_finished_and_unfinished_rows_are_mixed() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = false;
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    harness.state_mut().run(Command::SelectAll);
    harness.state_mut().run(Command::Remove);
    harness.run_steps(3);
    assert!(harness
        .query_by_label_contains("Some downloads aren't finished")
        .is_some());
    assert!(harness
        .query_by_label("The finished file is kept.")
        .is_some());
    assert!(harness
        .query_by_label_contains("Nothing was downloaded yet")
        .is_none());
}

/// The tray says how many downloads run and how fast; nothing to say when
/// none do.
#[test]
fn the_tray_says_how_many_downloads_run_and_how_fast() {
    let world = world();
    let mut harness = world.harness();
    assert_eq!(harness.state().tray_activity(), None);
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "the download to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    let said = harness.state().tray_activity().unwrap_or_default();
    assert!(said.starts_with("1 downloading at "), "{said}");
}

/// A new install sorts finished files into folders by kind and starts with
/// the computer, in the tray, so a browser's download has dm to land in.
#[test]
fn new_installs_sort_by_kind_and_start_at_login() {
    let fresh = Settings::default();
    assert!(fresh.categorize);
    assert!(fresh.start_on_login);
}

/// What a command acts on is the selection as the table shows it: the same
/// rows in the same order as filtering what is shown by the selection, under
/// every sort. `targets` copies and sorts only the selected rows, where it
/// once did all of them.
#[test]
fn the_rows_a_command_acts_on_are_the_selected_ones_in_table_order() {
    use dm_desktop::app::Column;
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    for name in ["file.zip", "other.pdf", "deep.pdf", "report.pdf"] {
        add_url(&mut harness, &world.url(name), "Start now");
    }
    until(&mut harness, "four rows", |app| app.rows().len() == 4);
    let ids: Vec<_> = harness.state().rows().iter().map(|r| r.id).collect();
    let picks = [
        vec![],
        vec![ids[0]],
        vec![ids[3], ids[1]],
        vec![ids[0], ids[1], ids[2], ids[3]],
    ];
    for picked in picks {
        harness.state_mut().selected = picked.iter().copied().collect();
        for column in Column::ALL {
            // Ascending, then descending, then a plain order.
            for _ in 0..2 {
                harness.state_mut().sort_by(column);
                let app = harness.state();
                let expected: Vec<_> = app
                    .visible()
                    .into_iter()
                    .filter(|r| picked.contains(&r.id))
                    .map(|r| r.id)
                    .collect();
                let got: Vec<_> = app.targets().into_iter().map(|r| r.id).collect();
                assert_eq!(got, expected, "{picked:?} under {column:?}");
            }
        }
    }
}

#[test]
#[ignore = "a timing, run by hand: cargo test -p dm-desktop --test window frame_cost -- --ignored --nocapture"]
fn frame_cost_of_a_long_list() {
    use dm_desktop::app::Column;
    let world = world();
    let mut harness = world.harness();
    let urls: Vec<String> = (0..5000)
        .map(|i| world.url(&format!("file{i}.zip")))
        .collect();
    let items = urls
        .iter()
        .map(|u| (u.clone(), Default::default()))
        .collect();
    let _ = harness.state().backend.add_each(items);
    harness.state_mut().refresh();
    assert!(harness.state().rows().len() >= 5000);
    harness.state_mut().sort_by(Column::Name);
    let app = harness.state();
    let frames = 200;
    let t = std::time::Instant::now();
    for _ in 0..frames {
        std::hint::black_box(app.visible());
    }
    let copied = t.elapsed() / frames;
    let t = std::time::Instant::now();
    for _ in 0..frames {
        std::hint::black_box(app.visible_shared());
    }
    let cached = t.elapsed() / frames;
    let t = std::time::Instant::now();
    for _ in 0..frames {
        std::hint::black_box(app.count(&dm_desktop::app::Place::All));
    }
    let counted = t.elapsed() / frames;
    println!("5000 rows sorted by name, per frame: copy+sort {copied:?}, cached {cached:?}, count {counted:?}");
}
