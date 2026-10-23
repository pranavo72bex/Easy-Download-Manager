use crate::support::*;

fn is_there(h: &Harness<'_, DmApp>, name: &str) -> bool {
    h.state()
        .rows()
        .iter()
        .any(|r| dm_desktop::app::name_of(r) == name)
}

/// The window walked in each language: the empty list, the menus, every
/// dialog and every Options tab. Whatever it asks to translate and a
/// catalog lacks is reported, which a scan of the source cannot see for
/// words chosen at run time (a tab's name, a choice in a list).
#[test]
fn every_dialog_in_every_language_is_translated() {
    let mut untranslated = std::collections::BTreeSet::new();
    for (code, _) in dm_desktop::i18n::LANGUAGES
        .iter()
        .filter(|(c, _)| *c != "en")
    {
        let world = world();
        let mut harness = world.harness();
        dm_desktop::i18n::set(code);
        dm_desktop::i18n::record_missing();
        harness.run_steps(3);
        for menu in ["Program", "File", "Downloads", "View", "Help"] {
            harness.get_by_label(dm_desktop::i18n::t(menu)).click();
            harness.run_steps(3);
            harness.key_press(egui::Key::Escape);
            harness.run_steps(2);
        }
        harness
            .state()
            .backend()
            .add_with(
                &world.url("stalled.bin"),
                dm_core::AddOptions {
                    later: true,
                    queue: dm_core::MAIN_QUEUE.into(),
                    ..Default::default()
                },
            )
            .unwrap();
        harness.state_mut().refresh();
        harness.run_steps(3);
        let id = harness.state().rows()[0].id;
        let row = harness.state().rows()[0].clone();
        harness.state_mut().dialogs.open_add_url(None);
        harness.run_steps(3);
        harness.state_mut().dialogs.add_url = None;
        harness
            .state_mut()
            .dialogs
            .open_file_info(vec![world.url("x.bin")]);
        harness.run_steps(3);
        harness.state_mut().dialogs.file_info = None;
        harness.state_mut().dialogs.progress.insert(id);
        harness.run_steps(3);
        harness.state_mut().dialogs.progress.clear();
        harness.state_mut().dialogs.complete.insert(id);
        harness.run_steps(3);
        harness.state_mut().dialogs.complete.clear();
        for ids in [vec![id], vec![id, id]] {
            harness.state_mut().dialogs.confirm_remove = Some(ids);
            harness.run_steps(3);
        }
        harness.state_mut().dialogs.confirm_remove = None;
        harness.state_mut().dialogs.open_properties(&row);
        harness.run_steps(3);
        harness.state_mut().dialogs.properties.clear();
        for command in [
            Command::AddSeries,
            Command::SiteFiles,
            Command::Scheduler,
            Command::SpeedLimiterSettings,
            Command::About,
        ] {
            harness.state_mut().run(command);
            harness.run_steps(3);
        }
        let settings = harness.state().applied.clone();
        harness.state_mut().dialogs = Default::default();
        harness.state_mut().dialogs.open_options(&settings);
        for tab in [
            dm_desktop::dialogs::Tab::General,
            dm_desktop::dialogs::Tab::FileTypes,
            dm_desktop::dialogs::Tab::SaveTo,
            dm_desktop::dialogs::Tab::Downloads,
            dm_desktop::dialogs::Tab::Connection,
            dm_desktop::dialogs::Tab::Proxy,
            dm_desktop::dialogs::Tab::SiteLogins,
        ] {
            if let Some(options) = harness.state_mut().dialogs.options.as_mut() {
                options.tab = tab;
            }
            harness.run_steps(3);
        }
        for missing in dm_desktop::i18n::take_missing() {
            untranslated.insert(format!("{code}: {missing:?}"));
        }
        dm_desktop::i18n::set("en");
    }
    assert!(untranslated.is_empty(), "untranslated: {untranslated:#?}");
}

