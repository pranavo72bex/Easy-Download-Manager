// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the window does between frames — reading the list, the hand-offs,
//! the schedule's end, drops, the backend's verbs — driven as the window
//! tests drive it (`tests/window/support.rs`), against a real queue on a
//! loopback server, with nothing of the system's touched. A platform that
//! refuses everything stands in where a test needs the system to say no.

#[path = "window/support.rs"]
#[allow(dead_code, unused_imports)]
mod support;

use support::*;

use dm_desktop::power::WhenDone;

/// One test at a time: they share one home, whose inbox every read of the
/// list empties, and the settings file.
static ONE: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// A system that says no: no login item, no power, no opening, no trash
/// (its file is left where it is). The rest of [`Platform`] is the trait's
/// own, which is what a platform that does not override it gets.
#[derive(Default)]
struct Refusing {
    said: Mutex<Vec<String>>,
}

impl Platform for Refusing {
    fn set_login_item(&self, _wanted: bool) -> Result<(), String> {
        Err("login items are not allowed here".into())
    }
    fn notify(&self, _title: &str, body: &str) {
        self.said.lock().unwrap().push(body.to_string());
    }
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, path: &str) -> Result<(), String> {
        Err(format!("cannot show {path}"))
    }
    fn open(&self, path: &str) -> Result<(), String> {
        Err(format!("cannot open {path}"))
    }
    fn chime(&self) {
        self.said.lock().unwrap().push("chime".into());
    }
    fn power(&self, action: WhenDone, _report: dm_desktop::power::Report) -> Result<(), String> {
        Err(format!("{action:?} is not allowed here"))
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn open_with(&self, _path: &str, _app: &std::path::Path) -> Result<(), String> {
        Err("no".into())
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

/// Frames, each after a read of the list, as the window runs them.
fn frames(harness: &mut Harness<'_, DmApp>, n: usize) {
    for _ in 0..n {
        harness.state_mut().refresh();
        harness.step();
    }
}

fn id_of(harness: &Harness<'_, DmApp>, name: &str) -> dm_core::Id {
    row(harness, name).id
}

fn inbox() -> std::path::PathBuf {
    let dir = dm_shell::host::inbox(&dm_shell::settings::home());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/* ----------------------------------------------------------- drops */

#[test]
fn a_dropped_file_of_links_is_downloaded_and_one_without_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    let links = world.dir.path().join("links.txt");
    std::fs::write(&links, format!("{}\n", world.url("file.zip"))).unwrap();
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(dm_desktop::drops::DroppedPath(links.clone())));
    harness.step();
    until(&mut harness, "the dropped link to finish", |app| {
        app.rows()
            .iter()
            .any(|r| dm_desktop::app::name_of(r) == "file.zip" && r.state == State::Completed)
    });

    // A picture holds no link: said, and nothing added.
    let picture = world.dir.path().join("photo.png");
    std::fs::write(&picture, b"png").unwrap();
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(dm_desktop::drops::DroppedPath(picture)));
    harness.step();
    assert_eq!(harness.state().said.english(), dm_desktop::app::drop_hint());
    assert_eq!(harness.state().rows().len(), 1);

    // A file dm's own drop target hands over is read as a file.
    use egui::DroppedFile as _;
    let dropped = dm_desktop::drops::DroppedPath(links.clone());
    assert_eq!(dropped.path(), links.as_path());
    assert_eq!(dropped.bytes().unwrap(), std::fs::read(&links).unwrap());
    let gone = dm_desktop::drops::DroppedPath(world.dir.path().join("gone.txt"));
    assert!(gone.bytes().is_err());
    // Nothing of the window system's to replace on this one.
    dm_desktop::drops::attach(raw_window_handle::RawWindowHandle::AppKit(
        raw_window_handle::AppKitWindowHandle::new(std::ptr::NonNull::dangling()),
    ));
    dm_desktop::drops::attach_floating();
}

/// The floating drop target as its own window (not drawn inside the main
/// one): placed in the main window's corner, filled while a link is held
/// over it, showing what downloads and how far, taking what is dropped on
/// it, and its menu.
#[test]
fn the_floating_drop_target_shows_progress_takes_drops_and_has_its_menu() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.ctx.set_embed_viewports(false);
    harness
        .input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .outer_rect = Some(egui::Rect::from_min_size(
        egui::pos2(0.0, 0.0),
        egui::vec2(1100.0, 720.0),
    ));
    harness.state_mut().run(Command::ToggleDropTarget);
    harness.run_steps(2);
    assert!(harness.state().applied.drop_target);

    // Something downloading: the card's bar and its words on hover.
    harness
        .state_mut()
        .add(&[world.url("stalled.bin")], dm_core::AddOptions::default())
        .unwrap();
    // Running, and its size known: until the server has said it, the card
    // rightly has no "— 0%" to show (CI's macOS runner and a loaded machine
    // looked before it had).
    until(
        &mut harness,
        "the download to run with its size known",
        |app| {
            app.rows()
                .iter()
                .any(|r| r.state == State::Running && r.progress.total.is_some())
        },
    );
    harness.run_steps(3);
    // The card is the whole of its window, which has no title bar.
    let window = bounds(&harness, dm_desktop::drops::DROP_TARGET_TITLE);
    let card = egui::pos2(
        ((window.x0 + window.x1) / 2.0) as f32,
        ((window.y0 + window.y1) / 2.0) as f32,
    );
    harness.hover_at(card);
    harness.run_steps(40);
    // With how far, which only the card says.
    assert!(
        harness
            .query_all_by_label_contains("1 downloading at")
            .any(|n| n
                .accesskit_node()
                .label()
                .is_some_and(|l| l.ends_with("— 0%"))),
        "the card did not say what is downloading"
    );

    // A link held over it, then dropped: taken.
    harness
        .input_mut()
        .hovered_files
        .push(egui::HoveredFile::default());
    harness.run_steps(2);
    harness.input_mut().hovered_files.clear();
    let links = world.dir.path().join("more.txt");
    std::fs::write(&links, format!("{}\n", world.url("other.pdf"))).unwrap();
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(dm_desktop::drops::DroppedPath(links)));
    harness.step();
    until(&mut harness, "the dropped link to be added", |app| {
        app.rows()
            .iter()
            .any(|r| dm_desktop::app::name_of(r) == "other.pdf")
    });
    // A drop with no link in it is said.
    let picture = world.dir.path().join("picture.png");
    std::fs::write(&picture, b"png").unwrap();
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(dm_desktop::drops::DroppedPath(picture)));
    harness.step();
    assert_eq!(harness.state().said.english(), dm_desktop::app::drop_hint());

    // Dragged: moved by the window system.
    harness.drag_at(card);
    harness.hover_at(card + egui::vec2(15.0, 15.0));
    harness.run_steps(2);
    harness.drop_at(card + egui::vec2(15.0, 15.0));
    harness.run_steps(2);

    // Its menu: Show dm, and Hide the drop target.
    harness.state_mut().show_asked = false;
    right_click(&mut harness, card);
    click(&mut harness, "Show dm");
    assert!(harness.state().show_asked);
    right_click(&mut harness, card);
    click(&mut harness, "Hide the drop target");
    harness.run_steps(3);
    assert!(!harness.state().applied.drop_target, "not put away");
    harness.state_mut().run(Command::StopAll);
}

