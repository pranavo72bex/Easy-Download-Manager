// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! More of what the window does around its list: the bars above it, the
//! status line's chips, the commands, the menus' queue and place items,
//! imports and the floating drop target while hidden, driven as the window
//! tests drive them (`tests/window/support.rs`), against a real queue on a
//! loopback server, with nothing of the system's touched.

#[path = "window/support.rs"]
#[allow(dead_code, unused_imports)]
mod support;

use egui::accesskit::Role;
use support::*;

/// One test at a time: they share one home and its settings file.
static ONE: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// A system that says no to opening anything and has no app to pick.
#[derive(Default)]
struct Refusing {
    asked: Mutex<Vec<String>>,
}

impl Platform for Refusing {
    fn set_login_item(&self, _wanted: bool) -> Result<(), String> {
        Err("login items are not allowed here".into())
    }
    fn notify(&self, _title: &str, _body: &str) {}
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, path: &str) -> Result<(), String> {
        Err(format!("cannot show {path}"))
    }
    fn open(&self, path: &str) -> Result<(), String> {
        self.asked.lock().unwrap().push(path.to_string());
        Err(format!("cannot open {path}"))
    }
    fn chime(&self) {}
    fn power(
        &self,
        action: dm_desktop::power::WhenDone,
        _report: dm_desktop::power::Report,
    ) -> Result<(), String> {
        Err(format!("{action:?} is not allowed here"))
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        Some("/no/such/app".into())
    }
    fn open_with(&self, path: &str, _app: &std::path::Path) -> Result<(), String> {
        Err(format!("{path} cannot be opened with that"))
    }
    fn choose_file(&self, _save: bool, _suggested: &str) -> Option<std::path::PathBuf> {
        None
    }
}

/// A window over `settings` whose system is `platform`.
fn app_on(world: &World, settings: Settings, platform: Arc<dyn Platform>) -> DmApp {
    let queue = Queue::new(
        dm_core::client(),
        settings.options(
            world.dir.path().join("parts"),
            dm_core::Limiter::unlimited(),
        ),
        settings.concurrency,
    );
    let backend = Backend::new(
        world.runtime.handle().clone(),
        queue,
        settings,
        Vec::new(),
        platform,
    );
    DmApp::new(backend)
}

fn harness_of(app: DmApp) -> Harness<'static, DmApp> {
    Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app)
}

/// What the status line says.
fn said(harness: &Harness<'_, DmApp>) -> String {
    harness.state().said.shown()
}

/// A right-click at `at`, as a person's.
fn right_click(harness: &mut Harness<'_, DmApp>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.run_steps(2);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.run_steps(2);
}

/* ----------------------------------------------------------- searches */

/// What a search found, kept in the site's layout: each file in folders
/// under the download folder that mirror its address, and an address dm
/// cannot fetch neither stops the rest nor is counted.
#[test]
fn what_a_search_found_keeps_the_sites_layout() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    let later = dm_core::AddOptions {
        later: true,
        queue: dm_core::MAIN_QUEUE.into(),
        ..Default::default()
    };
    let deep = world.url("docs/2024/deep.pdf");
    let top = world.url("file.zip");
    let added = app
        .add_found(&[deep.clone(), top.clone()], later.clone(), true)
        .unwrap();
    assert_eq!(added, 2);
    app.refresh();
    let site = format!("127.0.0.1_{}", world.addr.port());
    let root = world.dir.path().join("out");
    let saved = |url: &str| {
        app.rows()
            .iter()
            .find(|r| r.url == url)
            .and_then(|r| r.save_dir.clone())
            .unwrap_or_else(|| panic!("{url} has no folder of its own"))
    };
    assert_eq!(
        std::path::PathBuf::from(saved(&deep)),
        root.join(&site).join("docs").join("2024")
    );
    assert_eq!(std::path::PathBuf::from(saved(&top)), root.join(&site));

    // Only what cannot be fetched: refused, with why.
    let refused = app.add_found(&["not an address".to_string()], later.clone(), true);
    assert!(refused.is_err(), "{refused:?}");
    // Beside one that can: the one that can goes in.
    let mixed = app
        .add_found(
            &["not an address".to_string(), world.url("other.pdf")],
            later,
            true,
        )
        .unwrap();
    assert_eq!(mixed, 1);
}

