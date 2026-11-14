// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The commands, as the menus, the toolbar, a row's menu, the keys and the control port reach
//! them: each does what it says. Moved from the egui window's tests, but for two of egui's
//! own: that every control had a name in egui's AccessKit tree (the GPUI window's names are
//! `a11y.rs`'s and the accessibility checks', gaps row U23), and that each dialog kept its
//! title and buttons inside the main window at 150% and 200% text (a GPUI dialog is a window of
//! its own, sized to what it holds, and the text size is one for the whole process, so a test
//! of it would scale every test's window beside it).

use dm_core::State;
use dm_shell::app::commands::Command;

use crate::harness::world::{payload, world};
use crate::harness::Harness;

fn finished(h: &mut Harness) -> bool {
    h.rows().iter().any(|r| r.state == State::Completed)
}

/// The row called `name` is in `state`.
fn is(h: &mut Harness, name: &str, state: State) -> bool {
    h.rows()
        .iter()
        .any(|r| dm_shell::app::names::name_of(r) == name && r.state == state)
}

/// Walks the menus by their words, as a person clicks a menu's title and then its items.
fn press(h: &mut Harness, path: &[&str]) {
    for words in path {
        h.click(words);
    }
}

/// Whether the command is greyed in the menus and the toolbar.
fn greyed(h: &mut Harness, command: Command) -> bool {
    let listed = h.call("commands.list", serde_json::Value::Null).unwrap();
    listed
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == command.id())
        .is_some_and(|c| c["enabled"] == false)
}

/// The column the list is sorted by, and whether up.
fn sort(h: &mut Harness) -> Option<(String, bool)> {
    let seen = h.call("ui.inspect", serde_json::Value::Null).unwrap();
    let sort = &seen["sort"];
    sort["column"]
        .as_str()
        .map(|c| (c.to_string(), sort["ascending"].as_bool().unwrap_or(false)))
}

/// What a person's choice of `command` in the menu bar does: the menu's action in the main
/// window. Open, Open with… and Preview are a person's alone, so a script's click on them is
/// refused (`probe::is_run_a_file`); a person chooses them a moment after the script last
/// acted.
fn persons_choice(h: &mut Harness, command: Command) {
    h.until_a_persons_moment();
    h.act(|view, cx| view.run(command, cx));
}