/// A right-click where `at` is, as a person's: the pointer there first.
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

/// With no window to place it by, the drop target goes in the screen's
/// corner.
#[test]
fn the_drop_target_goes_in_the_screens_corner_with_the_window_minimised() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    {
        let root = harness
            .input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default();
        root.minimized = Some(true);
        root.outer_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        ));
        root.monitor_size = Some(egui::vec2(1920.0, 1080.0));
    }
    harness.state_mut().run(Command::ToggleDropTarget);
    harness.run_steps(3);
    assert!(harness
        .query_by_label_contains(dm_desktop::drops::DROP_TARGET_TITLE)
        .is_some());
    harness.state_mut().run(Command::ToggleDropTarget);
    harness.run_steps(2);
    assert!(!harness.state().applied.drop_target);
}

/* ------------------------------------------------------ reading the list */

#[test]
fn a_site_left_with_the_browser_is_not_offered_again_and_another_is_after_two_cancels() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    app.applied.catch.skip_hosts = vec!["kept.example".into()];
    app.hand_off_cancelled("kept.example");
    app.hand_off_cancelled("kept.example");
    assert_eq!(app.dialogs.skip_offer, None, "a site already left there");
    app.hand_off_cancelled("files.example");
    assert_eq!(app.dialogs.skip_offer, None, "once is not twice");
    app.hand_off_cancelled("files.example");
    assert_eq!(app.dialogs.skip_offer.as_deref(), Some("files.example"));
}

/// The command run on each finished file, where it cannot start, is a
/// problem; the sounds a person chose are played (here, as the platform
/// plays what it cannot: its own chime) for a finish, a failure and the
/// queue running dry.
#[test]
fn a_finished_files_command_that_cannot_start_is_a_problem_and_chosen_sounds_play() {
    let _one = one_at_a_time();
    let world = world();
    let mut settings = world.settings();
    settings.on_complete = world
        .dir
        .path()
        .join("no-such-program")
        .display()
        .to_string();
    settings.sound_on_complete = true;
    settings.sound_on_failure = true;
    for event in ["complete", "failed", "queue_done"] {
        settings
            .sounds
            .insert(event.into(), format!(" /sounds/{event}.wav "));
    }
    let mut harness = harness_of(world.app_with(settings));
    harness
        .state_mut()
        .add(&[world.url("file.zip")], dm_core::AddOptions::default())
        .unwrap();
    until(&mut harness, "the download to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    frames(&mut harness, 3);
    assert!(
        harness
            .state()
            .problems
            .iter()
            .any(|p| p.contains("no-such-program") && p.contains("could not start")),
        "{:?}",
        harness.state().problems
    );
    let chimes = || {
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter(|o| *o == "chime")
            .count()
    };
    assert!(chimes() >= 2, "the finish and the dry queue's sounds");
    let before = chimes();
    harness
        .state_mut()
        .add(&[world.url("secret.zip")], dm_core::AddOptions::default())
        .unwrap();
    until(&mut harness, "the download to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    frames(&mut harness, 3);
    assert!(chimes() > before, "no sound for the failure");

    // The finished file deleted since: its row keeps the size it had.
    let done = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.state == State::Completed)
        .map(|r| dm_core::Entry::clone(r))
        .unwrap();
    std::fs::remove_file(&done.path).unwrap();
    frames(&mut harness, 2);
    let after = harness.state().row(done.id).unwrap().clone();
    assert_eq!(after.progress.downloaded, done.progress.downloaded);
    assert_eq!(dm_desktop::app::status_in_english(&after), "File missing");
}

#[test]
fn with_no_window_exit_when_done_quits_at_once() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    app.headless = true;
    app.session.when_done = WhenDone::Quit;
    app.add(&[world.url("file.zip")], dm_core::AddOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.quit_asked {
        assert!(Instant::now() < deadline, "never quit");
        std::thread::sleep(Duration::from_millis(25));
        app.refresh();
    }
    assert!(
        app.session.countdown.is_none(),
        "counted down with nobody to see it"
    );
}

/// What the browser hands over with no window to ask in: a page's links go
/// into the main queue for later, each with its words and name; an
/// address that is no download is said.
#[test]
fn with_no_window_a_pages_links_wait_in_the_main_queue_and_a_bad_address_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    app.headless = true;
    let (one, two) = (world.url("file.zip"), world.url("other.pdf"));
    let mut texts = std::collections::BTreeMap::new();
    texts.insert(one.clone(), "the first".to_string());
    let mut names = std::collections::BTreeMap::new();
    names.insert(two.clone(), "named.pdf".to_string());
    app.backend
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            links: vec![one.clone(), two.clone()],
            referrer: world.url("page.html"),
            texts,
            names,
            ..Default::default()
        });
    app.take_handed();
    let rows = app.backend.list();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row.state, State::Paused, "started unasked");
        assert_eq!(row.queue, dm_core::MAIN_QUEUE);
        assert_eq!(row.referrer, world.url("page.html"));
    }
    let first = rows.iter().find(|r| r.url == one).unwrap();
    assert_eq!(first.description, "the first");
    let second = rows.iter().find(|r| r.url == two).unwrap();
    assert_eq!(second.save_name.as_deref(), Some("named.pdf"));

    // Links none of which can be added: said.
    app.backend
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            links: vec!["http://".into()],
            ..Default::default()
        });
    app.take_handed();
    assert!(
        app.said.english().contains("names no site"),
        "{}",
        app.said.english()
    );
    // One address that is no download: said.
    app.said = Default::default();
    app.backend
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: "http://".into(),
            ..Default::default()
        });
    app.take_handed();
    assert!(
        app.said.english().contains("names no site"),
        "{}",
        app.said.english()
    );
    assert_eq!(app.backend.list().len(), 2);
}