/* ----------------------------------------------------------- the bars */

/// The browser hint's buttons each open their own store's page.
#[test]
fn the_browser_hint_opens_each_browsers_page() {
    let _one = one_at_a_time();
    let world = world();
    let home = dm_shell::settings::home();
    let _ = std::fs::remove_file(home.join("browser-hint.hidden"));
    let mut harness = world.harness();
    harness.run_steps(3);
    for browser in ["Firefox", "Chrome and others", "Microsoft Edge"] {
        assert!(
            harness.query_by_label(browser).is_some(),
            "no {browser} in the browser hint"
        );
        let before = world.fake.opened.lock().unwrap().len();
        click(&mut harness, browser);
        let opened = world.fake.opened.lock().unwrap().clone();
        assert_eq!(opened.len(), before + 1, "{browser} opened nothing");
        assert!(
            opened.last().unwrap().starts_with("https://"),
            "{browser}: {opened:?}"
        );
    }
}

/// A download paused for want of its folder: Resume tries it again (and it
/// pauses again, the folder still missing), and another folder that cannot
/// be made either is said.
#[test]
fn a_stalled_download_resumes_and_a_folder_that_cannot_be_made_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let blocker = world.dir.path().join("not-a-drive");
    std::fs::write(&blocker, b"a file where the folder should be").unwrap();
    let mut harness = world.harness_with(Settings {
        download_dir: blocker.join("out"),
        ..world.settings()
    });
    harness
        .state()
        .backend()
        .add(&world.url("file.zip"))
        .unwrap();
    let stalled = |app: &DmApp| {
        app.rows()
            .iter()
            .any(|r| r.state == State::Paused && !r.stalled.is_empty())
    };
    until(&mut harness, "the download to pause by itself", stalled);
    harness.run_steps(3);
    *world.fake.chosen.lock().unwrap() = Some(blocker.join("still").join("not"));
    click(&mut harness, "Choose another folder…");
    assert!(
        said(&harness).contains("could not be made"),
        "{}",
        said(&harness)
    );
    // The bar's own Resume, not the toolbar's (nothing is selected).
    harness
        .query_all_by_label("Resume")
        .find(|n| !n.accesskit_node().is_disabled())
        .expect("no Resume on the bar")
        .click();
    harness.run_steps(2);
    until(
        &mut harness,
        "the download to pause by itself again",
        stalled,
    );
}

/// Several links on the clipboard: Download… lists them to pick from, and
/// Dismiss puts them away; one link's Download opens Download details.
#[test]
fn the_clipboards_bar_offers_its_links() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    let (a, b) = (world.url("file.zip"), world.url("other.pdf"));
    let offer = |harness: &mut Harness<'_, DmApp>, all: Vec<String>| {
        let app = harness.state_mut();
        app.clipboard.offered = Some(all[0].clone());
        app.clipboard.offered_all = all;
        harness.run_steps(3);
    };
    offer(&mut harness, vec![a.clone(), b.clone()]);
    assert!(harness
        .query_by_label("Links on your clipboard: 2")
        .is_some());
    click(&mut harness, "Dismiss");
    assert!(harness.state().clipboard.offered.is_none());
    assert!(harness.state().clipboard.dismissed.contains(&b));

    *world.fake.clipboard.lock().unwrap() = Some(format!("{a}\n{b}"));
    offer(&mut harness, vec![a.clone(), b.clone()]);
    click(&mut harness, "Download…");
    harness.run_steps(3);
    assert!(harness.state().clipboard.offered.is_none());
    assert!(
        harness.state().dialogs.clip_links.is_some(),
        "the links were not listed to pick from"
    );
    harness.state_mut().dialogs.clip_links = None;
    harness.run_steps(2);

    offer(&mut harness, vec![a.clone()]);
    assert!(harness.query_by_label("Link on your clipboard:").is_some());
    click(&mut harness, "Download");
    assert!(harness.state().clipboard.offered.is_none());
    assert!(harness.state().clipboard.dismissed.contains(&a));
    assert!(
        harness.state().dialogs.file_info.is_some(),
        "Download details did not open for the link"
    );
}