/// Every command in the registry runs without falling over, with a row
/// selected in each state — the menus, toolbar, keyboard and control
/// protocol all end here.
#[test]
fn every_command_runs_against_rows_in_every_state() {
    // The speed limiter's toggle saves the settings file the settings
    // tests read back.
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("stalled.bin"), "Add for later");
    until(&mut harness, "one to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    *world.fake.clipboard.lock().unwrap() = Some(world.url("clip.bin"));
    for command in Command::ALL {
        if *command == Command::Quit
            || *command == Command::Remove
            || *command == Command::Import
            || *command == Command::Export
        {
            continue;
        }
        harness.state_mut().run(Command::SelectAll);
        harness.state_mut().run(*command);
        harness.state_mut().dialogs = Default::default();
        harness.run_steps(2);
    }
    harness.state_mut().run(Command::Quit);
    assert!(harness.state().quit_asked, "Exit did not ask to quit");
}

#[test]
fn every_control_has_a_name_a_screen_reader_can_say() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    let settings = harness.state().applied.clone();
    harness.state_mut().dialogs.open_options(&settings);
    harness.state_mut().dialogs.open_add_url(None);
    harness.run_steps(3);

    use egui::accesskit::Role;
    let mut unnamed = Vec::new();
    for role in [
        Role::Button,
        Role::CheckBox,
        Role::TextInput,
        Role::ComboBox,
    ] {
        for node in harness.query_all_by_role(role) {
            let label = node.accesskit_node().label().unwrap_or_default();
            if label.trim().is_empty() {
                unnamed.push(format!("{role:?} {:?}", node.accesskit_node().value()));
            }
        }
    }
    assert!(unnamed.is_empty(), "controls with no name: {unnamed:?}");

    // The window itself, which a screen reader names before anything in it.
    assert_eq!(
        harness.root().accesskit_node().label().as_deref(),
        Some("dm"),
        "the window has no name a screen reader can say"
    );
}