/// The window walked in each language: the empty list, the menus, every dialog and every
/// Options tab. Whatever it asks to translate and a catalog lacks is reported, which a scan of
/// the source (`i18n_check.rs`) cannot see for words chosen at run time (a tab's name, a choice
/// in a list).
#[test]
fn every_dialog_in_every_language_is_translated() {
    let mut untranslated = std::collections::BTreeSet::new();
    for (code, _) in dm_desktop_base::i18n::LANGUAGES
        .iter()
        .filter(|(c, _)| *c != "en")
    {
        let world = world();
        let mut settings = world.settings();
        settings.language = (*code).into();
        let mut h = world.harness_with(settings);
        dm_desktop_base::i18n::set(code);
        dm_desktop_base::i18n::record_missing();
        for menu in crate::menus::WINDOW_MENUS {
            h.click(dm_desktop_base::i18n::t(menu));
            h.texts();
            h.key("escape", &[]);
        }
        h.view(|view| {
            view.backend.add_with(
                &world.url("stalled.bin"),
                dm_core::AddOptions {
                    later: true,
                    queue: dm_core::MAIN_QUEUE.into(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
        h.until("the row", |h| !h.rows().is_empty());
        let id = h.rows()[0].id;
        let name = dm_shell::app::names::name_of(&h.rows()[0]);
        h.select_by_port(&name);
        // Download details, the progress window, Download complete, Delete's question and
        // Properties, each looked at and closed.
        let url = world.url("x.bin");
        h.act(|_, cx| crate::details::open(cx.entity().downgrade(), vec![url], cx));
        h.until("Download details", |h| h.dialogs().len() == 1);
        h.close_dialogs();
        h.act(|view, cx| view.show_progress(id, cx));
        h.until("the progress window", |h| h.dialogs().len() == 1);
        h.close_dialogs();
        h.act(|view, cx| view.finished(&[id], cx));
        h.texts();
        h.close_dialogs();
        for command in [
            Command::Remove,
            Command::Properties,
            Command::AddUrl,
            Command::AddSeries,
            Command::AddFromClipboard,
            Command::SiteFiles,
            Command::Scheduler,
            Command::SpeedLimiterSettings,
            Command::MoveToQueue,
            Command::MoveRename,
            Command::UpdateLink,
            Command::About,
        ] {
            h.act(|view, cx| view.run(command, cx));
            h.texts();
            h.close_dialogs();
        }
        h.act(|view, cx| view.run(Command::Options, cx));
        h.until("Options", |h| h.dialogs().len() == 1);
        for tab in [
            "General",
            "File types",
            "Folders",
            "Downloads",
            "Connection",
            "Sites",
            "Sounds",
            "Advanced",
        ] {
            let tab = dm_desktop_base::i18n::t(tab);
            if h.shows(tab) {
                h.click(tab);
                h.texts();
            }
        }
        h.close_dialogs();
        for missing in dm_desktop_base::i18n::take_missing() {
            untranslated.insert(format!("{code}: {missing:?}"));
        }
        dm_desktop_base::i18n::set("en");
    }
    assert!(untranslated.is_empty(), "untranslated: {untranslated:#?}");
}

/// Every command in the registry runs without falling over, with every row selected in each
/// state: the menus, toolbar, keyboard and control port all end here. Check for updates is
/// left out: it asks GitHub, and a test makes no request off this computer.
#[test]
fn every_command_runs_against_rows_in_every_state() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Add for later");
    h.until("one to finish", finished);
    *world.fake.clipboard.lock().unwrap() = Some(world.url("clip.bin"));
    for command in Command::ALL {
        if matches!(
            command,
            Command::Quit
                | Command::Remove
                | Command::Import
                | Command::Export
                | Command::CheckForUpdates
        ) {
            continue;
        }
        h.act(|view, cx| {
            view.select_all(cx);
            view.run(*command, cx);
        });
        h.close_dialogs();
    }
    // With something downloading, Exit asks first.
    h.act(|view, cx| view.run(Command::DownloadNow, cx));
    h.until("a download to run", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    h.act(|view, cx| view.run(Command::Quit, cx));
    h.until("Exit to ask", |h| h.shows_part("still running"));
}

/// Every menu item, clicked by its name, does what it says: a dialog opens, a row changes
/// state, a file opens, a setting changes. "Check for updates" is found and enabled but not
/// clicked: it asks GitHub, and a test makes no request off this computer.
#[test]
fn every_menu_item_clicked_does_what_it_says() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.add_url(&world.url("other.pdf"), "Add for later");
    h.until("file.zip to finish", finished);
    let done = h.row("file.zip");

    // View
    press(&mut h, &["View", "Categories"]);
    assert!(
        !h.view(|v| v.show_sidebar),
        "View › Categories did not hide the side panel"
    );
    press(&mut h, &["View", "Categories"]);
    assert!(h.view(|v| v.show_sidebar));
    press(&mut h, &["View", "Toolbar"]);
    assert!(
        !h.view(|v| v.show_toolbar),
        "View › Toolbar did not hide it"
    );
    press(&mut h, &["View", "Toolbar"]);
    assert!(h.view(|v| v.show_toolbar));
    for (item, column) in [
        ("Sort by name", Command::SortByName),
        ("Sort by size", Command::SortBySize),
        ("Sort by status", Command::SortByStatus),
        ("Sort by time left", Command::SortByTimeLeft),
        ("Sort by speed", Command::SortByRate),
        ("Sort by date", Command::SortByDate),
        ("Sort by note", Command::SortByDescription),
    ] {
        h.view(|v| v.sort = None);
        press(&mut h, &["View", "Sort by", item]);
        let sorted = sort(&mut h);
        assert!(
            sorted.as_ref().is_some_and(|(_, up)| *up),
            "{item} ({column:?}): {sorted:?}"
        );
    }

    // Help
    press(&mut h, &["Help", "Help"]);
    h.until("the help page", |h| {
        h.fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o == dm_desktop_base::project::HELP)
    });
    press(&mut h, &["Help", "About dm"]);
    h.until("About dm", |h| h.dialog_open("About dm"));
    h.close_dialogs();
    assert!(
        !greyed(&mut h, Command::CheckForUpdates),
        "Check for updates is greyed out"
    );

    // Tasks
    press(&mut h, &["Tasks", "Add URL"]);
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    h.close_dialogs();
    press(&mut h, &["Tasks", "Add numbered series"]);
    h.until("Add numbered series", |h| {
        h.dialog_open("Add numbered series")
    });
    h.close_dialogs();
    h.cx.write_to_clipboard(gpui::ClipboardItem::new_string(world.url("clip.bin")));
    press(&mut h, &["Tasks", "Add links from clipboard"]);
    h.until("Links on the clipboard", |h| {
        h.dialog_open("Links on the clipboard")
    });
    h.close_dialogs();
    press(&mut h, &["Tasks", "Grabber"]);
    h.until("the Grabber", |h| h.dialog_open("Grabber"));
    h.close_dialogs();
    let list = world.dir.path().join("list.dmlist");
    h.choose_ahead(&list);
    press(&mut h, &["Tasks", "Export"]);
    h.until("the list to be written", |_| list.is_file());
    let exported = std::fs::read_to_string(&list).unwrap();
    assert!(
        ["file.zip", "stalled.bin", "other.pdf"]
            .iter()
            .all(|n| exported.contains(n)),
        "Tasks › Export: {exported}"
    );

    // File, on the finished row
    h.select_by_port("file.zip");
    persons_choice(&mut h, Command::OpenFile);
    h.until("File › Open", |h| {
        h.fake.opened.lock().unwrap().contains(&done.path)
    });
    press(&mut h, &["File", "Open folder"]);
    h.until("File › Open folder", |h| {
        h.fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.starts_with("reveal "))
    });
    press(&mut h, &["File", "Properties"]);
    h.until("File › Properties", |h| h.dialog_open("File properties"));
    h.close_dialogs();
    assert!(
        greyed(&mut h, Command::ShowProgress),
        "a finished download has no progress"
    );
    assert!(
        greyed(&mut h, Command::UpdateLink),
        "nor an address to refresh"
    );
    press(&mut h, &["File", "Move/Rename"]);
    h.until("File › Move/Rename", |h| h.dialog_open("Move/Rename"));
    h.close_dialogs();
    press(&mut h, &["File", "Move to queue"]);
    h.until("File › Move to queue", |h| h.dialog_open("Move to queue"));
    h.close_dialogs();

    // File, on the row waiting in the main queue
    h.select_by_port("other.pdf");
    let place = |h: &mut Harness| {
        h.rows()
            .iter()
            .position(|r| dm_shell::app::names::name_of(r) == "other.pdf")
    };
    let before = place(&mut h);
    press(&mut h, &["File", "Move up"]);
    h.until("File › Move up", |h| place(h) < before);
    press(&mut h, &["File", "Move down"]);
    h.until("File › Move down", |h| place(h) == before);
    press(&mut h, &["File", "Take out of queue"]);
    h.until("File › Take out of queue", |h| {
        h.row("other.pdf").queue.is_empty()
    });

    // File, on the running row
    h.select_by_port("stalled.bin");
    press(&mut h, &["File", "Show progress"]);
    h.until("File › Show progress", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
    h.close_dialogs();
    // Waited for, not read once: under the whole suite's load the running row's state can
    // reach the command list a moment after its progress window opens.
    h.until("Update download link greyed while downloading", |h| {
        greyed(h, Command::UpdateLink)
    });
    press(&mut h, &["File", "Stop download"]);
    h.until("Stop download", |h| is(h, "stalled.bin", State::Paused));
    press(&mut h, &["File", "Update download link"]);
    h.until("File › Update download link", |h| {
        h.dialog_open("Update download link")
    });
    h.close_dialogs();
    press(&mut h, &["File", "Resume"]);
    h.until("Resume", |h| !is(h, "stalled.bin", State::Paused));

    // Downloads
    press(&mut h, &["Downloads", "Stop All"]);
    h.until("Stop All", |h| {
        h.rows().iter().all(|r| r.state != State::Running)
    });
    press(&mut h, &["Downloads", "Resume all"]);
    h.until("Resume all", |h| !is(h, "stalled.bin", State::Paused));
    press(&mut h, &["File", "Select all"]);
    let shown: std::collections::BTreeSet<_> =
        h.view(|v| v.shown.iter().map(|&i| v.rows[i].id).collect());
    assert_eq!(h.view(|v| v.selected.clone()), shown, "File › Select all");
    // Find puts the keys in the top bar's search box; Find next steps to the next row whose
    // name holds its words.
    press(&mut h, &["Downloads", "Find"]);
    h.type_text("file.zip");
    h.view(|v| v.selected.clear());
    press(&mut h, &["Downloads", "Find next"]);
    h.until("Downloads › Find next", |h| {
        h.view(|v| v.selected.contains(&done.id))
    });
    // The box emptied by its ✕, and every row shown again.
    h.click("✕");
    h.until("the search box emptied", |h| h.view(|v| v.shown.len()) == 3);
    press(&mut h, &["Downloads", "Scheduler"]);
    h.until("Downloads › Scheduler", |h| h.dialog_open("Scheduler"));
    h.close_dialogs();
    press(
        &mut h,
        &["Downloads", "Speed limiter", "Speed limiter settings"],
    );
    h.until("Downloads › Speed limiter settings", |h| {
        h.dialog_open("Speed limiter")
    });
    h.close_dialogs();
    let limit = h.view(|v| v.backend.settings().speed_limit);
    press(&mut h, &["Downloads", "Speed limiter", "Limit speed"]);
    assert_ne!(
        h.view(|v| v.backend.settings().speed_limit),
        limit,
        "Limit speed"
    );
    press(&mut h, &["Downloads", "Speed limiter", "Limit speed"]);
    assert_eq!(
        h.view(|v| v.backend.settings().speed_limit),
        limit,
        "and back"
    );
    press(&mut h, &["Downloads", "Options"]);
    h.until("Downloads › Options", |h| h.dialog_open("Options"));
    h.close_dialogs();
    use dm_shell::app::power::WhenDone;
    let when_done = [
        ("Exit dm", WhenDone::Quit),
        ("Sleep", WhenDone::Sleep),
        ("Hibernate", WhenDone::Hibernate),
        ("Turn off computer", WhenDone::ShutDown),
        ("Do nothing", WhenDone::Nothing),
    ];
    // Greyed where the computer has no such command (macOS has no hibernate; a Linux with no
    // `systemctl` has none of the three).
    for (item, wanted) in when_done
        .into_iter()
        .filter(|(_, w)| w.possible(dm_desktop_base::power::available))
    {
        press(&mut h, &["Downloads", "When done", item]);
        assert_eq!(
            h.view(|v| v.session.when_done),
            wanted,
            "Downloads › When done › {item}"
        );
    }
    assert!(
        h.fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .all(|o| !o.starts_with("power")),
        "nothing slept"
    );
    // Each a list of the queues, as reference A's menu has.
    press(&mut h, &["Downloads", "Start Queue", "Main queue"]);
    let said = h.said();
    assert!(
        said.starts_with("Started") || said.starts_with("Nothing in"),
        "Start Queue: {said:?}"
    );
    press(&mut h, &["Downloads", "Stop Queue", "Main queue"]);
    let said = h.said();
    assert!(said.starts_with("Stopped"), "Stop Queue: {said:?}");

    // The finished row again: fetched afresh, then the list cleared of it.
    h.select_by_port("file.zip");
    press(&mut h, &["File", "Redownload"]);
    // A fresh copy, beside the first (which stays), followed by its row.
    h.until("Redownload to finish again", |h| {
        h.rows()
            .iter()
            .any(|r| r.id == done.id && r.state == State::Completed && r.path != done.path)
    });
    let again = h.rows().into_iter().find(|r| r.id == done.id).unwrap();
    assert_eq!(std::fs::read(&again.path).unwrap(), payload(), "Redownload");
    assert!(
        std::path::Path::new(&done.path).exists(),
        "the first copy is kept"
    );
    press(&mut h, &["Downloads", "Delete Completed"]);
    h.until("Downloads › Delete Completed", |h| {
        h.rows().iter().all(|r| r.state != State::Completed)
    });

    // Delete waits for a download to stop, then asks first.
    h.select_by_port("stalled.bin");
    if !is(&mut h, "stalled.bin", State::Running) {
        press(&mut h, &["File", "Resume"]);
        h.until("it to run", |h| is(h, "stalled.bin", State::Running));
    }
    assert!(
        greyed(&mut h, Command::Remove),
        "Delete while it is downloading"
    );
    press(&mut h, &["File", "Stop download"]);
    h.until("it to stop", |h| is(h, "stalled.bin", State::Paused));
    press(&mut h, &["File", "Delete"]);
    h.until_shown("Yes, remove");
    h.click("Yes, remove");
    h.until("the row to go", |h| {
        h.rows().iter().all(|r| r.filename != "stalled.bin")
    });

    // Import what Export wrote at the start: its rows come back.
    let rows = h.rows().len();
    h.choose_ahead(&list);
    press(&mut h, &["Tasks", "Import"]);
    // A row of the list still there (under load a download the list kept may not have
    // finished or gone yet) is asked about: kept once, the others come back.
    h.until("Tasks › Import", |h| {
        if h.shows("Skip them") {
            h.click("Skip them");
        }
        h.said().starts_with("Imported")
    });
    h.until("the rows imported", |h| h.rows().len() > rows);

    // With nothing downloading, Exit quits; with something, it asks.
    h.act(|view, cx| view.run(Command::StopAll, cx));
    h.until("all stopped", |h| {
        h.rows().iter().all(|r| r.state != State::Running)
    });
    press(&mut h, &["Tasks", "Exit"]);
    h.until("Tasks › Exit to quit", |_| crate::question::is_quitting());
}

/// Every toolbar button, clicked by its name, does what its menu item does.
#[test]
fn every_toolbar_button_clicked_does_what_it_says() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("file.zip to finish", finished);

    // The toolbar's button, not a menu item of the same name: no menu is open.
    h.click("Add URL");
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    h.close_dialogs();
    h.click("Options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.close_dialogs();
    h.click("Scheduler");
    h.until("Scheduler", |h| h.dialog_open("Scheduler"));
    h.close_dialogs();
    h.click("Grabber");
    h.until("Grabber", |h| h.dialog_open("Grabber"));
    h.close_dialogs();

    h.select_by_port("stalled.bin");
    h.click("Stop");
    h.until("Stop", |h| is(h, "stalled.bin", State::Paused));
    h.click("Resume");
    h.until("Resume", |h| !is(h, "stalled.bin", State::Paused));
    h.click("Stop All");
    h.until("Stop All", |h| {
        h.rows().iter().all(|r| r.state != State::Running)
    });

    h.call(
        "ui.place",
        serde_json::json!({ "place": "queue:Main queue" }),
    )
    .unwrap();
    h.click("Start Queue");
    let said = h.said();
    assert!(
        said.starts_with("Started") || said.starts_with("Nothing in"),
        "Start Queue: {said:?}"
    );
    h.click("Stop Queue");
    let said = h.said();
    assert!(said.starts_with("Stopped"), "Stop Queue: {said:?}");
    h.call("ui.place", serde_json::json!({ "place": "all" }))
        .unwrap();

    h.click("Delete Completed");
    h.until("Delete Completed", |h| {
        h.rows().iter().all(|r| r.state != State::Completed)
    });
    h.select_by_port("stalled.bin");
    h.click("Delete");
    h.until_shown("Yes, remove");
    h.click("Yes, remove");
    h.until("Delete", |h| {
        h.rows().iter().all(|r| r.filename != "stalled.bin")
    });
}

/// Each item of a row's right-click menu, clicked by its name, acts on that row.
#[test]
fn every_row_menu_item_clicked_does_what_it_says() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("file.zip to finish", finished);
    let done = h.row("file.zip");
    let stalled = h.row("stalled.bin");

    // Right-clicks the row, then the item: the menu is in front, so its item is the first of
    // that name on screen.
    let right = |h: &mut Harness, name: &str, item: &str| {
        h.right_click(name);
        h.until_shown(item);
        h.click(item);
        // A menu still open closed; Escape with none would close the dialog the item opened.
        if h.view(|v| v.menu_open()) {
            h.key("escape", &[]);
        }
    };

    // Open runs the file: a person's choice in the menu, a moment after the script last acted.
    h.right_click("file.zip");
    h.until_shown("Open folder");
    h.until_a_persons_moment();
    h.person_click("Open", 1);
    h.until("Open", |h| {
        h.fake.opened.lock().unwrap().contains(&done.path)
    });
    right(&mut h, "file.zip", "Open folder");
    h.until("Open folder", |h| {
        h.fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.starts_with("reveal "))
    });
    right(&mut h, "file.zip", "Move/Rename");
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    h.close_dialogs();
    right(&mut h, "file.zip", "Properties");
    h.until("Properties", |h| h.dialog_open("File properties"));
    h.close_dialogs();
    right(&mut h, "file.zip", "Move to queue");
    h.until("Move to queue", |h| h.dialog_open("Move to queue"));
    h.close_dialogs();

    right(&mut h, "stalled.bin", "Stop download");
    h.until("Stop download", |h| {
        h.rows()
            .iter()
            .any(|r| r.id == stalled.id && r.state == State::Paused)
    });
    right(&mut h, "stalled.bin", "Update download link");
    h.until("Update download link", |h| {
        h.dialog_open("Update download link")
    });
    h.close_dialogs();
    right(&mut h, "stalled.bin", "Resume");
    h.until("Resume", |h| {
        h.rows()
            .iter()
            .any(|r| r.id == stalled.id && r.state != State::Paused)
    });
    right(&mut h, "stalled.bin", "Stop download");
    h.until("it to stop", |h| {
        h.rows()
            .iter()
            .any(|r| r.id == stalled.id && r.state == State::Paused)
    });
    right(&mut h, "stalled.bin", "Delete");
    h.until_shown("Yes, remove");
    h.click("Yes, remove");
    h.until("Delete", |h| h.rows().iter().all(|r| r.id != stalled.id));

    right(&mut h, "file.zip", "Redownload");
    h.until("Redownload", |h| {
        h.rows()
            .iter()
            .any(|r| r.id == done.id && r.state == State::Completed && r.path != done.path)
    });
}

#[test]
fn command_shift_n_opens_the_numbered_series_not_add_url() {
    let world = world();
    let mut h = world.harness();
    h.key("n", &["cmd", "shift"]);
    h.until("the numbered series", |h| {
        h.dialog_open("Add numbered series")
    });
    assert!(!h.dialog_open("Add URL"), "Add URL opened too");
    // And ⌘N alone is still Add URL.
    h.close_dialogs();
    h.key("n", &["cmd"]);
    h.until("Add URL", |h| h.dialog_open("Add URL"));
}

/// Start Queue and Stop Queue each list the queues, as reference A's toolbar arrows and menus
/// do: the queue chosen there is started or stopped, whatever the list shows; the button
/// itself keeps to the list's queue. The toolbar's arrows open the system's own menu, which a
/// script cannot reach, so the lists are walked in the Downloads menu, whose items are the same
/// (`menus::RunOnQueue`, B64).
#[test]
fn the_queue_arrows_start_and_stop_the_queue_chosen() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let id = h.row("file.zip").id;
    h.view(|v| v.backend.set_queue(id, "Night"));
    h.until("the row to be in the Night queue", |h| {
        h.rows().iter().any(|r| r.queue == "Night")
    });
    // The list shows every download, so the button would act on the Main queue; the choice in
    // the list of queues is Night.
    h.call("ui.place", serde_json::json!({ "place": "all" }))
        .unwrap();
    // Each line waited for, not read at once: a slow runner says it a moment after the click
    // (Windows', 2026-11-14, read it empty).
    press(&mut h, &["Downloads", "Start Queue", "Night"]);
    h.until("the list to start the queue chosen", |h| {
        h.said() == "Started 1 download in Night."
    });
    h.until("the Night queue's download to complete", |h| {
        h.rows().iter().all(|r| r.state == State::Completed)
    });
    // Stop Queue's list on Night: nothing is left there to stop.
    press(&mut h, &["Downloads", "Stop Queue", "Night"]);
    h.until("Stop Queue on Night", |h| {
        h.said() == "Stopped 0 downloads in Night."
    });
    // The choice was for that one command: the button itself acts on the list's queue again.
    h.click("Start Queue");
    h.until("the button on the Main queue", |h| {
        h.said() == "Nothing in Main queue was waiting to start."
    });
}