/// A problem said above the list stays until it is dismissed.
#[test]
fn a_problem_above_the_list_is_dismissed() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .problem("the disk said no to something".into());
    harness.run_steps(3);
    assert!(harness
        .query_by_label_contains("The disk said no to something")
        .is_some());
    let dismiss = harness
        .query_all_by_label("Dismiss")
        .last()
        .expect("no Dismiss beside the problem");
    dismiss.click();
    harness.run_steps(3);
    assert!(harness.state().problems.is_empty());
}

/// The filter's field, in the top row where the window is wide and over
/// the list where it is not: Escape empties it.
#[test]
fn escape_in_the_filter_empties_it() {
    let _one = one_at_a_time();
    let world = world();
    for width in [1100.0, 560.0] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, 720.0))
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        add_named(&mut harness, &world, "alpha.zip");
        add_named(&mut harness, &world, "beta.zip");
        harness.state_mut().refresh();
        harness.run_steps(3);
        harness
            .query_all_by_role(Role::TextInput)
            .find(|n| n.accesskit_node().label().as_deref() == Some("Filter"))
            .unwrap_or_else(|| panic!("no Filter field {width} wide"))
            .click();
        harness.run_steps(2);
        harness.event(egui::Event::Text("alpha".into()));
        harness.run_steps(3);
        assert_eq!(harness.state().filter, "alpha", "{width} wide");
        harness.key_press(egui::Key::Escape);
        harness.run_steps(3);
        assert!(
            harness.state().filter.is_empty(),
            "Escape left the filter {width} wide"
        );
    }
}

/* ----------------------------------------------------------- commands */

/// Selects only the row of that name, as a click on it does.
fn only(app: &mut DmApp, name: &str) -> dm_core::Id {
    let id = app
        .rows()
        .iter()
        .find(|r| dm_desktop::app::name_of(r) == name)
        .unwrap_or_else(|| panic!("no row {name}"))
        .id;
    app.selected = [id].into();
    id
}

/// Commands on a row added for later: Update download link opens on it,
/// Show progress opens its window; the clipboard with no link in it is said.
#[test]
fn commands_on_a_row_for_later_and_an_empty_clipboard() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    add_named(&mut harness, &world, "later.zip");
    let app = harness.state_mut();
    app.refresh();
    let id = only(app, "later.zip");

    app.run(Command::UpdateLink);
    assert_eq!(app.dialogs.update_link.as_ref().map(|u| u.0), Some(id));
    app.dialogs.update_link = None;

    app.run(Command::ShowProgress);
    assert!(app.dialogs.progress.contains(&id));
    app.dialogs.progress.clear();

    *world.fake.clipboard.lock().unwrap() = Some("only words, no address".into());
    app.run(Command::AddFromClipboard);
    assert_eq!(app.said.shown(), "There are no links on the clipboard.");
    assert!(app.dialogs.clip_links.is_none());
}

/// A finished row: Show progress does not open a window for it.
#[test]
fn a_finished_row_has_no_progress_window() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("file.zip"))
        .unwrap();
    until(&mut harness, "the download to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let app = harness.state_mut();
    let id = only(app, "file.zip");
    app.run(Command::ShowProgress);
    assert!(!app.dialogs.progress.contains(&id));
}