/// Every menu item, clicked by its name on screen, does what it says: a
/// dialog opens, a row changes state, a file opens, a setting changes.
/// "Check for updates" is found and enabled but not clicked: it asks GitHub,
/// and a test makes no request off this computer.
#[test]
fn every_menu_item_clicked_does_what_it_says() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    add_url(&mut h, &world.url("file.zip"), "Start now");
    add_url(&mut h, &world.url("stalled.bin"), "Start now");
    add_url(&mut h, &world.url("other.pdf"), "Add for later");
    until(&mut h, "file.zip to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    h.run_steps(3);
    let done = row(&h, "file.zip");

    // View
    press(&mut h, &["View", "Categories"]);
    assert!(
        !h.state().show_categories,
        "View › Categories did not hide the tree"
    );
    press(&mut h, &["View", "Categories"]);
    assert!(h.state().show_categories);
    press(&mut h, &["View", "Toolbar"]);
    assert!(!h.state().show_toolbar, "View › Toolbar did not hide it");
    press(&mut h, &["View", "Toolbar"]);
    assert!(h.state().show_toolbar);
    for (item, column) in [
        ("Sort by name", Column::Name),
        ("Sort by size", Column::Size),
        ("Sort by status", Column::Status),
        ("Sort by time left", Column::TimeLeft),
        ("Sort by speed", Column::Rate),
        ("Sort by date", Column::LastTry),
        ("Sort by note", Column::Description),
    ] {
        h.state_mut().sort = None;
        press(&mut h, &["View", "Sort by", item]);
        assert_eq!(h.state().sort, Some((column, true)), "{item}");
    }

    // Help
    press(&mut h, &["Help", "Help"]);
    assert!(world
        .fake
        .opened
        .lock()
        .unwrap()
        .iter()
        .any(|o| o == dm_desktop::update::HELP));
    press(&mut h, &["Help", "About dm"]);
    assert!(h.state().dialogs.about_open, "Help › About dm");
    h.state_mut().dialogs.about_open = false;
    h.get_by_label("Help").click();
    h.run_steps(3);
    let check = h
        .query_all_by_label("Check for updates")
        .last()
        .expect("Check for updates");
    assert!(
        !check.accesskit_node().is_disabled(),
        "Check for updates is greyed out"
    );
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    // Tasks
    press(&mut h, &["Program", "Add URL"]);
    assert!(
        h.state().dialogs.add_url.is_some(),
        "Program › Add new download"
    );
    h.state_mut().dialogs.add_url = None;
    press(&mut h, &["Program", "Add numbered series"]);
    assert!(
        h.state().dialogs.series.is_some(),
        "Program › Add numbered series"
    );
    h.state_mut().dialogs.series = None;
    *world.fake.clipboard.lock().unwrap() = Some(world.url("clip.bin"));
    press(&mut h, &["Program", "Add links from clipboard"]);
    assert!(h.state().dialogs.clip_links.is_some(), "… from clipboard");
    h.state_mut().dialogs.clip_links = None;
    press(&mut h, &["Program", "Site files"]);
    assert!(
        h.state().dialogs.site_files.is_some(),
        "Program › Site files"
    );
    h.state_mut().dialogs.site_files = None;
    let list = world.dir.path().join("list.dmlist");
    *world.fake.chosen.lock().unwrap() = Some(list.clone());
    press(&mut h, &["Program", "Export"]);
    let exported = std::fs::read_to_string(&list).unwrap();
    assert!(
        ["file.zip", "stalled.bin", "other.pdf"]
            .iter()
            .all(|n| exported.contains(n)),
        "Program › Export: {exported}"
    );

    // File, on the finished row
    select(&mut h, "file.zip");
    press(&mut h, &["File", "Open"]);
    assert!(
        world.fake.opened.lock().unwrap().contains(&done.path),
        "File › Open"
    );
    let app_path = world.dir.path().join("Viewer.app");
    *world.fake.chosen.lock().unwrap() = Some(app_path.clone());
    press(&mut h, &["File", "Open with…"]);
    assert!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.ends_with(&format!("with {}", app_path.display()))),
        "File › Open with…"
    );
    press(&mut h, &["File", "Open folder"]);
    assert!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.starts_with("reveal ")),
        "File › Open folder"
    );
    press(&mut h, &["File", "Properties"]);
    assert!(
        h.state().dialogs.properties.contains_key(&done.id),
        "File › Properties"
    );
    h.state_mut().dialogs.properties.clear();
    assert!(
        greyed(&mut h, &["File", "Show progress"]),
        "a finished download has no progress"
    );
    assert!(
        greyed(&mut h, &["File", "Update download link"]),
        "nor an address to refresh"
    );
    press(&mut h, &["File", "Move/Rename"]);
    assert!(h.state().dialogs.rename.is_some(), "File › Move/Rename");
    h.state_mut().dialogs.rename = None;
    press(&mut h, &["File", "Move to queue"]);
    assert!(
        h.state().dialogs.move_to_queue.is_some(),
        "File › Move to queue"
    );
    h.state_mut().dialogs.move_to_queue = None;

    // File, on the row waiting in the main queue
    select(&mut h, "other.pdf");
    let before = h
        .state()
        .rows()
        .iter()
        .position(|r| dm_desktop::app::name_of(r) == "other.pdf");
    press(&mut h, &["File", "Move up"]);
    let after = h
        .state()
        .rows()
        .iter()
        .position(|r| dm_desktop::app::name_of(r) == "other.pdf");
    assert!(after < before, "File › Move up: {before:?} → {after:?}");
    press(&mut h, &["File", "Move down"]);
    let back = h
        .state()
        .rows()
        .iter()
        .position(|r| dm_desktop::app::name_of(r) == "other.pdf");
    assert_eq!(back, before, "File › Move down");
    press(&mut h, &["File", "Take out of queue"]);
    assert!(
        row(&h, "other.pdf").queue.is_empty(),
        "File › Take out of queue"
    );

    // File, on the running row
    select(&mut h, "stalled.bin");
    let stalled = row(&h, "stalled.bin");
    press(&mut h, &["File", "Show progress"]);
    assert!(
        h.state().dialogs.progress.contains(&stalled.id),
        "File › Show progress"
    );
    h.state_mut().dialogs.progress.clear();
    assert!(
        greyed(&mut h, &["File", "Update download link"]),
        "not while it is downloading"
    );
    press(&mut h, &["File", "Stop download"]);
    until(&mut h, "Stop download", |app| {
        app.rows()
            .iter()
            .any(|r| r.filename == "stalled.bin" && r.state == State::Paused)
    });
    press(&mut h, &["File", "Update download link"]);
    assert!(
        h.state().dialogs.update_link.is_some(),
        "File › Update download link"
    );
    h.state_mut().dialogs.update_link = None;
    press(&mut h, &["File", "Resume download"]);
    until(&mut h, "Resume download", |app| {
        app.rows()
            .iter()
            .any(|r| r.filename == "stalled.bin" && r.state != State::Paused)
    });

    // Downloads
    press(&mut h, &["Downloads", "Stop all"]);
    until(&mut h, "Stop all", |app| {
        app.rows().iter().all(|r| r.state != State::Running)
    });
    press(&mut h, &["Downloads", "Resume all"]);
    until(&mut h, "Resume all", |app| {
        app.rows()
            .iter()
            .any(|r| r.filename == "stalled.bin" && r.state != State::Paused)
    });
    press(&mut h, &["File", "Select all"]);
    let visible: std::collections::BTreeSet<_> = h.state().visible().iter().map(|r| r.id).collect();
    assert_eq!(h.state().selected, visible, "File › Select all");
    press(&mut h, &["Downloads", "Find"]);
    assert!(h.state().dialogs.find_open, "Downloads › Find");
    h.state_mut().dialogs.find_open = false;
    h.state_mut().dialogs.find_text = "file.zip".into();
    h.state_mut().selected.clear();
    press(&mut h, &["Downloads", "Find next"]);
    assert!(
        h.state().selected.contains(&done.id),
        "Downloads › Find next"
    );
    press(&mut h, &["Downloads", "Scheduler"]);
    assert!(
        h.state().dialogs.scheduler.is_some(),
        "Downloads › Scheduler"
    );
    h.state_mut().dialogs.scheduler = None;
    press(
        &mut h,
        &["Downloads", "Speed limiter", "Speed limiter settings"],
    );
    assert!(
        h.state().dialogs.limiter.is_some(),
        "Downloads › Speed limiter settings"
    );
    h.state_mut().dialogs.limiter = None;
    let limit = h.state().backend().settings().speed_limit;
    press(&mut h, &["Downloads", "Speed limiter", "Limit speed"]);
    assert_ne!(
        h.state().backend().settings().speed_limit,
        limit,
        "Limit speed"
    );
    press(&mut h, &["Downloads", "Speed limiter", "Limit speed"]);
    assert_eq!(
        h.state().backend().settings().speed_limit,
        limit,
        "and back"
    );
    press(&mut h, &["Downloads", "Options"]);
    assert!(h.state().dialogs.options.is_some(), "Downloads › Options");
    h.state_mut().dialogs.options = None;
    let mut when_done = vec![
        ("Exit dm", dm_desktop::power::WhenDone::Quit),
        ("Sleep", dm_desktop::power::WhenDone::Sleep),
        ("Turn off computer", dm_desktop::power::WhenDone::ShutDown),
        ("Do nothing", dm_desktop::power::WhenDone::Nothing),
    ];
    when_done.insert(2, ("Hibernate", dm_desktop::power::WhenDone::Hibernate));
    // Greyed where the computer has no such command (macOS has no
    // hibernate; a Linux with no `systemctl` has none of the three).
    when_done.retain(|(_, wanted)| {
        matches!(
            wanted,
            dm_desktop::power::WhenDone::Quit | dm_desktop::power::WhenDone::Nothing
        ) || dm_desktop::power::available(*wanted)
    });
    for (item, wanted) in when_done {
        press(&mut h, &["Downloads", "When done", item]);
        assert_eq!(
            h.state().when_done,
            wanted,
            "Downloads › When done › {item}"
        );
    }
    assert!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .all(|o| !o.starts_with("power")),
        "nothing slept"
    );
    h.state_mut().place = dm_desktop::app::Place::Queue(dm_core::MAIN_QUEUE.into());
    press(&mut h, &["Downloads", "Start queue"]);
    assert!(
        h.state().said.english().starts_with("Started")
            || h.state().said.english().starts_with("Nothing in"),
        "Start queue: {:?}",
        h.state().said
    );
    press(&mut h, &["Downloads", "Stop queue"]);
    assert!(
        h.state().said.english().starts_with("Stopped"),
        "Stop queue: {:?}",
        h.state().said
    );
    h.state_mut().place = dm_desktop::app::Place::All;
    h.run_steps(3);

    // The finished row again: fetched afresh, then the list cleared of it.
    select(&mut h, "file.zip");
    press(&mut h, &["File", "Redownload"]);
    // A fresh copy, beside the first (which stays), followed by its row.
    until(&mut h, "Redownload to finish again", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == done.id && r.state == State::Completed && r.path != done.path)
    });
    let again = h
        .state()
        .rows()
        .iter()
        .find(|r| r.id == done.id)
        .unwrap()
        .clone();
    assert_eq!(std::fs::read(&again.path).unwrap(), payload(), "Redownload");
    assert!(
        std::path::Path::new(&done.path).exists(),
        "the first copy is kept"
    );
    press(&mut h, &["Downloads", "Clear finished"]);
    assert!(
        h.state().rows().iter().all(|r| r.state != State::Completed),
        "Downloads › Clear all finished"
    );

    // Remove waits for a download to stop, then asks first.
    select(&mut h, "stalled.bin");
    assert!(
        greyed(&mut h, &["File", "Remove"]),
        "Remove while it is downloading"
    );
    press(&mut h, &["File", "Stop download"]);
    until(&mut h, "it to stop", |app| {
        app.rows()
            .iter()
            .all(|r| r.filename != "stalled.bin" || r.state == State::Paused)
    });
    press(&mut h, &["File", "Remove"]);
    assert!(
        h.state().dialogs.confirm_remove.is_some(),
        "File › Remove asks first"
    );
    click(&mut h, "Yes, remove");
    until(&mut h, "the row to go", |app| {
        app.rows().iter().all(|r| r.filename != "stalled.bin")
    });

    // Import what Export wrote at the start: its rows come back.
    *world.fake.chosen.lock().unwrap() = Some(list.clone());
    let rows = h.state().rows().len();
    press(&mut h, &["Program", "Import"]);
    assert!(
        h.state().said.english().starts_with("Imported"),
        "Program › Import: {:?}",
        h.state().said
    );
    h.state_mut().refresh();
    assert!(
        h.state().rows().len() > rows,
        "Program › Import added nothing"
    );

    // With nothing downloading, Exit quits; with something, it asks.
    h.state_mut().run(Command::StopAll);
    h.state_mut().refresh();
    press(&mut h, &["Program", "Exit"]);
    assert!(h.state().quit_asked, "Program › Exit");
}