/// At the window's smallest usual size (960 by 660) the columns run past its right edge, and
/// the list scrolls sideways to reach the last of them, Folder.
#[test]
fn every_column_is_reached_at_the_default_window_size() {
    let world = world();
    let mut h = world.harness();
    let main = h.window;
    h.cx.update(|cx| {
        main.update(cx, |_, window, _| {
            window.resize(gpui::size(gpui::px(960.0), gpui::px(660.0)))
        })
    })
    .unwrap();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let folder = |h: &mut Harness| {
        let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
        texts["texts"].as_array().unwrap().iter().find_map(|t| {
            (t["text"] == "Folder" && t["window"] == "main").then(|| {
                let r = t["rect"].as_array().unwrap();
                (r[0].as_f64().unwrap(), r[2].as_f64().unwrap())
            })
        })
    };
    let width = 960.0;
    let first = folder(&mut h);
    assert!(
        first.is_none_or(|(_, right)| right > width - 1.0),
        "the premise: Folder starts past the edge, at {first:?}"
    );
    // A wheel's sideways turn over the headings, as far as the list goes.
    let over = serde_json::json!({ "text": "File Name", "sideways": 40.0 });
    if let Err(why) = h.call("ui.scroll", over) {
        panic!("the list did not scroll: {why}");
    }
    h.until("Folder in the window", |h| {
        folder(h).is_some_and(|(left, right)| left >= 0.0 && right <= width)
    });
}