/// Copy details of a failed row puts its block on the clipboard and says
/// how many; When done is set by its command.
#[test]
fn copy_details_of_a_failed_row_and_when_done() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("secret.zip"))
        .unwrap();
    until(&mut harness, "the download to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let app = harness.state_mut();
    only(app, "secret.zip");
    app.run(Command::CopyDetails);
    let copied = app.copied.clone().expect("nothing was copied");
    assert!(copied.contains("State: failed"), "{copied}");
    assert_eq!(app.said.shown(), "Copied the details of 1 download.");

    app.run(Command::WhenDoneQuit);
    assert_eq!(app.session.when_done, dm_desktop::power::WhenDone::Quit);
    app.run(Command::WhenDoneNothing);
    assert_eq!(app.session.when_done, dm_desktop::power::WhenDone::Nothing);
}

/// Start queue while the schedule is shut says the queue waits for it;
/// Stop queue says how many it stopped.
#[test]
fn start_queue_under_a_shut_schedule_and_stop_queue() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    add_named(&mut harness, &world, "held.zip");
    let app = harness.state_mut();
    app.refresh();
    // A schedule on no day there is: shut, and never opening.
    app.applied.schedule = dm_core::Schedule {
        enabled: true,
        from: "01:00".into(),
        to: "02:00".into(),
        days: vec![9],
        ..dm_core::Schedule::default()
    };
    app.run(Command::StartQueue);
    assert!(
        app.said
            .shown()
            .ends_with("is waiting for the schedule; it starts by itself when the schedule opens."),
        "{}",
        app.said.shown()
    );
    app.run(Command::StopQueue);
    assert!(
        app.said.shown().starts_with("Stopped 1 download in "),
        "{}",
        app.said.shown()
    );
}

/* ----------------------------------------------------------- menus */

/// Downloads › Start Queue lists the queues; the one clicked is started.
#[test]
fn start_queue_lists_the_queues_and_starts_the_one_clicked() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    add_named(&mut harness, &world, "waiting.zip");
    harness.state_mut().refresh();
    harness.run_steps(2);
    click(&mut harness, "Downloads");
    harness
        .query_all_by_label_contains("Start Queue")
        .rfind(|n| n.accesskit_node().role() != Role::Label)
        .expect("no Start Queue in Downloads")
        .click();
    harness.run_steps(3);
    harness
        .query_all_by_label("Main queue")
        .last()
        .expect("Start Queue does not list the main queue")
        .click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().queue_chosen,
        None,
        "the choice was not taken"
    );
    assert!(
        said(&harness).starts_with("Started 1 download in "),
        "{}",
        said(&harness)
    );
}

/// Help's pages are opened by the system; one it refuses to open is said.
#[test]
fn a_help_page_the_system_cannot_open_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let refusing = Arc::new(Refusing::default());
    let mut harness = harness_of(app_on(&world, world.settings(), refusing.clone()));
    harness.run_steps(2);
    click(&mut harness, "Help");
    click(&mut harness, "Home page");
    harness.run_steps(2);
    let asked = refusing.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        said(&harness).starts_with("cannot open "),
        "{}",
        said(&harness)
    );
}

/// The row menu's "Delete file after" sets the time for the rows chosen.
#[test]
fn the_row_menus_delete_file_after_sets_the_time() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    add_named(&mut harness, &world, "kept.zip");
    harness.state_mut().refresh();
    harness.run_steps(3);
    harness.get_by_label("kept.zip").click_secondary();
    harness.run_steps(3);
    harness
        .query_all_by_label_contains("Delete file after")
        .rfind(|n| n.accesskit_node().role() != Role::Label)
        .expect("no Delete file after in the row menu")
        .click();
    harness.run_steps(3);
    harness
        .query_all_by_label("1 hour")
        .last()
        .expect("no 1 hour")
        .click();
    harness.run_steps(3);
    assert_eq!(row(&harness, "kept.zip").delete_after, Some(3600));
}

/* ----------------------------------------------------------- places */