/// A download caught again for a finished file, where the person ticked
/// "Remember my choice" on Show it: answered at once, the row shown.
#[test]
fn a_caught_download_already_listed_is_answered_as_remembered() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .add(&[world.url("file.zip")], dm_core::AddOptions::default())
        .unwrap();
    until(&mut harness, "the download to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let id = id_of(&harness, "file.zip");
    let app = harness.state_mut();
    app.applied.duplicate_choice = "show".into();
    app.selected.clear();
    app.backend
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("file.zip"),
            caught: true,
            ..Default::default()
        });
    app.take_handed();
    assert!(app.dialogs.duplicate.is_none(), "asked though remembered");
    assert!(app.selected.contains(&id));
    assert_eq!(app.backend.list().len(), 1, "fetched a second time");
}

/// `--when-done` from another launch, for what this computer can and
/// cannot do, and a word that is none of them.
#[test]
fn when_done_from_the_command_line_is_taken_where_the_computer_can_do_it() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    let file = inbox().join(dm_shell::shell::WHEN_DONE_FILE);
    for (word, action) in [
        ("sleep", WhenDone::Sleep),
        ("hibernate", WhenDone::Hibernate),
        ("shutdown", WhenDone::ShutDown),
    ] {
        app.session.when_done = WhenDone::Nothing;
        app.said = Default::default();
        std::fs::write(&file, format!("{word}\n")).unwrap();
        app.refresh();
        assert!(!file.exists(), "{word}: left in the inbox");
        if dm_desktop::power::available(action) {
            assert_eq!(app.session.when_done, action, "{word}");
        } else {
            assert_eq!(app.session.when_done, WhenDone::Nothing, "{word}");
            assert_eq!(
                app.said.english(),
                "This computer has no way to do that when the downloads are done.",
                "{word}"
            );
        }
    }
    app.session.when_done = WhenDone::Nothing;
    std::fs::write(&file, "reboot").unwrap();
    app.refresh();
    assert!(!file.exists());
    assert_eq!(app.session.when_done, WhenDone::Nothing);
}

/// A fresh link awaited for too long is no longer awaited: the next
/// download caught is a row of its own. One that is no link is said.
#[test]
fn a_fresh_link_is_awaited_for_a_while_and_a_bad_one_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    add_named(&mut harness, &world, "wanted.bin");
    harness.state_mut().refresh();
    let id = id_of(&harness, "wanted.bin");

    // A bad link handed over while one is awaited.
    let app = harness.state_mut();
    app.await_link(id);
    app.backend
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: "not a link".into(),
            ..Default::default()
        });
    app.take_handed();
    assert!(app.session.hands.awaiting_link.is_none());
    assert!(
        app.said
            .english()
            .contains("must be a link dm can download"),
        "{}",
        app.said.english()
    );

    // Awaited for longer than it is: the next is a download of its own.
    if let Some(long_ago) = Instant::now().checked_sub(Duration::from_secs(3600)) {
        app.await_link(id);
        app.session.hands.awaiting_link = Some((id, long_ago));
        app.backend
            .handed
            .lock()
            .unwrap()
            .push_back(dm_shell::host::Request {
                url: world.url("other.pdf"),
                ..Default::default()
            });
        app.take_handed();
        assert!(app.session.hands.awaiting_link.is_none());
        assert_eq!(app.backend.list().len(), 2, "not a row of its own");
        assert_eq!(
            app.row(id).map(|r| r.url.clone()),
            Some(world.url("wanted.bin"))
        );
    }

    // The bar that says one is awaited has a way out.
    harness.state_mut().await_link(id);
    harness.run_steps(3);
    assert!(harness
        .query_by_label_contains("Click the download again on its page in your browser")
        .is_some());
    let cancel = harness
        .query_all_by_label("Cancel")
        .last()
        .expect("no Cancel beside the words")
        .rect()
        .center();
    harness.hover_at(cancel);
    harness.run_steps(1);
    harness.get_all_by_label("Cancel").last().unwrap().click();
    harness.run_steps(3);
    assert!(harness.state().session.hands.awaiting_link.is_none());
}

/// A sleep the system will not start is said, in the status line and the
/// problems.
#[test]
fn a_countdown_whose_action_the_system_refuses_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = app_on(&world, world.settings(), Arc::new(Refusing::default()));
    app.session.countdown = Some((WhenDone::Sleep, Instant::now()));
    app.countdown_tick();
    assert!(app.session.countdown.is_none());
    assert!(app
        .problems
        .iter()
        .any(|p| p == "Sleep is not allowed here"));
    assert_eq!(app.said.english(), "Sleep is not allowed here");
}