/// Every toolbar button, clicked by its name, does what its menu item does.
#[test]
fn every_toolbar_button_clicked_does_what_it_says() {
    let world = world();
    let mut h = world.harness();
    add_url(&mut h, &world.url("file.zip"), "Start now");
    add_url(&mut h, &world.url("stalled.bin"), "Start now");
    until(&mut h, "file.zip to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    h.run_steps(3);

    let tool = |h: &mut Harness<'_, DmApp>, name: &str| {
        h.query_all_by_label(name)
            .next()
            .unwrap_or_else(|| panic!("no toolbar button {name:?}"))
            .click();
        h.run_steps(3);
    };
    tool(&mut h, "Add URL");
    assert!(h.state().dialogs.add_url.is_some(), "Add URL");
    h.state_mut().dialogs.add_url = None;
    tool(&mut h, "Options");
    assert!(h.state().dialogs.options.is_some(), "Options");
    h.state_mut().dialogs.options = None;
    tool(&mut h, "Scheduler");
    assert!(h.state().dialogs.scheduler.is_some(), "Scheduler");
    h.state_mut().dialogs.scheduler = None;
    tool(&mut h, "Site files");
    assert!(h.state().dialogs.site_files.is_some(), "Site files");
    h.state_mut().dialogs.site_files = None;

    select(&mut h, "stalled.bin");
    tool(&mut h, "Stop");
    until(&mut h, "Stop", |app| {
        app.rows()
            .iter()
            .any(|r| r.filename == "stalled.bin" && r.state == State::Paused)
    });
    tool(&mut h, "Resume");
    until(&mut h, "Resume", |app| {
        app.rows()
            .iter()
            .any(|r| r.filename == "stalled.bin" && r.state != State::Paused)
    });
    tool(&mut h, "Stop all");
    until(&mut h, "Stop all", |app| {
        app.rows().iter().all(|r| r.state != State::Running)
    });

    h.state_mut().place = dm_desktop::app::Place::Queue(dm_core::MAIN_QUEUE.into());
    tool(&mut h, "Start queue");
    assert!(
        h.state().said.english().starts_with("Started")
            || h.state().said.english().starts_with("Nothing in"),
        "Start queue"
    );
    tool(&mut h, "Stop queue");
    assert!(
        h.state().said.english().starts_with("Stopped"),
        "Stop queue"
    );
    h.state_mut().place = dm_desktop::app::Place::All;
    h.run_steps(3);

    tool(&mut h, "Clear finished");
    assert!(!is_there(&h, "file.zip"), "Clear finished");
    select(&mut h, "stalled.bin");
    tool(&mut h, "Remove");
    assert!(
        h.state().dialogs.confirm_remove.is_some(),
        "Delete asks first for an unfinished one"
    );
    click(&mut h, "Yes, remove");
    until(&mut h, "Remove", |app| {
        app.rows().iter().all(|r| r.filename != "stalled.bin")
    });
}