/// In a narrow window the places are one drop-down, a category of the person's
/// own among them; a place's Open folder opens its folder.
#[test]
fn the_place_picker_has_own_categories_and_open_folder_opens_them() {
    let _one = one_at_a_time();
    let world = world();
    let mut settings = world.settings();
    settings.custom_categories = vec![
        dm_core::Custom {
            name: "Books".into(),
            extensions: vec!["epub".into()],
            sites: Vec::new(),
        },
        dm_core::Custom {
            name: "  ".into(),
            extensions: vec!["xyz".into()],
            sites: Vec::new(),
        },
    ];
    // Too narrow for the places beside the list.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(600.0, 900.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app_with(settings));
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::ComboBox, "Place")
        .click();
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Books (0)").is_some(),
        "the person's own category is not a place"
    );
    // A blank name is no place.
    assert!(harness.query_by_label(" (0)").is_none());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);

    let root = world.dir.path().join("out");
    let app = harness.state_mut();
    app.tree_action(
        Place::Custom("Books".into()),
        dm_desktop::app::TreeAction::OpenFolder,
    );
    app.tree_action(
        Place::Kind(dm_core::Category::Videos),
        dm_desktop::app::TreeAction::OpenFolder,
    );
    app.tree_action(Place::All, dm_desktop::app::TreeAction::OpenFolder);
    let opened = world.fake.opened.lock().unwrap().clone();
    let tail: Vec<&String> = opened.iter().rev().take(3).collect();
    assert_eq!(*tail[2], root.join("Books").display().to_string());
    assert!(std::path::Path::new(tail[1]).starts_with(&root), "{tail:?}");
    assert_eq!(*tail[0], root.display().to_string());
}

/* ----------------------------------------------------------- imports */

/// A list file that cannot be read is said; a `.torrent` is a download,
/// not a list.
#[test]
fn an_unreadable_list_is_said_and_a_torrent_is_a_download() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    let gone = world.dir.path().join("no-such-list.txt");
    let err = app.import_from(&gone).unwrap_err();
    assert!(err.contains("could not be read"), "{err}");
    assert_eq!(app.said.shown(), err);

    let torrent = world.dir.path().join("not-really.torrent");
    std::fs::write(&torrent, b"this is not bencode").unwrap();
    match app.import_from(&torrent) {
        // Refused as the download it is, and said.
        Err(e) => assert_eq!(app.said.shown(), e),
        // Or taken as one, to be read when it starts.
        Ok(()) => {
            app.refresh();
            assert!(
                app.rows()
                    .iter()
                    .any(|r| r.url.ends_with("not-really.torrent")),
                "the torrent was not added"
            );
        }
    }
}

/* ----------------------------------------------------------- keys */

/// Enter on the list opens the selected rows' progress windows.
#[test]
fn enter_on_the_list_shows_the_selected_rows_progress() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    add_named(&mut harness, &world, "enter.zip");
    harness.state_mut().refresh();
    harness.run_steps(2);
    select(&mut harness, "enter.zip");
    let id = row(&harness, "enter.zip").id;
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.progress.contains(&id),
        "Enter did not open its progress"
    );
}