/// In the tray, and with the clipboard watched: the list is still read and
/// the clipboard looked at.
#[test]
fn hidden_in_the_tray_the_list_is_read_and_the_clipboard_watched() {
    let _one = one_at_a_time();
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some(world.url("deep.pdf"));
    let mut harness = world.harness();
    let ctx = harness.ctx.clone();
    let app = harness.state_mut();
    app.hidden = true;
    app.logic(&ctx);
    app.add(&[world.url("stalled.bin")], dm_core::AddOptions::default())
        .unwrap();
    app.logic(&ctx);
    app.clipboard.on = true;
    app.clipboard.polled = None;
    app.logic(&ctx);
    assert_eq!(
        app.clipboard.offered.as_deref(),
        Some(world.url("deep.pdf").as_str())
    );
    assert!(app.clipboard.polled.is_some());
    app.run(Command::StopAll);
}

/// A proxy on loopback that answers every request, after `delay`, with a
/// refusal; and the first line of each request it was sent.
fn refusing_proxy(world: &World, delay: Duration) -> (u16, Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let port = world.runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut tcp, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut head = vec![0u8; 4096];
                    let n = tcp.read(&mut head).await.unwrap_or(0);
                    let line = String::from_utf8_lossy(&head[..n])
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    seen.lock().unwrap().push(line);
                    tokio::time::sleep(delay).await;
                    let _ = tcp
                        .write_all(b"HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\n\r\n")
                        .await;
                });
            }
        });
        port
    });
    (port, asked)
}

/// The weekly check, due, asks through the proxy downloads go through
/// (here one on loopback that refuses it, so nothing leaves the computer),
/// records when it asked, and opens About when a newer dm is found.
#[test]
fn the_weekly_update_check_asks_through_the_proxy_and_offers_what_it_finds() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let (port, asked) = refusing_proxy(&world, Duration::from_millis(1500));
    let mut harness = world.harness();
    let mut wanted = harness.state().applied.clone();
    wanted.proxy = dm_core::Proxy {
        enabled: true,
        kind: dm_core::ProxyKind::Http,
        host: "127.0.0.1".into(),
        port,
        ..Default::default()
    };
    harness.state_mut().apply_settings(wanted).unwrap();
    // The window's client goes through it, or the check is not made.
    let client = harness.state().client();
    let _ = world
        .runtime
        .block_on(async { client.get("http://dm-cover.invalid/probe").send().await });
    assert_eq!(
        asked.lock().unwrap().first().map(String::as_str),
        Some("GET http://dm-cover.invalid/probe HTTP/1.1"),
        "the window's client does not go through the test's proxy; stopping"
    );

    let mut wanted = harness.state().applied.clone();
    wanted.check_updates = true;
    wanted.update_checked = 0;
    harness.state_mut().apply_settings(wanted).unwrap();
    harness.step();
    assert!(
        harness.state().applied.update_checked > 0,
        "when it asked is not kept"
    );
    assert_eq!(
        *harness.state().update.lock().unwrap(),
        dm_desktop::update::Update::Asking
    );
    // A newer one found: About opens to offer it.
    *harness.state().update.lock().unwrap() =
        dm_desktop::update::Update::Newer("99.0.0".into(), None);
    harness.step();
    assert!(harness.state().dialogs.about_open);
    harness.state_mut().dialogs.about_open = false;
    // The real answer: refused by the proxy, and said so.
    let update = harness.state().update.clone();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !matches!(
        *update.lock().unwrap(),
        dm_desktop::update::Update::Failed(_)
    ) {
        assert!(Instant::now() < deadline, "the check never ended");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(asked
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.starts_with("CONNECT github.com:443")));
    let mut off = harness.state().applied.clone();
    off.check_updates = false;
    off.proxy = dm_core::Proxy::default();
    harness.state_mut().apply_settings(off).unwrap();
}