/// Each item of a row's right-click menu, clicked by its name, acts on that
/// row.
#[test]
fn every_row_menu_item_clicked_does_what_it_says() {
    let world = world();
    let mut h = world.harness();
    add_url(&mut h, &world.url("file.zip"), "Start now");
    add_url(&mut h, &world.url("stalled.bin"), "Start now");
    until(&mut h, "file.zip to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    h.run_steps(3);
    let done = row(&h, "file.zip");
    let stalled = row(&h, "stalled.bin");

    // Right-clicks the row, then the item: the last of that name is the
    // one in the menu.
    let right = |h: &mut Harness<'_, DmApp>, name: &str, item: &str| {
        h.run_steps(2);
        h.get_by_label(name).click_secondary();
        h.run_steps(3);
        h.query_all_by_label(item)
            .last()
            .unwrap_or_else(|| panic!("{item:?} is not in {name}'s menu"))
            .click();
        h.run_steps(3);
        // Only a menu still open: Escape with none would close the dialog
        // the item opened.
        if h.ctx.any_popup_open() {
            h.key_press(egui::Key::Escape);
            h.run_steps(2);
        }
    };

    right(&mut h, "file.zip", "Open");
    assert!(
        world.fake.opened.lock().unwrap().contains(&done.path),
        "Open"
    );
    right(&mut h, "file.zip", "Open folder");
    assert!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.starts_with("reveal ")),
        "Open folder"
    );
    right(&mut h, "file.zip", "Move/Rename");
    assert!(h.state().dialogs.rename.is_some(), "Move/Rename");
    h.state_mut().dialogs.rename = None;
    right(&mut h, "file.zip", "Properties");
    assert!(
        h.state().dialogs.properties.contains_key(&done.id),
        "Properties"
    );
    h.state_mut().dialogs.properties.clear();
    right(&mut h, "file.zip", "Move to queue");
    assert!(h.state().dialogs.move_to_queue.is_some(), "Move to queue");
    h.state_mut().dialogs.move_to_queue = None;

    right(&mut h, "stalled.bin", "Stop download");
    until(&mut h, "Stop download", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == stalled.id && r.state == State::Paused)
    });
    right(&mut h, "stalled.bin", "Update download link");
    assert!(
        h.state().dialogs.update_link.is_some(),
        "Update download link"
    );
    h.state_mut().dialogs.update_link = None;
    right(&mut h, "stalled.bin", "Resume download");
    until(&mut h, "Resume download", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == stalled.id && r.state != State::Paused)
    });
    right(&mut h, "stalled.bin", "Stop download");
    until(&mut h, "it to stop", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == stalled.id && r.state == State::Paused)
    });
    right(&mut h, "stalled.bin", "Remove");
    assert_eq!(
        h.state().dialogs.confirm_remove,
        Some(vec![stalled.id]),
        "Remove asks first"
    );
    click(&mut h, "Yes, remove");
    until(&mut h, "Remove", |app| {
        app.rows().iter().all(|r| r.id != stalled.id)
    });

    right(&mut h, "file.zip", "Redownload");
    until(&mut h, "Redownload", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == done.id && r.state == State::Completed && r.path != done.path)
    });
}