/// A text field's right-click menu copies what was selected, goes when
/// clicked away from, and goes with its dialog.
#[test]
fn a_fields_menu_copies_and_goes_away() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    *world.fake.clipboard.lock().unwrap() = None;
    harness.state_mut().run(Command::AddUrl);
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .type_text("http://example.invalid/x.zip");
    harness.run_steps(2);
    let field = harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .rect();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(2);
    right_click(&mut harness, field.center());
    assert!(harness.query_by_label("Copy").is_some(), "no menu");
    click(&mut harness, "Copy");
    harness.run_steps(2);
    assert!(harness.query_by_label("Cut").is_none(), "the menu stayed");
    assert_eq!(
        harness
            .state()
            .dialogs
            .add_url
            .as_ref()
            .map(|d| d.address.clone())
            .as_deref(),
        Some("http://example.invalid/x.zip"),
        "Copy changed the text"
    );

    // Clicked away from: gone.
    right_click(&mut harness, field.center());
    assert!(harness.query_by_label("Paste").is_some());
    let away = egui::pos2(field.center().x, field.max.y + 120.0);
    harness.hover_at(away);
    harness.event(egui::Event::PointerButton {
        pos: away,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(1);
    harness.event(egui::Event::PointerButton {
        pos: away,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(2);
    assert!(
        harness.query_by_label("Paste").is_none(),
        "a click away left it"
    );

    // Its dialog closed under it: gone too.
    if harness.state().dialogs.add_url.is_some() {
        right_click(&mut harness, field.center());
        assert!(harness.query_by_label("Paste").is_some());
        harness.state_mut().dialogs.add_url = None;
        harness.run_steps(3);
        assert!(
            harness.query_by_label("Paste").is_none(),
            "the menu outlived its dialog"
        );
    }
}

/// Hidden in the tray, the floating drop target is still drawn, and only
/// when it is on.
#[test]
fn hidden_the_drop_target_is_drawn_only_when_on() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    app.applied.drop_target = false;
    // The window drawn once first, as before it was hidden: it sets up the
    // fonts the card draws with.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, app: &mut DmApp| {
                if app.hidden {
                    app.while_hidden(ui.ctx());
                } else {
                    app.ui(ui);
                }
            },
            app,
        );
    harness.run_steps(2);
    harness.state_mut().hidden = true;
    harness.run_steps(2);
    assert!(harness
        .query_by_label_contains(dm_desktop::drops::DROP_TARGET_TITLE)
        .is_none());
    harness.state_mut().applied.drop_target = true;
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains(dm_desktop::drops::DROP_TARGET_TITLE)
            .is_some(),
        "no drop target while hidden"
    );
}

/* ----------------------------------------------------------- status line */

/// The speed limiter's chip in the status line turns the limiter on at the
/// last limit, and off again.
#[test]
fn the_speed_chip_turns_the_limiter_on_and_off() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(2);
    assert_eq!(harness.state().applied.speed_limit, 0);
    harness
        .query_all_by_label("· Speed limiter")
        .last()
        .expect("no speed chip in the status line")
        .click();
    harness.run_steps(3);
    // Never set: the 512 KB/s a new install goes back to.
    assert_eq!(harness.state().applied.speed_limit, 512);
    assert_eq!(said(&harness), "Speed limiter on: 512 KB/s.");
    harness
        .query_all_by_label("· limited to 512 KB/s")
        .last()
        .expect("the chip does not say the limit")
        .click();
    harness.run_steps(3);
    assert_eq!(harness.state().applied.speed_limit, 0);
}

/// A schedule that never opens, with a download waiting on it: the status
/// line says it waits for the schedule, with no time to give.
#[test]
fn a_schedule_that_never_opens_is_waited_for_with_no_time() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let mut wanted = harness.state().applied.clone();
    wanted.schedule = dm_core::Schedule {
        enabled: true,
        from: "01:00".into(),
        to: "02:00".into(),
        days: vec![9],
        ..dm_core::Schedule::default()
    };
    let applied = harness.state_mut().apply_settings(wanted);
    if let Err(e) = applied {
        panic!("the schedule was refused: {e}");
    }
    harness
        .state_mut()
        .add(&[world.url("file.zip")], dm_core::AddOptions::default())
        .unwrap();
    until(
        &mut harness,
        "the download to wait for the schedule",
        |app| {
            app.rows()
                .iter()
                .any(|r| r.state == State::Paused && r.paused_by_schedule)
        },
    );
    harness.state_mut().said = Default::default();
    harness.run_steps(3);
    let found = harness
        .query_by_label("· waiting for the schedule")
        .is_some();
    let mut wanted = harness.state().applied.clone();
    wanted.schedule = dm_core::Schedule::default();
    harness.state_mut().apply_settings(wanted).unwrap();
    assert!(found, "the wait for the schedule was not said");
}

/* ----------------------------------------------------------- the list */

/// Presses on the row of that name and drags towards the row `onto`,
/// leaving the button held.
fn drag_row(harness: &mut Harness<'_, DmApp>, from: &str, onto: &str) -> egui::Pos2 {
    let rect = |h: &Harness<'_, DmApp>, name: &str| h.get_by_label(name).rect();
    let from = rect(harness, from).center();
    let onto = rect(harness, onto).center();
    harness.event(egui::Event::PointerMoved(from));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for step in 1..=10 {
        let at = from + (onto - from) * (step as f32 / 10.0);
        harness.event(egui::Event::PointerMoved(at));
        harness.step();
    }
    onto
}