/// "Delete file after" with a file that cannot be moved to the Trash: it
/// stays with its row, is said, and is not tried again. Made so by a
/// read-only folder, which only Unix's permissions give.
#[cfg(unix)]
#[test]
fn a_file_due_for_deletion_that_cannot_be_moved_stays_and_is_said() {
    use std::os::unix::fs::PermissionsExt;
    let _one = one_at_a_time();
    let world = world();
    let mut app = app_on(&world, world.settings(), Arc::new(Refusing::default()));
    app.add(&[world.url("file.zip")], dm_core::AddOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.rows().iter().any(|r| r.state == State::Completed) {
        assert!(Instant::now() < deadline, "never finished");
        std::thread::sleep(Duration::from_millis(25));
        app.refresh();
    }
    let id = app.rows()[0].id;
    let path = std::path::PathBuf::from(&app.rows()[0].path);
    let folder = path.parent().unwrap().to_path_buf();
    // Its folder made read-only, so the file cannot be taken out of it.
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    // As root the folder is no bar: nothing to see then.
    if std::fs::write(folder.join("probe"), b"x").is_ok() {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("the folder can be written read-only (root?); skipped");
        return;
    }
    app.backend.set_delete_after(id, Some(1));
    app.refresh();
    let row = Arc::make_mut(&mut app.rows[0]);
    row.finished_at = Some(1);
    app.delete_due_files();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(path.exists(), "the file was deleted");
    assert!(app.row(id).is_some(), "the row was removed");
    assert!(
        app.said.english().starts_with("Could not delete"),
        "{}",
        app.said.english()
    );
    assert_eq!(
        app.backend.row(id).unwrap().delete_after,
        None,
        "tried again"
    );
}

/// A router answering `/huge.bin` with a length of six gigabytes and then
/// nothing; its address.
fn huge_server(world: &World) -> String {
    let addr = world.runtime.block_on(async {
        let app = axum::Router::new().route(
            "/huge.bin",
            axum::routing::get(|| async {
                let never = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
                Response::builder()
                    .header("content-length", (6u64 << 30).to_string())
                    .body(Body::from_stream(never))
                    .unwrap()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    format!("http://{addr}/huge.bin")
}

/// The hints said once ever (a big download's, Details'): when what marks
/// them said cannot be saved, the reason is said instead.
#[test]
fn a_hint_whose_mark_cannot_be_saved_says_why() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = app_on(&world, world.settings(), Arc::new(Refusing::default()));
    app.add(&[huge_server(&world)], dm_core::AddOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app
        .rows()
        .iter()
        .any(|r| r.state == State::Running && r.progress.total.is_some())
    {
        assert!(Instant::now() < deadline, "never started");
        std::thread::sleep(Duration::from_millis(25));
        app.refresh();
    }
    // The window's copy saying the login item is on, the settings in force
    // that it is off: saving anything changes the login item, which this
    // system refuses.
    app.applied.start_on_login = true;
    app.big_download_hint();
    assert!(
        app.said
            .english()
            .starts_with("could not change start-on-login"),
        "{}",
        app.said.english()
    );
    assert!(!app.applied.hinted_big);
    assert!(
        !app.applied.start_on_login,
        "the window's copy not put back"
    );

    app.applied.start_on_login = true;
    app.said = Default::default();
    app.details_hint();
    assert!(
        app.said
            .english()
            .starts_with("could not change start-on-login"),
        "{}",
        app.said.english()
    );
    app.run(Command::StopAll);
}

#[test]
fn undo_after_its_time_or_with_nothing_puts_nothing_back() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    app.undo_remove();
    assert!(app.rows().is_empty());
    app.add(&[world.url("file.zip")], dm_core::AddOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.rows().iter().any(|r| r.state == State::Completed) {
        assert!(Instant::now() < deadline, "never finished");
        std::thread::sleep(Duration::from_millis(25));
        app.refresh();
    }
    let id = app.rows()[0].id;
    app.remove_finished_with_undo(&[id]);
    assert!(app.rows().is_empty());
    if let Some((at, _)) = app.undo_remove.as_mut() {
        *at = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .unwrap_or(*at);
    }
    app.undo_remove();
    assert!(app.rows().is_empty(), "put back after its time");
    app.undo_remove = Some((Instant::now(), Vec::new()));
    app.undo_remove();
    assert!(app.rows().is_empty());
}

/// A sentence about one row stands while that row has not yet run, and
/// is dropped once something else is said.
#[test]
fn a_sentence_about_a_row_stands_until_it_ran_or_another_is_said() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    add_named(&mut harness, &world, "later.bin");
    harness.state_mut().refresh();
    let id = id_of(&harness, "later.bin");
    let app = harness.state_mut();
    app.said = dm_desktop::i18n::Said::new("Trying {} again with the sign-in.", &[&"later.bin"]);
    app.said_about = Some((id, app.said.clone(), false));
    app.refresh();
    assert!(app.said_about.is_some(), "dropped before the row ran");
    assert!(!app.said.is_empty());
    app.said = dm_desktop::i18n::Said::new("Something else.", &[]);
    app.refresh();
    assert!(app.said_about.is_none());
    assert_eq!(app.said.english(), "Something else.");
}

/// What holds waiting rows, said: the download limit and how long until it
/// goes on; a schedule that never opens.
#[test]
fn held_rows_say_whether_the_limit_or_a_schedule_that_never_opens_holds_them() {
    let _one = one_at_a_time();
    let world = world();
    let _file = settings_file();
    let mut app = world.app();
    let mut wanted = app.applied.clone();
    wanted.limit_mb = 1;
    app.apply_settings(wanted).unwrap();
    for _ in 0..6 {
        app.add(&[world.url("file.zip")], dm_core::AddOptions::default())
            .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.backend.queue.allowance_spent().is_none() {
        assert!(Instant::now() < deadline, "the limit was never reached");
        std::thread::sleep(Duration::from_millis(25));
        app.refresh();
    }
    let held = app.held_because().unwrap();
    assert!(
        held.starts_with("Held by the download limit; it goes on by itself in "),
        "{held}"
    );
    let mut wanted = app.applied.clone();
    wanted.limit_mb = 0;
    app.apply_settings(wanted).unwrap();

    // A schedule on no day there is: shut, and never opening.
    let other = world.app();
    let mut other = other;
    other.applied.schedule = dm_core::Schedule {
        enabled: true,
        from: "01:00".into(),
        to: "02:00".into(),
        days: vec![9],
        ..dm_core::Schedule::default()
    };
    assert_eq!(
        other.held_because().as_deref(),
        Some("Held by the schedule.")
    );
}

/// Several links added at once where some cannot be: the rest go in, and
/// how many did not is said; none at all is the reason itself.
#[test]
fn links_that_cannot_be_added_are_counted_and_the_rest_go_in() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    let later = dm_core::AddOptions {
        later: true,
        ..Default::default()
    };
    let one_bad = [world.url("a.bin"), "http://".to_string()];
    assert_eq!(app.add(&one_bad, later.clone()), Ok(1));
    assert!(app
        .problems
        .iter()
        .any(|p| p.starts_with("One of the links was not added: ")));
    let two_bad = [
        "http://".to_string(),
        world.url("b.bin"),
        "https://".to_string(),
    ];
    assert_eq!(app.add(&two_bad, later.clone()), Ok(1));
    assert!(app
        .problems
        .iter()
        .any(|p| p.starts_with("2 of the links were not added: ")));
    let err = app.add(&["http://".to_string()], later).unwrap_err();
    assert!(err.contains("names no site"), "{err}");
    assert_eq!(app.backend.list().len(), 2);
}

/// The rows already there when the window first reads the list are not
/// asked about: a name the server gave an earlier run's download.
#[test]
fn rows_from_before_the_window_opened_are_not_asked_about() {
    let _one = one_at_a_time();
    let world = world();
    let settings = world.settings();
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
        world.fake.clone(),
    );
    let id = backend
        .add_with(&world.url("named"), dm_core::AddOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while backend.row(id).is_none_or(|r| r.state != State::Completed) {
        assert!(Instant::now() < deadline, "never finished");
        std::thread::sleep(Duration::from_millis(25));
    }
    let mut app = DmApp::new(backend);
    app.refresh();
    assert_eq!(app.row(id).unwrap().filename, "report-2026.pdf");
    assert!(
        app.dialogs.name_ask.is_none(),
        "asked about an earlier run's row"
    );
}

/// "No" to the server's name: a finished file is renamed at once, a failed
/// one is left, and one still running is renamed when it finishes, or not
/// at all if it fails.
#[test]
fn no_to_the_servers_name_renames_a_finished_file_and_leaves_a_failed_one() {
    let _one = one_at_a_time();
    let world = world();
    let mut harness = world.harness();
    let ask = |id, address: &str| {
        Some(dm_desktop::dialogs::NameAsk {
            id,
            address_name: address.into(),
            server_name: "server.bin".into(),
        })
    };
    harness
        .state_mut()
        .add(&[world.url("file.zip")], dm_core::AddOptions::default())
        .unwrap();
    until(&mut harness, "the download to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let done = id_of(&harness, "file.zip");
    harness.state_mut().dialogs.name_ask = ask(done, "mine.zip");
    harness.run_steps(3);
    click(&mut harness, "No");
    until(&mut harness, "the file to take the address's name", |app| {
        app.row(done).is_some_and(|r| r.filename == "mine.zip")
    });
    assert!(std::path::Path::new(&harness.state().row(done).unwrap().path).exists());

    // A failed row: nothing to rename.
    harness
        .state_mut()
        .add(&[world.url("secret.zip")], dm_core::AddOptions::default())
        .unwrap();
    until(&mut harness, "the download to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let failed = id_of(&harness, "secret.zip");
    harness.state_mut().dialogs.name_ask = ask(failed, "other.zip");
    harness.run_steps(3);
    click(&mut harness, "No");
    frames(&mut harness, 2);
    assert_eq!(id_of(&harness, "secret.zip"), failed);

    // Running, then failed: the rename waiting for it is dropped.
    harness
        .state_mut()
        .add(&[world.url("drops.bin")], dm_core::AddOptions::default())
        .unwrap();
    until(&mut harness, "the download to start", |app| {
        app.rows()
            .iter()
            .any(|r| dm_desktop::app::name_of(r) == "drops.bin" && r.state == State::Running)
    });
    let dropping = id_of(&harness, "drops.bin");
    harness.state_mut().dialogs.name_ask = ask(dropping, "kept.bin");
    harness.run_steps(3);
    click(&mut harness, "No");
    harness.state_mut().run(Command::StopAll);
    harness.state().backend().cancel(dropping);
    frames(&mut harness, 3);
    assert_ne!(
        harness.state().row(dropping).map(|r| r.filename.clone()),
        Some("kept.bin".into())
    );
}

/* --------------------------------------------------------------- backend */

/// What a platform gets from the trait when it does not say otherwise: an
/// opener's failure reported, the finish's sound for everything, a folder
/// chooser that answers as a Cancel, and a file deleted for want of a Trash.
#[test]
fn a_platform_that_says_nothing_more_gets_the_traits_own_ways() {
    let platform = Refusing::default();
    let reported = Arc::new(Mutex::new(Vec::new()));
    let report = |into: &Arc<Mutex<Vec<String>>>| -> dm_desktop::power::Report {
        let into = into.clone();
        Box::new(move |why| into.lock().unwrap().push(why))
    };
    platform.open_later("/x/a.zip", report(&reported));
    platform.reveal_later("/x/a.zip", report(&reported));
    assert_eq!(
        *reported.lock().unwrap(),
        ["cannot open /x/a.zip", "cannot show /x/a.zip"]
    );
    platform.play_file("/sounds/done.wav");
    platform.alert();
    platform.tick();
    platform.notify_file("Download finished", "a.zip", "/x/a.zip");
    assert_eq!(*platform.said.lock().unwrap(), ["chime", "chime", "a.zip"]);
    assert_eq!(platform.choose_folder(), None);
    assert!(platform.can_choose_folder());
    assert!(platform.can_choose_file());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gone.bin");
    std::fs::write(&file, b"x").unwrap();
    platform.trash(&file.display().to_string()).unwrap();
    assert!(!file.exists());
    assert!(platform.trash(&file.display().to_string()).is_err());
}

#[test]
fn the_backends_verbs_on_rows_that_cannot_take_them_say_why() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut app = world.app();
    let backend = app.backend.clone();
    // Not a torrent's files, and none ticked.
    let id = backend
        .add_with(
            &world.url("held.bin"),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        backend.set_torrent_files(id, &[false]),
        Err("Tick at least one file.".into())
    );
    assert!(backend.set_torrent_files(id, &[true]).is_err());
    // An address that is no link.
    assert_eq!(
        backend.set_url(id, "not a link"),
        Err("it must be a link dm can download".into())
    );
    // Resuming a row that is gone, or one already running, does nothing.
    backend.resume(id + 1000);
    backend.flush();
    assert_eq!(backend.list().len(), 1);
    // The settings shared with a thread of its own are the ones in force.
    let shared = backend
        .settings_handle()
        .lock()
        .unwrap()
        .download_dir
        .clone();
    assert_eq!(shared, backend.settings().download_dir);
    // "Always save here without asking", from the dialog: kept alone.
    let now = backend.set_ask_caught_downloads(true).unwrap();
    assert!(now.ask_caught_downloads);
    assert!(Settings::load().0.ask_caught_downloads);
    backend.set_ask_caught_downloads(false).unwrap();
    assert!(!Settings::load().0.ask_caught_downloads);

    // A finished row's address cannot be changed.
    app.add(&[world.url("file.zip")], dm_core::AddOptions::default())
        .unwrap();
    let done = *app.last_added.first().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while backend
        .row(done)
        .is_none_or(|r| r.state != State::Completed)
    {
        assert!(Instant::now() < deadline, "never finished");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(backend
        .set_url(done, &world.url("other.pdf"))
        .unwrap_err()
        .starts_with("a running or finished download's address cannot be changed"));
}

/// A running download given a new address is stopped, given it, and
/// started again.
#[test]
fn a_running_download_given_a_new_address_goes_on_from_it() {
    let _one = one_at_a_time();
    let world = world();
    let mut app = world.app();
    app.add(&[world.url("stalled.bin")], dm_core::AddOptions::default())
        .unwrap();
    let id = *app.last_added.first().unwrap();
    let backend = app.backend.clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    while backend.row(id).is_none_or(|r| r.state != State::Running) {
        assert!(Instant::now() < deadline, "never started");
        std::thread::sleep(Duration::from_millis(25));
    }
    // Resumed while it runs: nothing changes.
    backend.resume(id);
    let fresh = format!("{}?fresh=1", world.url("stalled.bin"));
    backend.set_url(id, &fresh).unwrap();
    let row = backend.row(id).unwrap();
    assert_eq!(row.url, fresh);
    let deadline = Instant::now() + Duration::from_secs(10);
    while backend.row(id).is_none_or(|r| r.state != State::Running) {
        assert!(Instant::now() < deadline, "not started again");
        std::thread::sleep(Duration::from_millis(25));
    }
    backend.pause(id);
}

/// A cookies file moved away since it was chosen does not stop a change of
/// settings: it is a problem said beside it.
#[test]
fn a_change_of_settings_with_a_cookies_file_gone_is_made_and_the_problem_said() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut app = world.app();
    let cookies = world.dir.path().join("cookies.txt");
    std::fs::write(&cookies, "# Netscape HTTP Cookie File\n").unwrap();
    let mut wanted = app.applied.clone();
    wanted.cookies_file = cookies.display().to_string();
    app.apply_settings(wanted).unwrap();
    std::fs::remove_file(&cookies).unwrap();
    let mut other = app.applied.clone();
    other.hinted_big = !other.hinted_big;
    app.apply_settings(other).unwrap();
    let problems = app.backend.problems();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("downloads go without its cookies")),
        "{problems:?}"
    );
    let mut back = app.applied.clone();
    back.cookies_file = String::new();
    app.apply_settings(back).unwrap();
}

/* ------------------------------------------------------------ the schedule */

/// The schedule open all day, every day.
fn always_open(when_done: &str, retries: u8, run_when_done: &str) -> dm_core::Schedule {
    dm_core::Schedule {
        enabled: true,
        from: "00:00".into(),
        to: "00:00".into(),
        days: (0..=6).collect(),
        when_done: when_done.into(),
        retries,
        run_when_done: run_when_done.into(),
        hang_up: true,
        ..dm_core::Schedule::default()
    }
}

/// A once-only date that is over is put away, for every queue and for a
/// queue of its own, and the weekly window is back.
#[test]
fn a_once_only_date_that_is_over_is_put_away() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut app = world.app();
    let spent = dm_core::Schedule {
        once_at: Some("2020-01-01 01:00".into()),
        enabled: true,
        from: "01:00".into(),
        to: "02:00".into(),
        ..dm_core::Schedule::default()
    };
    app.applied.schedule = spent.clone();
    app.applied
        .queue_schedules
        .insert("Nightly".into(), spent.clone());
    app.refresh();
    assert_eq!(app.applied.schedule.once_at, None);
    assert_eq!(app.applied.queue_schedules["Nightly"].once_at, None);
    assert_eq!(
        app.backend.settings().queue_schedules["Nightly"].once_at,
        None,
        "not put into force"
    );
    let mut back = app.applied.clone();
    back.schedule = dm_core::Schedule::default();
    back.queue_schedules.clear();
    app.apply_settings(back).unwrap();
}

/// When the schedule's window opens on something waiting to run, its
/// "when done" is armed as the session's; closed with it unused, it is put
/// away again.
#[test]
fn the_schedules_when_done_is_armed_as_its_window_opens_and_put_away_as_it_closes() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut app = world.app();
    app.add(&[world.url("stalled.bin")], dm_core::AddOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.rows().iter().any(|r| r.state == State::Running) {
        assert!(Instant::now() < deadline, "never started");
        std::thread::sleep(Duration::from_millis(25));
        app.refresh();
    }
    let mut wanted = app.applied.clone();
    wanted.schedule = always_open("sleep", 0, "");
    app.apply_settings(wanted).unwrap();
    app.refresh();
    if dm_desktop::power::available(WhenDone::Sleep) {
        assert_eq!(app.session.when_done, WhenDone::Sleep);
        assert!(
            app.said
                .english()
                .starts_with("The schedule has set what dm does when the downloads are done: "),
            "{}",
            app.said.english()
        );
    } else {
        assert_eq!(app.session.when_done, WhenDone::Nothing);
    }
    let mut shut = app.applied.clone();
    shut.schedule.enabled = false;
    app.apply_settings(shut).unwrap();
    app.refresh();
    assert_eq!(app.session.when_done, WhenDone::Nothing, "left armed");
    app.run(Command::StopAll);
}

/// While the window is open a failed download is tried again as many times
/// as allowed, and then the program set to run at the end is started; one
/// that cannot start is a problem.
#[test]
fn the_schedule_tries_a_failure_again_then_runs_its_program() {
    let _one = one_at_a_time();
    let _file = settings_file();
    let world = world();
    let mut app = world.app();
    let program = world.dir.path().join("no-such-program");
    let mut wanted = app.applied.clone();
    wanted.schedule = always_open("", 1, &program.display().to_string());
    app.apply_settings(wanted).unwrap();
    let asked = SECRET_ASKED.load(std::sync::atomic::Ordering::SeqCst);
    app.add(&[world.url("secret.zip")], dm_core::AddOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !app.problems.iter().any(|p| p.contains("no-such-program")) {
        assert!(
            Instant::now() < deadline,
            "the program at the end was never tried"
        );
        std::thread::sleep(Duration::from_millis(25));
        app.refresh();
    }
    assert!(
        SECRET_ASKED.load(std::sync::atomic::Ordering::SeqCst) >= asked + 2,
        "not tried again"
    );
    assert!(app
        .problems
        .iter()
        .any(|p| p.contains("to run when the scheduled downloads are done, could not start")));
    let mut back = app.applied.clone();
    back.schedule = dm_core::Schedule::default();
    app.apply_settings(back).unwrap();
}

/* ------------------------------------------------------------------ rows */

/// A row to say things of: one added for later, as the queue keeps it.
fn a_row(world: &World) -> dm_core::Entry {
    let app = world.app();
    let id = app
        .backend
        .add_with(
            &world.url("f.bin"),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    app.backend.row(id).unwrap()
}

#[test]
fn every_state_has_its_word_and_every_kind_its_name() {
    use dm_desktop::app::{category_word, kind};
    let states = [
        (State::Queued, "queued"),
        (State::Running, "running"),
        (State::Paused, "paused"),
        (State::Completed, "completed"),
        (State::Failed("x".into()), "failed"),
        (State::Cancelled, "cancelled"),
    ];
    for (state, word) in states {
        assert_eq!(kind(&state), word);
    }
    assert_eq!(category_word(dm_core::Category::Videos), "videos");
    assert_eq!(category_word(dm_core::Category::Apps), "apps");
}

#[test]
fn what_a_row_says_of_retries_stalls_sign_ins_and_a_file_due_to_go() {
    use dm_desktop::app::{link_expired, set_retrying, stalled_said, status_in_english};
    let _one = one_at_a_time();
    let world = world();
    dm_desktop::i18n::set(dm_desktop::i18n::chosen("en"));
    let base = a_row(&world);

    // Retrying, with and without an end, with and without a cause.
    let mut running = base.clone();
    running.state = State::Running;
    let retry = |attempt, of, why: &str| {
        let mut now = std::collections::HashMap::new();
        now.insert(running.id, (attempt, of, why.to_string(), Instant::now()));
        set_retrying(now);
        status_in_english(&running)
    };
    assert_eq!(retry(2, 5, "something odd"), "Retrying (attempt 2 of 5)");
    assert_eq!(
        retry(3, 0, "connection reset by peer"),
        "Connection dropped (retry 3)"
    );
    assert_eq!(retry(4, 0, "something odd"), "Retrying (attempt 4)");
    set_retrying(std::collections::HashMap::new());

    // Some bytes of a file of unknown length, nothing arriving now.
    running.progress.downloaded = 2048;
    running.progress.speed = 0.0;
    assert_eq!(status_in_english(&running), "2.0 KB");

    // Not failed: not a link that ran out.
    assert!(!link_expired(&base));
    let mut expired = base.clone();
    expired.state = State::Failed("the server answered 403 Forbidden".into());
    expired.progress.downloaded = 10;
    assert!(link_expired(&expired));

    // Stopped by itself: a full disk names its folder where it knows one.
    let mut stalled = base.clone();
    stalled.stalled = "No space left on device (os error 28)".into();
    stalled.save_dir = Some("/Volumes/Big".into());
    assert_eq!(stalled_said(&stalled), "the disk is full in /Volumes/Big");
    stalled.save_dir = None;
    stalled.began_in = String::new();
    assert_eq!(stalled_said(&stalled), "the disk is full");
    stalled.stalled = "the drive was taken away".into();
    assert_eq!(stalled_said(&stalled), "the drive was taken away");

    // A refusal that wants a sign-in, in words dm has none plainer for.
    let mut refused = base.clone();
    refused.state = State::Failed("the ftp server said 401 not logged in".into());
    assert_eq!(
        status_in_english(&refused),
        "Error: the ftp server said 401 not logged in — it needs a sign-in: Properties › Login"
    );

    // Finished, its time to go come: "Complete" until it has gone.
    let file = world.dir.path().join("kept.bin");
    std::fs::write(&file, b"x").unwrap();
    let mut due = base.clone();
    due.state = State::Completed;
    due.path = file.display().to_string();
    due.finished_at = Some(1);
    due.delete_after = Some(1);
    assert_eq!(status_in_english(&due), "Complete");
}

/// Sorted by Status: running, queued, stopped, failed, cancelled, done.
#[test]
fn sorted_by_status_the_rows_go_in_the_order_of_their_state() {
    let _one = one_at_a_time();
    let world = world();
    let mut settings = world.settings();
    settings.concurrency = 1;
    let mut harness = harness_of(world.app_with(settings));
    for name in ["stalled.bin", "half.mp4"] {
        let url = world.url(name);
        harness
            .state_mut()
            .add(&[url], dm_core::AddOptions::default())
            .unwrap();
    }
    add_named(&mut harness, &world, "cancelled.bin");
    harness.state_mut().refresh();
    let cancelled = id_of(&harness, "cancelled.bin");
    harness.state().backend().cancel(cancelled);
    add_named(&mut harness, &world, "stopped.bin");
    until(&mut harness, "one running and one queued", |app| {
        let states: Vec<State> = app.rows().iter().map(|r| r.state.clone()).collect();
        states.contains(&State::Running)
            && states.contains(&State::Queued)
            && states.contains(&State::Cancelled)
    });
    harness.state_mut().sort = Some((Column::Status, true));
    harness.run_steps(2);
    let order: Vec<String> = harness
        .state()
        .visible()
        .iter()
        .map(|r| dm_desktop::app::kind(&r.state).to_string())
        .collect();
    assert_eq!(order, ["running", "queued", "paused", "cancelled"]);
    harness.state_mut().run(Command::StopAll);
}