#[test]
fn command_shift_n_opens_the_numbered_series_not_add_url() {
    let world = world();
    let mut harness = world.harness();
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::N,
    );
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.series.is_some(),
        "no numbered series"
    );
    assert!(
        harness.state().dialogs.add_url.is_none(),
        "Add URL opened too"
    );
    // And ⌘N alone is still Add URL.
    harness.key_press(egui::Key::Escape);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    harness.run_steps(3);
    assert!(harness.state().dialogs.add_url.is_some());
}

#[test]
fn every_column_is_reached_at_the_default_window_size() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(960.0, 660.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    let folder = harness.get_by_role_and_label(Role::Button, "Folder");
    assert!(
        folder.rect().max.x > 960.0,
        "the premise: Folder starts past the edge"
    );
    folder.scroll_to_me();
    harness.run_steps(5);
    let at = harness.get_by_role_and_label(Role::Button, "Folder").rect();
    assert!(
        at.min.x >= 0.0 && at.max.x <= 960.0,
        "the Folder column cannot be reached: {at:?}"
    );
}

/// At 150% and 200% text in the smallest window, a dialog
/// keeps its title on screen and scrolls what does not fit; Files on a
/// site ran off the top and bottom, and every dialog was at least 440
/// points wide.
#[test]
fn every_dialog_keeps_its_title_and_buttons_on_screen_at_large_text() {
    use egui::accesskit::Role;
    for (zoom, size) in [
        (1.5, egui::vec2(720.0, 460.0)),
        (2.0, egui::vec2(960.0, 660.0)),
    ] {
        for (command, title, button) in [
            (Command::SiteFiles, "Site files", "Find files"),
            (Command::AddUrl, "Add URL", "OK"),
            (Command::AddSeries, "Add numbered series", "Cancel"),
            (Command::Scheduler, "Scheduler", "Cancel"),
            (Command::SpeedLimiterSettings, "Speed limiter", "Cancel"),
            (Command::About, "About dm", "Check for updates"),
            (Command::Options, "Options", "OK"),
        ] {
            let world = world();
            let mut harness = Harness::builder()
                .with_size(size)
                .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
            harness.ctx.set_zoom_factor(zoom);
            harness.run_steps(3);
            harness.state_mut().run(command);
            harness.run_steps(8);
            let points = size / zoom;
            let inside = |at: egui::Rect| {
                at.left() >= -0.5
                    && at.right() <= points.x + 0.5
                    && at.top() >= -0.5
                    && at.bottom() <= points.y + 0.5
            };
            // The dialog's own title, drawn after a toolbar button or
            // menu item of the same name.
            let heading = harness
                .query_all_by_label(title)
                .map(|n| n.rect())
                .rfind(|at| at.height() > 0.0)
                .unwrap_or_else(|| panic!("{zoom}x: {title} did not open"));
            assert!(
                inside(heading),
                "{zoom}x {size:?}: {title}'s title at {heading:?}"
            );
            // Below the menu bar, which dialogs opened over.
            let bar = harness
                .query_all_by_label("Program")
                .map(|n| n.rect())
                .find(|at| at.top() < 40.0)
                .expect("the menu bar");
            assert!(
                heading.top() >= bar.bottom(),
                "{zoom}x {size:?}: {title} at {heading:?} is over the menu bar at {bar:?}"
            );
            // Its buttons on screen, or brought there by scrolling its body
            // with the wheel; never past the window's edge out of reach.
            // The title is the window's own name now, its node the whole
            // dialog: the wheel goes in under its top.
            harness.hover_at(heading.left_top() + egui::vec2(40.0, 96.0));
            for _ in 0..10 {
                harness.event(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -200.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                });
                harness.run_steps(1);
            }
            harness.run_steps(4);
            let pressed: Vec<egui::Rect> = harness
                .query_all_by_role_and_label(Role::Button, button)
                .map(|n| n.rect())
                .collect();
            assert!(!pressed.is_empty(), "{zoom}x: {title} has no {button}");
            assert!(
                pressed.iter().any(|at| inside(*at)),
                "{zoom}x {size:?}: {title}'s {button} cannot be brought onto the window"
            );
        }
    }
}