fn order(h: &Harness<'_, DmApp>) -> Vec<String> {
    h.state()
        .rows()
        .iter()
        .map(|r| dm_desktop::app::name_of(r))
        .collect()
}

/// A row dragged while the list is sorted by a column is not moved, and
/// what to do is said; unsorted, Escape lets go of a dragged row where it
/// was.
#[test]
fn a_drag_on_a_sorted_list_says_why_and_escape_lets_go() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    for name in ["a.zip", "b.zip", "c.zip"] {
        add_named(&mut harness, &world, name);
    }
    harness.state_mut().refresh();
    harness.run_steps(3);
    let before = order(&harness);

    harness.state_mut().sort = Some((Column::Name, true));
    harness.run_steps(2);
    let onto = drag_row(&mut harness, "c.zip", "a.zip");
    assert!(harness.state().reordering.is_none());
    assert!(
        said(&harness).starts_with("The list is sorted by a column"),
        "{}",
        said(&harness)
    );
    harness.event(egui::Event::PointerButton {
        pos: onto,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.run_steps(3);
    assert_eq!(order(&harness), before);

    harness.state_mut().sort = None;
    harness.run_steps(2);
    drag_row(&mut harness, "c.zip", "a.zip");
    assert!(
        harness.state().reordering.is_some(),
        "the drag took nothing"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().reordering.is_none(), "Escape kept the row");
    assert_eq!(order(&harness), before, "Escape moved the row");
}

/// An empty list offers its first step as a button.
#[test]
fn an_empty_list_offers_to_add_a_download() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(2);
    assert!(harness.state().rows().is_empty());
    click(&mut harness, "Add a download");
    harness.run_steps(2);
    assert!(
        harness.state().dialogs.add_url.is_some(),
        "Add a download did not open Add URL"
    );
}

/// In the classic look a submenu is the plain kind, and it still opens and
/// takes its items: Downloads › When done › Exit dm.
#[test]
fn a_classic_submenu_opens_and_takes_its_item() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness_with(Settings {
        theme: "classic".into(),
        ..world.settings()
    });
    harness.run_steps(3);
    press(&mut harness, &["Downloads", "When done", "Exit dm"]);
    assert_eq!(
        harness.state().session.when_done,
        dm_desktop::power::WhenDone::Quit
    );
    harness.state_mut().run(Command::WhenDoneNothing);
    // A row selected in the classic look is drawn in its selection's
    // colours, and stays selected.
    add_named(&mut harness, &world, "classic.zip");
    harness.state_mut().refresh();
    harness.run_steps(2);
    select(&mut harness, "classic.zip");
    harness.run_steps(2);
    assert_eq!(harness.state().selected.len(), 1);
}

/// An Undo whose time is over is put away by the status line, not offered.
#[test]
fn an_undo_whose_time_is_over_is_put_away() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    let long_ago = Instant::now()
        .checked_sub(Duration::from_secs(600))
        .expect("a time ten minutes ago");
    harness.state_mut().undo_remove = dm_shell::app::backend::Undo::since(long_ago, Vec::new());
    harness.state_mut().said = dm_desktop::i18n::Said::new("Removed 1 download.", &[]);
    harness.run_steps(2);
    assert!(!harness.state().undo_remove.is_kept());
    assert!(harness.query_by_label("Undo").is_none());
}

/* ----------------------------------------------------------- refusals */

/// A window whose settings in force differ from the saved ones in a way the
/// system refuses to make (start at login): every later change to them is
/// refused, as on a computer that does not allow login items.
fn refusing_window(world: &World, settings: Settings) -> (Harness<'static, DmApp>, Arc<Refusing>) {
    let refusing = Arc::new(Refusing::default());
    let mut app = app_on(world, settings, refusing.clone());
    app.applied.start_on_login = true;
    (harness_of(app), refusing)
}

/// A saved search that is due runs even where when it ran cannot be saved,
/// which is said as a problem; what it found goes in, kept in the site's
/// layout, and what it saw is kept.
#[test]
fn a_due_search_runs_where_when_it_ran_cannot_be_saved() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.site_searches = vec![dm_shell::lists::Search {
        name: "site".into(),
        page: world.url("page.html"),
        every_hours: 1,
        keep_layout: true,
        ..Default::default()
    }];
    let (mut harness, _) = refusing_window(&world, settings);
    until(&mut harness, "the search's files to be added", |app| {
        app.rows().iter().any(|r| r.url.ends_with("/other.pdf"))
    });
    harness.state_mut().refresh();
    let problems = harness.state().problems.clone();
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("when the saved searches last ran could not be saved: ")),
        "{problems:?}"
    );
    // The refused change is put back with the refusal, so what the search
    // saw is kept by the next save.
    until(&mut harness, "what the search saw to be kept", |app| {
        app.applied
            .site_searches
            .iter()
            .any(|s| s.name == "site" && s.seen.iter().any(|f| f.ends_with("/other.pdf")))
    });
    let site = format!("127.0.0.1_{}", world.addr.port());
    let pdf = row(&harness, "other.pdf");
    assert_eq!(
        pdf.save_dir.map(std::path::PathBuf::from),
        Some(world.dir.path().join("out").join(site))
    );
}

/// A sort that cannot be saved is said, and the list is still sorted.
#[test]
fn a_sort_that_cannot_be_saved_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let (mut harness, _) = refusing_window(&world, world.settings());
    harness.state_mut().sort_by(Column::Size);
    assert!(
        said(&harness).contains("start-on-login"),
        "{}",
        said(&harness)
    );
    assert_eq!(harness.state().sort, Some((Column::Size, true)));
}

/// Open with… where the app chosen cannot open the file: said.
#[test]
fn open_with_an_app_that_cannot_open_it_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let (mut harness, _) = refusing_window(&world, world.settings());
    harness.state_mut().open_with_chosen("/some/file.zip");
    assert_eq!(said(&harness), "/some/file.zip cannot be opened with that");
}

/// A row the last read of the list has not seen yet is filtered by its own
/// words.
#[test]
fn a_row_not_yet_read_is_filtered_by_its_own_words() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    add_named(&mut harness, &world, "seen.zip");
    harness.state_mut().refresh();
    let mut unseen = row(&harness, "seen.zip");
    unseen.id += 1000;
    unseen.filename = "unseen-report.pdf".into();
    unseen.save_name = Some("unseen-report.pdf".into());
    let app = harness.state_mut();
    app.filter = "report".into();
    assert!(app.shows(&unseen));
    app.filter = "nothing-like-it".into();
    assert!(!app.shows(&unseen));
}

/// A row's hover buttons: on a row among several selected, Resume acts on
/// all of them and keeps the selection.
#[test]
fn a_rows_hover_buttons_act_on_the_selection_it_is_in() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    for name in ["one.zip", "two.zip"] {
        add_named(&mut harness, &world, name);
    }
    harness.state_mut().refresh();
    harness.run_steps(3);
    let (one, two) = (row(&harness, "one.zip").id, row(&harness, "two.zip").id);
    harness.state_mut().selected = [one, two].into();
    harness.run_steps(2);

    let at = harness.get_by_label("one.zip").rect().center();
    harness.hover_at(at);
    harness.run_steps(3);
    harness
        .query_all_by_label("Resume one.zip")
        .last()
        .expect("no Resume on the hovered row")
        .click();
    harness.run_steps(3);
    assert_eq!(harness.state().selected, [one, two].into());
    until(&mut harness, "both selected rows to be resumed", |app| {
        [one, two]
            .iter()
            .all(|id| app.row(*id).is_some_and(|r| r.state != State::Paused))
    });
}
