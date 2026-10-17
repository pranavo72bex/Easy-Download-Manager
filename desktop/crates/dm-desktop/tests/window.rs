//! The window, driven the way a person drives it — through the names a
//! screen reader reads, the words on its buttons and dialogs — against a real queue fetching from a real server on loopback.
//! Nothing is stubbed but the operating system: the login item,
//! notifications, the clipboard, opening files and choosing them.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::response::Response;
use dm_core::{Queue, State};
use dm_desktop::app::{Column, DmApp, Place};
use dm_desktop::backend::{Backend, Platform};
use dm_desktop::commands::Command;
use dm_shell::Settings;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

/// Held by every test that writes the settings file: they share one, and a
/// test reading back what it wrote must not read what another wrote.
static SETTINGS_FILE: Mutex<()> = Mutex::new(());

/// Every test's settings file and queue live here, never in the real home.
fn home() -> &'static std::path::Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        // Set once, before any test reads it.
        unsafe { std::env::set_var("DM_HOME", dir.path()) };
        dir
    })
    .path()
}

#[derive(Default)]
struct Fake {
    notified: Mutex<Vec<String>>,
    clipboard: Mutex<Option<String>>,
    login_fails: Mutex<bool>,
    power_fails: Mutex<bool>,
    login: Mutex<Option<bool>>,
    opened: Mutex<Vec<String>>,
    chosen: Mutex<Option<std::path::PathBuf>>,
}

impl Platform for Fake {
    fn set_login_item(&self, wanted: bool) -> Result<(), String> {
        if *self.login_fails.lock().unwrap() {
            return Err("not allowed here".into());
        }
        *self.login.lock().unwrap() = Some(wanted);
        Ok(())
    }
    fn notify(&self, _title: &str, body: &str) {
        self.notified.lock().unwrap().push(body.to_string());
    }
    fn clipboard(&self) -> Option<String> {
        self.clipboard.lock().unwrap().clone()
    }
    fn reveal(&self, path: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(format!("reveal {path}"));
        Ok(())
    }
    fn open(&self, path: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(path.to_string());
        Ok(())
    }
    fn choose_file(&self, _save: bool, _suggested: &str) -> Option<std::path::PathBuf> {
        self.chosen.lock().unwrap().clone()
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        self.chosen.lock().unwrap().clone()
    }
    fn chime(&self) {
        self.opened.lock().unwrap().push("chime".into());
    }
    fn alert(&self) {
        self.opened.lock().unwrap().push("alert".into());
    }
    fn power(
        &self,
        action: dm_desktop::power::WhenDone,
        report: dm_desktop::power::Report,
    ) -> Result<(), String> {
        self.opened
            .lock()
            .unwrap()
            .push(format!("power {action:?}"));
        if *self.power_fails.lock().unwrap() {
            // As the system's command does: started, and refusing later.
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                report("could not put the computer to sleep: not allowed".into());
            });
        }
        Ok(())
    }
    fn open_with(&self, path: &str, app: &std::path::Path) -> Result<(), String> {
        self.opened
            .lock()
            .unwrap()
            .push(format!("{path} with {}", app.display()));
        Ok(())
    }
}

struct World {
    runtime: tokio::runtime::Runtime,
    fake: Arc<Fake>,
    dir: tempfile::TempDir,
    addr: SocketAddr,
}

const PAYLOAD: usize = 256 * 1024;

fn payload() -> Vec<u8> {
    (0..PAYLOAD).map(|i| (i * 7 % 251) as u8).collect()
}

/// A server with one file that arrives at once and one that never does.
fn world() -> World {
    home();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let addr = runtime.block_on(async {
        let app = axum::Router::new()
            .route(
                "/file.zip",
                axum::routing::get(|| async { Response::new(Body::from(payload())) }),
            )
            .route(
                "/page.html",
                axum::routing::get(|| async {
                    Response::new(Body::from(
                        r#"<a href="file.zip">one</a> <a href="/other.pdf">two</a> <a href="next.html">page</a>"#,
                    ))
                }),
            )
            .route(
                "/other.pdf",
                axum::routing::get(|| async { Response::new(Body::from(payload())) }),
            )
            .route(
                "/next.html",
                axum::routing::get(|| async {
                    // Said to be a page, so "Download all links" hides it.
                    Response::builder()
                        .header("content-type", "text/html")
                        .body(Body::from(
                            r#"<a href="deep.pdf">deep</a> <a href="page.html">back</a> <a href="http://elsewhere.invalid/x.html">away</a>"#,
                        ))
                        .unwrap()
                }),
            )
            .route(
                "/deep.pdf",
                axum::routing::get(|| async { Response::new(Body::from(payload())) }),
            )
            .route(
                "/stalled.bin",
                axum::routing::get(|| async {
                    let never = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
                    Response::builder()
                        .header("content-length", "1000000")
                        .body(Body::from_stream(never))
                        .unwrap()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    World {
        runtime,
        fake: Arc::default(),
        dir: tempfile::tempdir().unwrap(),
        addr,
    }
}

impl World {
    fn url(&self, name: &str) -> String {
        format!("http://{}/{name}", self.addr)
    }

    fn app(&self) -> DmApp {
        // Without the progress and complete windows unless a test asks:
        // most tests are about the list, not about windows over it.
        self.app_with(Settings {
            download_dir: self.dir.path().join("out"),
            connections: 1,
            show_progress: false,
            show_complete: false,
            // The tests find controls by their English names, whatever the
            // machine running them is set to.
            language: "en".into(),
            ..Settings::default()
        })
    }

    /// A window over these settings, as a launch that read them would be.
    fn app_with(&self, settings: Settings) -> DmApp {
        let queue = Queue::new(
            dm_core::client(),
            settings.options(self.dir.path().join("parts"), dm_core::Limiter::unlimited()),
            settings.concurrency,
        );
        let backend = Backend::new(
            self.runtime.handle().clone(),
            queue,
            settings,
            Vec::new(),
            self.fake.clone(),
        );
        DmApp::new(backend)
    }

    fn harness(&self) -> Harness<'static, DmApp> {
        Harness::builder()
            .with_size(egui::vec2(1100.0, 720.0))
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), self.app())
    }

    /// A harness with 20 ms frames, so two clicks can land inside egui's
    /// 0.3 s double-click window (each input event takes a frame).
    fn quick_harness(&self) -> Harness<'static, DmApp> {
        Harness::builder()
            .with_size(egui::vec2(1100.0, 720.0))
            .with_step_dt(0.02)
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), self.app())
    }
}

/// Runs frames until `done` says so, or fails after a few seconds.
fn until(harness: &mut Harness<'_, DmApp>, what: &str, done: impl Fn(&DmApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        harness.state_mut().refresh();
        harness.step();
        if done(harness.state()) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Two clicks inside egui's double-click time, as a person's double-click;
/// with `quick_harness`'s 20 ms frames they land about 0.12 s apart.
///
/// A second first: egui counts a third click within 0.6 s as a triple,
/// which is not a double, and the test's own clicks may be that recent.
fn double_click(harness: &mut Harness<'_, DmApp>, name: &str) {
    harness.run_steps(40);
    harness.get_by_label(name).click();
    harness.get_by_label(name).click();
    harness.run_steps(2);
}

/// Clicks something by its name and lets the window catch up.
fn click(harness: &mut Harness<'_, DmApp>, name: &str) {
    harness.get_by_label(name).click();
    harness.run_steps(3);
}

/// Types into a field found by its name.
fn type_into(harness: &mut Harness<'_, DmApp>, name: &str, text: &str) {
    harness.get_by_label(name).click();
    harness.run_steps(2);
    harness.get_by_label(name).type_text(text);
    harness.run_steps(2);
}

/// Types into the text field of that name, where a column heading or a
/// label shares it.
fn type_role(harness: &mut Harness<'_, DmApp>, name: &str, text: &str) {
    use egui::accesskit::Role;
    harness.get_by_role_and_label(Role::TextInput, name).click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::TextInput, name)
        .type_text(text);
    harness.run_steps(2);
}

/// Add URL, the address, OK, then the File Info dialog's button.
fn add_url(harness: &mut Harness<'_, DmApp>, url: &str, button: &str) {
    click(harness, "Add URL");
    type_into(harness, "Address", url);
    click(harness, "OK");
    // An address already in the list is asked about first; a test adding
    // the same file twice means it.
    if harness.query_by_label("Already in the list").is_some() {
        click(harness, "Download again");
    }
    assert!(
        harness.query_by_label("Download details").is_some(),
        "OK did not lead to Download details"
    );
    click(harness, button);
}

/// Selects a row by clicking its name in the table.
fn select(harness: &mut Harness<'_, DmApp>, name: &str) {
    click(harness, name);
    assert!(
        harness
            .state()
            .targets()
            .iter()
            .any(|r| dm_desktop::app::name_of(r) == name),
        "clicking {name} did not select it"
    );
}

#[test]
fn a_finished_file_dragged_out_of_the_list_is_handed_to_the_systems_drag() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = false;
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("stalled.bin"), "Add for later");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    let done = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.state == State::Completed)
        .unwrap()
        .clone();

    // Pressed on its name and moved: a drag, not a click.
    let drag = |harness: &mut Harness<'_, DmApp>, name: &str| {
        let at = harness.get_by_label(name).rect().center();
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(at));
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        harness.step();
        for step in 1..=4 {
            let to = at + egui::vec2(12.0 * step as f32, 6.0 * step as f32);
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(to));
            harness.step();
        }
        let carried = std::mem::take(&mut harness.state_mut().drag_out);
        let end = at + egui::vec2(48.0, 24.0);
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        harness.run_steps(2);
        carried
    };
    assert_eq!(
        drag(&mut harness, "file.zip"),
        [std::path::PathBuf::from(&done.path)],
        "the finished file was not carried"
    );
    assert!(
        drag(&mut harness, "stalled.bin").is_empty(),
        "an unfinished download has no file to carry"
    );
}

#[test]
fn add_url_downloads_the_file_announces_it_once_and_says_it_is_complete() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;

    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);

    let row = harness.state().rows()[0].clone();
    assert_eq!(row.filename, "file.zip");
    assert_eq!(
        std::fs::read(&row.path).unwrap(),
        payload(),
        "the saved bytes are not the served ones"
    );
    assert_eq!(
        *world.fake.notified.lock().unwrap(),
        ["file.zip"],
        "announced not exactly once"
    );

    assert!(harness.query_by_label("Download complete").is_some());
    click(&mut harness, "Open");
    assert_eq!(
        *world.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );
    assert!(harness.query_by_label("Download complete").is_none());
}

#[test]
fn a_newer_release_offers_its_installer_as_a_download_in_dm() {
    let world = world();
    let mut harness = world.harness();
    // The local server's file stands in for the release's installer.
    let installer = world.url("file.zip");
    *harness.state().update.lock().unwrap() =
        dm_desktop::update::Update::Newer("9.9.9".into(), Some(installer.clone()));
    harness.state_mut().dialogs.about_open = true;
    harness.run_steps(2);
    click(&mut harness, "Download the update");
    until(&mut harness, "the installer to be fetched", |app| {
        app.rows()
            .iter()
            .any(|r| r.url == installer && r.state == State::Completed)
    });
}

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

    harness.state_mut().run(Command::WhenDoneQuit);
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "dm to quit", |app| app.quit_asked);
}

#[test]
fn a_finished_file_deleted_since_says_so_and_is_not_offered_to_open() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    select(&mut harness, "file.zip");
    assert!(harness.state().enabled(Command::OpenFile));

    std::fs::remove_file(&row.path).unwrap();
    harness.run_steps(3);
    assert!(harness.query_by_label("File missing").is_some());
    assert!(!harness.state().enabled(Command::OpenFile));
    assert!(!harness.state().enabled(Command::OpenWith));
    assert!(
        harness.state().enabled(Command::OpenFolder),
        "Open folder still answers: it says the file is gone"
    );
    assert!(harness.state().enabled(Command::Redownload));
}

#[test]
fn links_on_the_clipboard_are_picked_and_put_in_the_main_queue() {
    let world = world();
    let mut harness = world.harness();
    *world.fake.clipboard.lock().unwrap() = Some(format!(
        "see {} and {} and not this",
        world.url("file.zip"),
        world.url("clip.bin")
    ));
    harness.state_mut().run(Command::AddFromClipboard);
    harness.run_steps(2);
    let offered: Vec<String> = harness
        .state()
        .dialogs
        .clip_links
        .as_ref()
        .unwrap()
        .iter()
        .map(|l| l.0.clone())
        .collect();
    assert_eq!(offered, [world.url("file.zip"), world.url("clip.bin")]);
    click(&mut harness, "Add for later");
    harness.state_mut().refresh();
    let rows = harness.state().rows();
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|r| r.queue == dm_core::MAIN_QUEUE && r.state == State::Paused));
}

#[test]
fn a_video_from_the_browsers_panel_opens_download_file_info_with_its_quality() {
    let world = world();
    let mut harness = world.harness();
    let page = world.url("file.zip");
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: page.clone(),
            referrer: "https://video.example/watch".into(),
            quality: Some("720".into()),
            label: Some("MP4 1280×720".into()),
            ask: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(3);
    assert!(harness.query_by_label("Download details").is_some());
    assert!(harness.query_by_label("MP4 1280×720").is_some());
    assert!(
        harness.state().rows().is_empty(),
        "nothing starts before it is asked"
    );

    click(&mut harness, "Start now");
    harness.state_mut().refresh();
    let row = harness.state().rows()[0].clone();
    assert_eq!(row.url, page);
    assert_eq!(row.quality.as_deref(), Some("720"));
    assert_eq!(row.referrer, "https://video.example/watch");
}

#[test]
fn the_category_tree_has_right_click_menus() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(2);

    // A queue: Start queue runs the synchronization check on it.
    harness
        .get_by_label_contains("Recheck queue")
        .click_secondary();
    harness.run_steps(2);
    // The toolbar has a Start queue too; the menu's is the later one.
    harness
        .query_all_by_label("Start queue")
        .last()
        .unwrap()
        .click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().place,
        dm_desktop::app::Place::Queue(dm_core::RECHECK_QUEUE.into()),
        "a right-click selects the place"
    );
    assert!(
        harness.state().said.english().contains("Checked"),
        "{}",
        harness.state().said.english()
    );

    // Done: Clear all finished.
    harness.get_by_label_contains("Done (").click_secondary();
    harness.run_steps(2);
    click(&mut harness, "Clear all finished");
    harness.state_mut().refresh();
    assert!(harness.state().rows().is_empty());

    // A kind: categories are edited in Options › Save to.
    harness.get_by_label_contains("Videos (").click_secondary();
    harness.run_steps(2);
    click(&mut harness, "Add or edit categories…");
    let tab = harness.state().dialogs.options.as_ref().map(|o| o.tab);
    assert_eq!(tab, Some(dm_desktop::dialogs::Tab::SaveTo));
}

#[test]
fn a_pages_links_open_the_download_all_links_dialog() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("page.html"),
            referrer: world.url("page.html"),
            links: vec![
                world.url("file.zip"),
                world.url("other.pdf"),
                world.url("next.html"),
            ],
            ask: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(2);
    assert!(harness.query_by_label("Download all links").is_some());
    // Each link's server is asked for its kind and size.
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.query_by_label("Archives").is_none() {
        assert!(Instant::now() < deadline, "the links were never checked");
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(50));
    }
    // "Hide HTML files" is on by default: the page is not offered.
    assert!(harness.query_by_label("next.html").is_none());
    click(&mut harness, "other.pdf");
    click(&mut harness, "OK");
    harness.state_mut().refresh();
    let rows = harness.state().rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].url, world.url("file.zip"));
    assert_eq!(rows[0].queue, dm_core::MAIN_QUEUE);
    assert_eq!(
        rows[0].state,
        State::Paused,
        "into the queue, to start with it"
    );
    assert_eq!(rows[0].referrer, world.url("page.html"));
}

#[test]
fn a_double_click_opens_properties_and_open_runs_a_finished_file() {
    let world = world();
    let mut harness = world.quick_harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "one to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(2);

    // An unfinished one: Properties, with nothing yet to open.
    double_click(&mut harness, "stalled.bin");
    harness.run_steps(3);
    assert!(harness.query_by_label("File properties").is_some());
    let open = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Open");
    assert!(open.accesskit_node().is_disabled(), "nothing to open yet");
    harness.state_mut().dialogs.properties.clear();
    harness.run_steps(2);

    // A finished one: Properties, whose Open runs the file.
    let row = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.state == State::Completed)
        .cloned()
        .unwrap();
    double_click(&mut harness, "file.zip");
    harness.run_steps(3);
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Open")
        .click();
    harness.run_steps(2);
    assert_eq!(*world.fake.opened.lock().unwrap(), [row.path]);
}

#[test]
fn open_with_asks_for_an_application_and_opens_the_file_with_it() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    select(&mut harness, "file.zip");
    assert!(harness.state().enabled(Command::OpenWith));

    // Cancelled: nothing is opened.
    harness.state_mut().run(Command::OpenWith);
    assert!(world.fake.opened.lock().unwrap().is_empty());

    let app = std::path::PathBuf::from("/Applications/Archive Utility.app");
    *world.fake.chosen.lock().unwrap() = Some(app.clone());
    harness.get_by_label("file.zip").click_secondary();
    harness.run_steps(2);
    click(&mut harness, "Open with…");
    assert_eq!(
        *world.fake.opened.lock().unwrap(),
        [format!("{} with {}", row.path, app.display())]
    );
}

#[test]
fn the_file_info_dialog_puts_the_file_where_and_as_what_it_says() {
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    let folder = world.dir.path().join("picked");
    type_into(&mut harness, "Save to folder", &folder.to_string_lossy());
    type_into(&mut harness, "Save as", "renamed.zip");
    type_role(&mut harness, "Note", "the one from the page");
    click(&mut harness, "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    assert_eq!(std::path::Path::new(&row.path), folder.join("renamed.zip"));
    assert_eq!(row.description, "the one from the page");
    harness.run_steps(2);
    assert!(
        harness.query_by_label("the one from the page").is_some(),
        "the Note column is empty"
    );
}

#[test]
fn download_later_waits_in_the_main_queue_until_start_queue() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.state_mut().refresh();
    let row = harness.state().rows()[0].clone();
    assert_eq!(row.state, State::Paused);
    assert_eq!(row.queue, dm_core::MAIN_QUEUE);

    harness.get_by_label_contains("Main queue").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().place,
        Place::Queue(dm_core::MAIN_QUEUE.into())
    );
    click(&mut harness, "Start queue");
    until(&mut harness, "the queue's download to complete", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
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
    click(&mut harness, "Delete");
    assert!(harness
        .query_by_label("Delete unfinished downloads")
        .is_some());
    click(&mut harness, "Keep them");
    assert_eq!(harness.state().rows().len(), 1, "Keep deleted it");
    click(&mut harness, "Delete");
    click(&mut harness, "Yes, delete");
    assert!(harness.state().rows().is_empty());
}

#[test]
fn the_category_tree_filters_the_one_list() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("stalled.bin"), "Add for later");
    until(&mut harness, "one to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(2);

    harness.get_by_label_contains("Done (1)").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("file.zip").is_some());
    assert!(harness.query_by_label("stalled.bin").is_none());
    harness.get_by_label_contains("In progress (1)").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("stalled.bin").is_some());
    assert!(harness.query_by_label("file.zip").is_none());
    harness.get_by_label_contains("Archives (1)").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("file.zip").is_some());
}

#[test]
fn delete_waits_for_a_download_to_stop() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "the row to start", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    select(&mut harness, "stalled.bin");
    assert!(
        !harness.state().enabled(Command::Remove),
        "Delete is offered for a download still running"
    );
    harness.state_mut().run(Command::Remove);
    assert_eq!(
        harness.state().rows().len(),
        1,
        "a running download was deleted"
    );
}

#[test]
fn properties_keep_the_login_out_of_the_address_box() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add_with(
            &world
                .url("private.bin")
                .replace("http://", "http://"),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    let row = harness.state().rows()[0].clone();
    harness.state_mut().dialogs.open_properties(&row);
    harness.run_steps(3);
    use egui::accesskit::Role;
    let address = harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .value()
        .unwrap_or_default();
    assert!(
        !address.contains("secret"),
        "the password is in the address box: {address}"
    );
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "Login")
            .value()
            .as_deref(),
        Some("me")
    );
    // Changing the password changes it on the download.
    harness
        .get_by_role_and_label(Role::PasswordInput, "Password")
        .focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::PasswordInput, "Password")
        .type_text("better");
    harness.run_steps(2);
    click(&mut harness, "OK");
    let url = harness.state().rows()[0].url.clone();
    assert!(url.contains("me:better@"), "{url}");
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
            .map(dm_desktop::app::name_of)
            .collect()
    };
    assert_eq!(names(&harness), ["a.bin", "b.bin", "c.bin"]);
    assert_eq!(harness.state().sort, Some((Column::Name, true)));
    click(&mut harness, "Name");
    assert_eq!(names(&harness), ["c.bin", "b.bin", "a.bin"]);
}

#[test]
fn properties_change_the_description_and_refresh_an_expired_address() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("gone.bin"), "Start now");
    until(&mut harness, "the missing file to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    select(&mut harness, "gone.bin");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(3);
    assert!(harness.query_by_label("File properties").is_some());
    type_role(&mut harness, "Note", "fixed link");
    use egui::accesskit::Role;
    harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .type_text(&world.url("file.zip"));
    harness.run_steps(2);
    click(&mut harness, "OK");

    let row = harness.state().rows()[0].clone();
    assert_eq!(row.description, "fixed link");
    assert!(row.url.ends_with("/file.zip"), "{}", row.url);
    // Not yet named by a server, the row goes by its new address's name.
    select(&mut harness, "file.zip");
    click(&mut harness, "Resume");
    until(&mut harness, "the refreshed download to complete", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
}

#[test]
fn a_column_turned_off_is_gone_and_comes_back() {
    // Turning one off is saved in the settings file other tests read.
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
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
        [Column::Name, Column::Queue, Column::Status, Column::Size]
    );
    // Put back for the other tests.
    let mut wanted = saved;
    wanted.column_order.clear();
    wanted.columns_hidden.clear();
    harness.state_mut().apply_settings(wanted).unwrap();
}

#[test]
fn help_opens_the_documentation() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::Help);
    let opened = world.fake.opened.lock().unwrap().clone();
    assert_eq!(opened, [dm_desktop::update::HELP]);
}

#[test]
fn a_batch_adds_every_address_the_star_makes() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(2);
    type_into(&mut harness, "Address with *", &world.url("part*.bin"));
    type_into(&mut harness, "From", "1");
    type_into(&mut harness, "To", "12");
    assert!(harness.query_by_label_contains("12 addresses").is_some());
    click(&mut harness, "Add for later");
    let urls: Vec<String> = harness
        .state()
        .rows()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(urls.len(), 12);
    assert!(urls[0].ends_with("/part1.bin") && urls[11].ends_with("/part12.bin"));
}

#[test]
fn the_site_file_finder_finds_the_files_a_page_links_to() {
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Site files");
    type_into(&mut harness, "Start page", &world.url("page.html"));
    let types = harness.get_by_label("File types");
    types.focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label("File types").type_text("zip pdf");
    harness.run_steps(1);
    click(&mut harness, "Find files");
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.query_by_label_contains("2 files found").is_none() {
        assert!(
            Instant::now() < deadline,
            "the site file finder found nothing"
        );
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(25));
    }
    // The window grew to hold the list and, centred, moved: let it settle
    // before clicking where its button now is.
    harness.run_steps(3);
    click(&mut harness, "Add for later");
    let urls: Vec<String> = harness
        .state()
        .rows()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(urls.len(), 2, "{urls:?}");
    assert!(urls.iter().any(|u| u.ends_with("/file.zip")));
    assert!(urls.iter().any(|u| u.ends_with("/other.pdf")));
}

#[test]
fn a_site_search_reads_pages_deep_and_a_saved_one_runs_on_its_own() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Site files");
    type_into(&mut harness, "Start page", &world.url("page.html"));
    assert!(harness
        .query_by_label("Pages deep (0 for the start page only)")
        .is_some());
    if let Some(d) = harness.state_mut().dialogs.site_files.as_mut() {
        d.depth = 1;
        d.types = "zip pdf".into();
    }
    harness.run_steps(1);
    click(&mut harness, "Find files");
    // page.html and next.html on this site; the page elsewhere is not read.
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness
        .query_by_label_contains("3 files found on 2 pages")
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "the search did not go a page deep"
        );
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(25));
    }

    // The window grew to hold the list and, centred, moved: let it settle
    // before clicking where its fields now are.
    harness.run_steps(3);

    // Saved to run every hour: it runs now, and what it finds is added for
    // later; run again by hand it adds nothing twice.
    type_into(&mut harness, "Save this search as", "Nightly");
    if let Some(d) = harness.state_mut().dialogs.site_files.as_mut() {
        d.every_hours = 1;
    }
    harness.run_steps(1);
    click(&mut harness, "Save search");
    let saved = harness.state().backend().settings().site_searches;
    assert_eq!(saved.len(), 1);
    assert_eq!(
        (saved[0].name.as_str(), saved[0].depth, saved[0].every_hours),
        ("Nightly", 1, 1)
    );
    assert_eq!(saved[0].types, ["zip", "pdf"]);
    assert!(harness.query_by_label_contains("Nightly").is_some());
    until(&mut harness, "the saved search's files", |app| {
        app.rows().len() == 3
    });
    assert!(harness
        .state()
        .rows()
        .iter()
        .all(|r| r.state == State::Paused && r.queue == dm_core::MAIN_QUEUE));
    assert!(harness.state().backend().settings().site_searches[0].last_run > 0);

    click(&mut harness, "Run");
    until(&mut harness, "the search to come back", |app| {
        app.searches_running == 0
    });
    harness.run_steps(2);
    assert_eq!(harness.state().rows().len(), 3, "found twice, added twice");
    // The list emptied: what the search found before is still not new.
    let ids: Vec<_> = harness.state().rows().iter().map(|r| r.id).collect();
    for id in ids {
        harness.state().backend().remove(id);
    }
    harness.state_mut().refresh();
    assert!(harness.state().rows().is_empty());
    click(&mut harness, "Run");
    until(&mut harness, "the search to come back again", |app| {
        app.searches_running == 0
    });
    harness.run_steps(2);
    assert!(
        harness.state().rows().is_empty(),
        "found again after the list was cleared"
    );
    assert!(harness
        .query_by_label_contains("found nothing new")
        .is_some());

    click(&mut harness, "Delete search");
    assert!(harness
        .state()
        .backend()
        .settings()
        .site_searches
        .is_empty());
}

#[test]
fn export_and_import_carry_the_list_as_a_dmlist_file() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                referrer: "https://page.example/".into(),
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    let file = world.dir.path().join("list.dmlist");
    *world.fake.chosen.lock().unwrap() = Some(file.clone());
    harness.state_mut().run(Command::Export);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("\"referrer\": \"https://page.example/\""),
        "{text}"
    );

    let other = world.harness();
    let mut other = other;
    other.state_mut().run(Command::Import);
    let rows = other.state().rows().to_vec();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].referrer, "https://page.example/");
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
fn options_are_put_into_force_on_ok_and_written_down() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    click(&mut harness, "Sort finished files into folders by kind");
    click(&mut harness, "OK");
    assert!(harness.state().backend().settings().categorize);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
    assert_eq!(saved["categorize"], true);
}

#[test]
fn a_category_made_in_options_takes_its_files_into_its_own_folder() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    click(&mut harness, "Sort finished files into folders by kind");
    click(&mut harness, "Add a category");
    type_into(&mut harness, "Category", "Lectures");
    type_into(&mut harness, "Types", "zip");
    type_into(&mut harness, "Sites", "127.0.0.1");
    harness.run_steps(2);
    click(&mut harness, "OK");
    let made = harness.state().backend().settings().custom_categories;
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].extensions, ["zip"]);

    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    assert!(
        std::path::Path::new(&row.path)
            .parent()
            .unwrap()
            .ends_with("Lectures"),
        "{} is not in the category's folder",
        row.path
    );
    assert_eq!(std::fs::read(&row.path).unwrap(), payload());
    harness.run_steps(2);
    click(&mut harness, "Lectures (1)");
    assert_eq!(harness.state().visible().len(), 1);

    // Deleted, it takes nothing more.
    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    click(&mut harness, "Delete category");
    click(&mut harness, "OK");
    assert!(harness
        .state()
        .backend()
        .settings()
        .custom_categories
        .is_empty());
}

#[test]
fn the_toolbar_is_customized_from_the_view_menu_and_remembered() {
    use egui::accesskit::Role;
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
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
fn the_file_types_the_browser_hands_over_are_set_in_options() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Browser")
        .click();
    harness.run_steps(3);
    use egui::accesskit::Role;
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "File types")
        .focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "File types")
        .type_text("ZIP .iso, mkv");
    harness.run_steps(2);
    type_into(
        &mut harness,
        "Never take downloads from these sites",
        "intranet.example",
    );
    click(&mut harness, "OK");
    let catch = harness.state().backend().settings().catch;
    assert_eq!(catch.extensions, ["zip", "iso", "mkv"]);
    assert_eq!(catch.skip_hosts, ["intranet.example"]);
    // What the extension is sent is what was typed.
    let rules = dm_protocol::CatchRules { ..catch };
    assert!(rules.enabled);
}

#[test]
fn site_limits_attempts_and_the_timeout_are_set_in_options_and_reach_the_queue() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Network");
    assert!(harness
        .query_by_label("Attempts per connection (0 to keep trying)")
        .is_some());
    assert!(harness
        .query_by_label("Give up on a connection that sends nothing for, seconds")
        .is_some());
    click(&mut harness, "Add a site limit");
    type_into(&mut harness, "Site", "Slow.Example");
    // The numbers are set on the draft, as dragging them would.
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.attempts = 0;
        d.draft.read_timeout = 10;
        if let Some(limit) = d.draft.host_limits.first_mut() {
            limit.connections = 2;
        }
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    let settings = harness.state().backend().settings();
    assert_eq!((settings.attempts, settings.read_timeout), (0, 10));
    assert_eq!(
        settings.host_limits,
        [dm_core::HostLimit {
            host: "slow.example".into(),
            connections: 2
        }]
    );
    // In force on the queue, not only written down.
    let options = world
        .runtime
        .block_on(harness.state().backend().queue.options());
    assert_eq!(options.attempts, 0);
    assert_eq!(options.host_limits.len(), 1);
    assert_eq!(
        options.connections_for("https://dl.slow.example/f"),
        1,
        "these tests run on one connection, and a site's limit never raises that"
    );
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
    assert_eq!(saved["read_timeout"], 10);
    assert_eq!(saved["host_limits"][0]["host"], "slow.example");
    // The limit can be taken off again.
    click(&mut harness, "Options");
    click(&mut harness, "Network");
    click(&mut harness, "Delete site limit");
    click(&mut harness, "OK");
    assert!(harness.state().backend().settings().host_limits.is_empty());
}

#[test]
fn the_proxys_bypass_list_and_the_recheck_timer_are_set_in_options() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Proxy");
    click(&mut harness, "Connect through a proxy");
    type_into(&mut harness, "Host", "proxy.example");
    type_into(
        &mut harness,
        "Not for these sites (fetched directly)",
        "Intranet.example 10.0.0.5",
    );
    // The recheck timer lives on the Downloads tab; the draft is one
    // across the tabs, so it is set here as dragging it there would.
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.recheck_hours = 6;
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    let settings = harness.state().backend().settings();
    assert!(settings.proxy.enabled);
    assert_eq!(settings.proxy.host, "proxy.example");
    assert_eq!(settings.proxy.bypass, ["intranet.example", "10.0.0.5"]);
    assert_eq!(settings.recheck_hours, 6);
    let options = world
        .runtime
        .block_on(harness.state().backend().queue.options());
    assert_eq!(options.proxy_bypass, ["intranet.example", "10.0.0.5"]);
    assert_eq!(options.proxy_for("http://files.intranet.example/a"), None);
    assert!(options.proxy_for("http://outside.example/a").is_some());
    // Put back, so the other tests' downloads do not go looking for it.
    click(&mut harness, "Options");
    click(&mut harness, "Proxy");
    click(&mut harness, "Connect through a proxy");
    click(&mut harness, "OK");
    assert!(!harness.state().backend().settings().proxy.enabled);
}

#[test]
fn an_address_already_in_the_list_is_asked_about_before_it_is_added_again() {
    let world = world();
    let mut harness = world.harness();
    let url = world.url("file.zip");
    add_url(&mut harness, &url, "Add for later");
    let id = harness.state().rows()[0].id;

    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    assert!(
        harness.query_by_label("Already in the list").is_some(),
        "a second row was made without asking"
    );
    assert!(harness.query_by_label("Download details").is_none());
    click(&mut harness, "Show it");
    assert!(harness.query_by_label("Already in the list").is_none());
    assert!(
        harness.state().selected.contains(&id),
        "the row was not shown"
    );
    assert_eq!(harness.state().rows().len(), 1);

    // Asked again, and this time wanted again: the usual dialog follows.
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    click(&mut harness, "Download again");
    assert!(harness.query_by_label("Download details").is_some());
    click(&mut harness, "Add for later");
    assert_eq!(harness.state().rows().len(), 2);
}

#[test]
fn a_failed_download_plays_the_failure_sound_when_asked() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    let mut wanted = harness.state().backend().settings();
    wanted.sound_on_failure = true;
    harness.state_mut().apply_settings(wanted).unwrap();
    // Nothing is served at this name: a 404, failed and not retried.
    add_url(&mut harness, &world.url("missing.bin"), "Start now");
    until(&mut harness, "the row failed", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(3);
    let opened = world.fake.opened.lock().unwrap().clone();
    assert!(opened.iter().any(|o| o == "alert"), "{opened:?}");
    assert!(!opened.iter().any(|o| o == "chime"), "{opened:?}");
    // Once: a redraw does not sound it again.
    harness.run_steps(5);
    assert_eq!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter(|o| *o == "alert")
            .count(),
        1
    );
    let mut wanted = harness.state().backend().settings();
    wanted.sound_on_failure = false;
    harness.state_mut().apply_settings(wanted).unwrap();
}

#[test]
fn escape_closes_the_menu_or_the_dialog_in_front_and_nothing_behind_it() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");

    // A menu over the dialog: Escape closes the menu and the dialog stays.
    harness.get_by_label("View").click();
    harness.run_steps(3);
    assert!(harness.ctx.any_popup_open(), "the menu did not open");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(!harness.ctx.any_popup_open());
    assert!(
        harness.state().dialogs.options.is_some(),
        "the Escape that closed the menu closed Options too"
    );

    // A combo box dropped down in the dialog, the same.
    click(&mut harness, "Proxy");
    click(&mut harness, "Connect through a proxy");
    harness
        .get_by_role_and_label(Role::ComboBox, "Proxy type")
        .click();
    harness.run_steps(3);
    assert!(harness.ctx.any_popup_open(), "the combo box did not open");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(!harness.ctx.any_popup_open());
    assert!(
        harness.state().dialogs.options.is_some(),
        "the Escape that put the combo box away closed Options"
    );
    // Typing in a field: the same Escape leaves the field and closes.
    type_into(&mut harness, "Host", "proxy.example");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.options.is_none(),
        "Escape did not close Options"
    );
    assert!(
        !harness.state().backend().settings().proxy.enabled,
        "Escape kept what was typed"
    );

    // A dialog over a dialog: Find over Download details. Escape closes
    // Find; Download details, and what was typed into it, stay.
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    type_role(&mut harness, "Note", "keep me");
    harness.state_mut().run(Command::Find);
    harness.run_steps(2);
    assert!(harness.state().dialogs.find_open);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(!harness.state().dialogs.find_open, "Find stayed open");
    let details = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .expect("Download details was closed under Find");
    assert_eq!(details.description, "keep me");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().dialogs.file_info.is_none());
}

#[test]
fn two_cancelled_hand_offs_from_a_site_offer_to_leave_it_to_the_browser() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    let hand = |harness: &mut Harness<'_, DmApp>, name: &str| {
        harness
            .state()
            .backend()
            .handed
            .lock()
            .unwrap()
            .push_back(dm_shell::host::Request {
                url: world.url(name),
                referrer: world.url("page.html"),
                ask: true,
                ..Default::default()
            });
        harness.state_mut().take_handed();
        harness.run_steps(2);
        assert!(harness.query_by_label("Download details").is_some());
    };
    // One cancelled: nothing asked. A typed-in one cancelled: nothing either.
    hand(&mut harness, "file.zip");
    click(&mut harness, "Cancel");
    assert!(harness
        .query_by_label("Leave this site to the browser?")
        .is_none());
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("other.pdf"));
    click(&mut harness, "OK");
    click(&mut harness, "Cancel");
    assert!(harness
        .query_by_label("Leave this site to the browser?")
        .is_none());
    // The second handed one: asked, and "keep" leaves the rules alone.
    hand(&mut harness, "other.pdf");
    click(&mut harness, "Cancel");
    assert!(harness
        .query_by_label("Leave this site to the browser?")
        .is_some());
    click(&mut harness, "Keep handing them over");
    assert!(harness
        .state()
        .backend()
        .settings()
        .catch
        .skip_hosts
        .is_empty());
    // Twice more, and this time left to the browser: the site joins the
    // list the extension reads.
    hand(&mut harness, "file.zip");
    click(&mut harness, "Cancel");
    hand(&mut harness, "file.zip");
    click(&mut harness, "Cancel");
    click(&mut harness, "Leave them to the browser");
    assert_eq!(
        harness.state().backend().settings().catch.skip_hosts,
        ["127.0.0.1"]
    );
    // Put back for the other tests.
    let mut wanted = harness.state().backend().settings();
    wanted.catch.skip_hosts.clear();
    harness.state_mut().apply_settings(wanted).unwrap();
}

#[test]
fn a_queues_own_file_limit_and_the_parts_folder_are_set_in_options() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.tab = dm_desktop::dialogs::Tab::Downloads;
    }
    harness.run_steps(2);
    click(&mut harness, "Add a queue limit");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        if let Some(limit) = d.draft.queue_limits.first_mut() {
            limit.files = 2;
        }
        d.tab = dm_desktop::dialogs::Tab::SaveTo;
    }
    harness.run_steps(2);
    let staged = world.dir.path().join("staging");
    type_into(
        &mut harness,
        "Folder for unfinished downloads (empty for dm's own)",
        &staged.display().to_string(),
    );
    click(&mut harness, "OK");
    let settings = harness.state().backend().settings();
    assert_eq!(
        settings.queue_limits,
        [dm_shell::settings::QueueLimit {
            queue: dm_core::MAIN_QUEUE.into(),
            files: 2
        }]
    );
    assert_eq!(settings.parts_dir(), staged);
    // In force on the queue.
    assert_eq!(
        harness.state().backend().queue.queue_limits(),
        std::collections::HashMap::from([(dm_core::MAIN_QUEUE.to_string(), 2)])
    );
    let options = world
        .runtime
        .block_on(harness.state().backend().queue.options());
    assert_eq!(options.parts_dir, staged);
    // Put back for the other tests.
    let mut wanted = harness.state().backend().settings();
    wanted.queue_limits.clear();
    wanted.parts_dir.clear();
    harness.state_mut().apply_settings(wanted).unwrap();
}

#[test]
fn a_rows_handed_over_sign_in_is_never_read_back_out() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                later: true,
                cookies: "session=SECRET123".into(),
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
    assert!(!text.contains("SECRET123"), "{text}");
    assert_eq!(listed[0]["signed_in"], true, "{text}");
    let seen = harness
        .state_mut()
        .control_call("ui.inspect", &serde_json::json!({}))
        .unwrap();
    assert!(!seen.to_string().contains("SECRET123"));
}

#[test]
fn a_handed_over_videos_sign_in_survives_download_details() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("file.zip"),
            referrer: world.url("page.html"),
            ask: true,
            cookies: "session=1".into(),
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(2);
    click(&mut harness, "Add for later");
    let rows = harness.state().rows().to_vec();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].cookies, "session=1",
        "Download details dropped the sign-in"
    );

    // A page's links: its sign-in goes with the links on its own server only.
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("page.html"),
            referrer: world.url("page.html"),
            links: vec![
                world.url("other.pdf"),
                "http://elsewhere.invalid/x.zip".into(),
            ],
            ask: true,
            cookies: "session=2".into(),
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(2);
    click(&mut harness, "OK");
    harness.state_mut().refresh();
    let rows = harness.state().rows().to_vec();
    let of = |end: &str| {
        rows.iter()
            .find(|r| r.url.ends_with(end))
            .map(|r| r.cookies.clone())
    };
    assert_eq!(of("/other.pdf").as_deref(), Some("session=2"));
    assert_eq!(
        of("/x.zip").as_deref(),
        Some(""),
        "another server was given the page's sign-in"
    );
}

#[test]
fn an_option_that_cannot_be_applied_is_said_and_what_was_typed_is_kept() {
    /* A failed OK used to reopen Options on the settings in force, which
     * threw away every edit in the dialog to say what was wrong with one.
     * The edits stay, with the reason, and OK again once the cause is gone
     * puts them in force. */
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    *world.fake.login_fails.lock().unwrap() = true;
    let mut harness = world.harness();
    click(&mut harness, "Options");
    harness
        .get_by_label_contains("Start dm when you log in")
        .click();
    harness
        .get_by_label_contains("Play a sound when a download finishes")
        .click();
    harness.run_steps(2);
    click(&mut harness, "OK");
    let held = harness.state().backend().settings();
    assert!(!held.start_on_login);
    assert!(
        !held.sound_on_complete,
        "half of the change was put in force"
    );
    assert!(harness
        .query_by_label_contains("could not change start-on-login")
        .is_some());
    for typed in [
        "Start dm when you log in",
        "Play a sound when a download finishes",
    ] {
        assert_eq!(
            harness
                .get_by_label_contains(typed)
                .accesskit_node()
                .toggled(),
            Some(egui::accesskit::Toggled::True),
            "{typed}: the edit was thrown away"
        );
    }

    *world.fake.login_fails.lock().unwrap() = false;
    click(&mut harness, "OK");
    let held = harness.state().backend().settings();
    assert!(held.start_on_login && held.sound_on_complete);
    assert!(harness.state().dialogs.options.is_none());
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
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
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
    click(&mut harness, "Apply");
    assert!(
        harness.state().dialogs.scheduler.is_some(),
        "closed as if taken"
    );
    assert!(harness
        .query_by_label_contains("times are HH:MM in 24 hours")
        .is_some());
    assert!(!harness.state().backend().settings().schedule.enabled);

    // No day ticked is every day, as the dialog says, and is taken.
    if let Some((schedule, _)) = harness.state_mut().dialogs.scheduler.as_mut() {
        schedule.from = "01:00".into();
        schedule.to = "07:00".into();
        schedule.days.clear();
    }
    harness.run_steps(2);
    click(&mut harness, "Apply");
    let held = harness.state().backend().settings().schedule;
    assert!(held.enabled && held.days.is_empty());
    assert!(held.open_at(0, 2 * 60) && held.open_at(3, 2 * 60));
    let mut back = harness.state().backend().settings();
    back.schedule.enabled = false;
    harness.state_mut().apply_settings(back).unwrap();
}

#[test]
fn a_bad_category_options_opened_with_does_not_fail_an_unrelated_ok() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    let bad = dm_core::Custom {
        name: "Work/Docs".into(),
        extensions: vec!["pdf".into()],
        ..Default::default()
    };
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        // As if an older dm had saved it.
        d.opened.custom_categories.push(bad);
        d.custom
            .push(["Work/Docs".into(), "pdf".into(), String::new()]);
        d.draft.sound_on_failure = !d.draft.sound_on_failure;
    }
    let wanted = !harness.state().backend().settings().sound_on_failure;
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.options.is_none(),
        "the OK was refused"
    );
    assert_eq!(
        harness.state().backend().settings().sound_on_failure,
        wanted
    );
    let mut back = harness.state().backend().settings();
    back.sound_on_failure = !wanted;
    harness.state_mut().apply_settings(back).unwrap();
}

#[test]
fn options_refuses_a_category_that_is_not_one_folder_and_a_relative_download_folder() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    let before = harness.state().backend().settings();
    click(&mut harness, "Options");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.custom
            .push(["Work/Docs".into(), "pdf".into(), String::new()]);
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness
        .query_by_label_contains("cannot be a category")
        .is_some());
    assert_eq!(
        harness.state().backend().settings().custom_categories,
        before.custom_categories
    );

    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.custom.clear();
        d.draft.download_dir = "Downloads/dm".into();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness
        .query_by_label_contains("must be a full path")
        .is_some());
    assert_eq!(
        harness.state().backend().settings().download_dir,
        before.download_dir
    );

    // From the home folder is a full path, and is expanded.
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.download_dir = "~/dm-test-downloads".into();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert_eq!(
        harness.state().backend().settings().download_dir,
        std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join("dm-test-downloads")
    );
    let mut back = harness.state().backend().settings();
    back.download_dir = before.download_dir;
    harness.state_mut().apply_settings(back).unwrap();
}

#[test]
fn the_speed_limiter_turns_off_and_back_on_to_its_last_value() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
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
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::SpeedLimiterSettings);
    harness.run_steps(3);
    assert!(harness
        .query_by_label("Download no more than, MB (0 for no limit)")
        .is_some());
    assert!(harness.query_by_label("every, hours").is_some());
    if let Some(limiter) = harness.state_mut().dialogs.limiter.as_mut() {
        limiter.2 = 500;
        limiter.3 = 4;
    }
    harness.get_by_label("Apply").click();
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
    harness.get_by_label("Apply").click();
    harness.run_steps(3);
    assert_eq!(harness.state().backend().settings().limit_mb, 0);
}

#[test]
fn the_weekly_update_check_asks_nothing_when_off_or_not_yet_due() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
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
fn view_language_puts_the_window_in_another_language_and_back() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(2);
    assert!(harness.query_by_label("Program").is_some());
    click(&mut harness, "View");
    harness.get_by_label_contains("Language").click();
    harness.run_steps(3);
    harness.get_by_label("Deutsch").click();
    harness.run_steps(3);
    assert_eq!(harness.state().backend().settings().language, "de");
    assert!(
        harness.query_by_label("Programm").is_some() && harness.query_by_label("Program").is_none(),
        "the menu is not in German"
    );
    assert!(
        harness.query_by_label("Geschwindigkeit").is_some(),
        "nor the list's headings"
    );
    click(&mut harness, "Ansicht");
    harness.get_by_label_contains("Sprache").click();
    harness.run_steps(3);
    harness.get_by_label("English").click();
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Program").is_some(),
        "not back in English"
    );
    assert_eq!(harness.state().backend().settings().language, "en");
}

#[test]
fn the_clipboard_watcher_offers_only_new_links_and_only_when_on() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some(world.url("old.iso"));
    let mut harness = world.harness();
    let clip = |h: &mut Harness<'_, DmApp>, text: &str| {
        *world.fake.clipboard.lock().unwrap() = Some(text.to_string());
        h.state_mut().check_clipboard();
        h.run_steps(1);
        h.state().clipboard_offer().map(str::to_owned)
    };
    assert_eq!(
        clip(&mut harness, &world.url("new.iso")),
        None,
        "offered while off"
    );

    click(&mut harness, "Options");
    click(
        &mut harness,
        "Watch the clipboard for links and offer to download them",
    );
    click(&mut harness, "OK");
    assert!(
        harness.state().backend().settings().watch_clipboard,
        "ticking the box did nothing"
    );
    harness.state_mut().check_clipboard();
    assert_eq!(
        harness.state().clipboard_offer(),
        None,
        "offered what was there before"
    );

    assert_eq!(clip(&mut harness, "just some words"), None);
    let fresh = world.url("fresh.iso");
    assert_eq!(clip(&mut harness, &fresh).as_deref(), Some(fresh.as_str()));
    click(&mut harness, "Dismiss");
    clip(&mut harness, "elsewhere");
    assert_eq!(
        clip(&mut harness, &fresh),
        None,
        "a dismissed link came back"
    );
}

#[test]
fn a_progress_window_shows_each_connection() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_progress = true;
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    // Running comes a moment before the plan is written; until then the
    // window says "Unknown until the server answers", which is right.
    until(&mut harness, "the download to be planned", |app| {
        app.rows()
            .iter()
            .any(|r| r.state == State::Running && app.backend().resumable(r.id).is_some())
    });
    harness.run_steps(3);
    assert!(harness
        .query_by_label_contains("Download progress")
        .is_some());
    assert!(harness.query_by_label("Can resume").is_some());
    // That server does not serve ranges, and the window says so rather than
    // guessing "Yes" from a known size.
    assert!(harness
        .query_by_label_contains("No — a stop starts it again")
        .is_some());
    assert!(harness.query_by_label("Connections").is_some());
    click(&mut harness, "Pause");
    assert_eq!(harness.state().rows()[0].state, State::Paused);
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
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
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

#[test]
fn five_hundred_rows_draw_inside_a_frame_budget() {
    let world = world();
    let mut harness = world.harness();
    let links: Vec<String> = (0..500)
        .map(|i| world.url(&format!("stalled-{i}.bin")))
        .collect();
    harness.state().backend().pause_all();
    harness.state().backend().add(&links.join(" ")).unwrap();
    harness.state().backend().pause_all();
    harness.state_mut().refresh();
    harness.run_steps(2);

    let started = Instant::now();
    for _ in 0..10 {
        harness.step();
    }
    let per_frame = started.elapsed() / 10;
    println!("500 rows: {per_frame:?} a frame");
    assert_eq!(harness.state().rows().len(), 500);
    assert!(
        per_frame < Duration::from_millis(100),
        "{per_frame:?} a frame"
    );
}

/// The control protocol, over a real TCP connection, against the window:
/// what an agent or a test outside the process uses to drive dm.
#[test]
fn the_control_protocol_runs_commands_and_reads_the_window_back() {
    use std::io::{BufRead, BufReader, Write};
    let world = world();
    let mut harness = world.harness();
    let (to_window, requests) = std::sync::mpsc::channel();
    harness.state_mut().attach_control(requests);
    let token = "f".repeat(64);
    let port = dm_desktop::control::listen(0, token.clone(), to_window, || {}).unwrap();
    let url = world.url("file.zip");

    let client = std::thread::spawn(move || {
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut out = stream.try_clone().unwrap();
        let mut replies = BufReader::new(stream);
        let mut call = |method: &str, params: serde_json::Value| -> serde_json::Value {
            let line = serde_json::json!({ "id": 1, "method": method, "params": params });
            out.write_all(format!("{line}\n").as_bytes()).unwrap();
            let mut reply = String::new();
            replies.read_line(&mut reply).unwrap();
            serde_json::from_str(&reply).unwrap()
        };
        let auth = call("auth", serde_json::json!({ "token": token }));
        assert_eq!(auth["ok"], true, "{auth}");
        let commands = call("commands.list", serde_json::json!({}));
        let ids: Vec<&str> = commands["result"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["id"].as_str())
            .collect();
        assert!(
            ids.contains(&"program.add_url") && ids.contains(&"downloads.start_queue"),
            "{ids:?}"
        );
        let refused = call("command.run", serde_json::json!({ "id": "file.remove" }));
        assert_eq!(
            refused["ok"], false,
            "removing nothing was allowed: {refused}"
        );
        let opened = call(
            "command.run",
            serde_json::json!({ "id": "program.add_url" }),
        );
        assert_eq!(opened["ok"], true, "{opened}");
        let seen = call("ui.inspect", serde_json::json!({}));
        let dialogs = seen["result"]["dialogs"].as_array().unwrap().clone();
        assert!(dialogs.iter().any(|d| d == "Add URL"), "{dialogs:?}");
        let unknown = call("nonsense", serde_json::json!({}));
        assert_eq!(unknown["ok"], false);
        url
    });

    // The window answers on its own thread, as frames are drawn.
    while !client.is_finished() {
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    client.join().expect("the client's checks failed");
    assert!(
        harness.state().dialogs.add_url.is_some(),
        "the command never reached the window"
    );
}

/// The MCP bridge, as an AI client would use it: tools listed, a download
/// added and listed back, a wrong tool refused — through a real control
/// connection to the window.
#[test]
fn the_mcp_bridge_gives_an_agent_dm_as_tools() {
    let world = world();
    let mut harness = world.harness();
    let (to_window, requests) = std::sync::mpsc::channel();
    harness.state_mut().attach_control(requests);
    let token = "e".repeat(64);
    let port = dm_desktop::control::listen(0, token.clone(), to_window, || {}).unwrap();
    let url = world.url("file.zip");

    let client = std::thread::spawn(move || {
        let mut control = dm_desktop::mcp::Control::connect(port, &token).unwrap();
        let script = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#.to_string(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_string(),
            format!(r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"dm_add_downloads","arguments":{{"urls":["{url}"],"later":true}}}}}}"#),
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"dm_list_downloads","arguments":{}}}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"dm_nonsense"}}"#.to_string(),
            "not json".to_string(),
        ]
        .join("\n");
        let mut out = Vec::new();
        dm_desktop::mcp::serve(std::io::Cursor::new(script), &mut out, &mut control).unwrap();
        String::from_utf8(out).unwrap()
    });
    while !client.is_finished() {
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = client.join().unwrap();
    let replies: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        replies.len(),
        6,
        "a notification was answered, or a request was not: {out}"
    );
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "dm");
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "dm_add_downloads"));
    assert_eq!(
        replies[2]["result"]["isError"],
        serde_json::Value::Null,
        "{}",
        replies[2]
    );
    assert!(replies[3]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("file.zip"));
    assert_eq!(replies[4]["error"]["code"], -32602);
    assert_eq!(replies[5]["error"]["code"], -32700);
    assert_eq!(
        harness.state().rows().len(),
        1,
        "the agent's download is not in the window"
    );
}

/// An empty list of links was answered "added 0" as a success, and any
/// `queue:` name — one never made, or 100 KB of one — was taken as a place.
#[test]
fn the_control_protocol_refuses_nothing_to_add_and_a_queue_that_is_not_there() {
    let world = world();
    let mut harness = world.harness();
    let app = harness.state_mut();
    for empty in [
        serde_json::json!({ "urls": [] }),
        serde_json::json!({ "urls": [""] }),
        serde_json::json!({ "urls": "" }),
    ] {
        assert!(
            app.control_call("downloads.add", &empty).is_err(),
            "{empty} was added"
        );
    }
    for nowhere in [
        "queue:nowhere".to_string(),
        format!("queue:{}", "q".repeat(100_000)),
    ] {
        assert!(app
            .control_call("ui.place", &serde_json::json!({ "place": nowhere }))
            .is_err());
    }
    assert!(app
        .control_call(
            "ui.place",
            &serde_json::json!({ "place": "queue:Main queue" })
        )
        .is_ok());
}

/// Opening a file runs it if it is a program: never at a script's word.
#[test]
fn opening_a_file_is_refused_over_the_control_protocol() {
    let world = world();
    let mut harness = world.harness();
    let app = harness.state_mut();
    for id in ["file.open", "file.open_with"] {
        let refused = app
            .control_call("command.run", &serde_json::json!({ "id": id }))
            .expect_err(id);
        assert!(refused.contains("by a person"), "{refused}");
    }
}

#[test]
fn a_batch_runs_its_steps_in_order() {
    let world = world();
    let mut harness = world.harness();
    let steps = serde_json::json!({ "steps": [
        { "method": "downloads.add", "params": { "urls": [world.url("a.bin"), world.url("b.bin")], "later": true } },
        { "method": "ui.place", "params": { "place": "queue:Main queue" } },
        { "method": "nonsense" },
        { "method": "ui.inspect" },
    ]});
    let results = harness.state_mut().control_call("batch", &steps).unwrap();
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0]["result"]["added"], 2);
    assert_eq!(results[2]["ok"], false);
    assert_eq!(results[3]["result"]["rows"].as_array().unwrap().len(), 2);
    let too_many =
        serde_json::json!({ "steps": vec![serde_json::json!({"method": "ui.inspect"}); 257] });
    assert!(harness
        .state_mut()
        .control_call("batch", &too_many)
        .is_err());
}

/// The pictures in the README, rendered offscreen from a queue the engine
/// really served — the sizes, rates and times are its, not a mock-up's.
///
///     DM_SHOTS=docs cargo test -p dm-desktop --test window shots
///
/// Writes screenshot.png, screenshot-dark.png and settings.png there.
#[test]
fn shots() {
    let Some(dir) = std::env::var_os("DM_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "a finished row", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    add_url(&mut harness, &world.url("gone.iso"), "Start now");
    add_url(&mut harness, &world.url("later.mkv"), "Add for later");
    until(&mut harness, "a failed row", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    select(&mut harness, "stalled.bin");
    harness.run_steps(4);

    // dm's own looks, chosen as Options › Look chooses them: Light
    // and Dark. A few frames let the embedded font come into force.
    let render = |harness: &mut Harness<'_, DmApp>, look: &str, name: &str| {
        harness.state_mut().applied.theme = look.into();
        // No test pointer over the picture, and a download folder a reader
        // would recognise rather than the test's temporary one.
        harness.event(egui::Event::PointerGone);
        harness.state_mut().applied.download_dir = "~/Downloads".into();
        harness.run_steps(6);
        let image = harness.render().expect("the window could not be rendered");
        image.save(dir.join(name)).unwrap();
    };
    render(&mut harness, "light", "screenshot.png");
    render(&mut harness, "dark", "screenshot-dark.png");
    let id = harness.state().targets()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    render(&mut harness, "light", "progress.png");
    render(&mut harness, "dark", "progress-dark.png");
    harness.state_mut().dialogs.progress.clear();
    let settings = harness.state().applied.clone();
    harness.state_mut().dialogs.open_options(&settings);
    render(&mut harness, "light", "settings.png");
    render(&mut harness, "dark", "settings-dark.png");
}

#[cfg(unix)]
#[test]
fn a_finished_download_is_handed_to_the_command_set_to_run_after() {
    /* A command run on each finished file, its path added as one argument,
     * whatever characters the name has. */
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

    let mut harness = world.harness();
    harness.state_mut().applied.on_complete = format!("'{}' --scan", script.display());
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let path = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.state == State::Completed)
        .map(|r| r.path.clone())
        .unwrap();
    let ran = (0..100).find_map(|_| {
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::read_to_string(&record).ok()
    });
    assert_eq!(
        ran.as_deref(),
        Some(format!("--scan|{path}").as_str()),
        "the command was not run on the file"
    );
}

/* ------------------------------------------------- every click, checked */

/// Opens a menu path by clicking each name on screen, as a person does:
/// the top menu by its exact name, a submenu by the name its arrow follows,
/// the item by its exact name. The last match is the one in the open menu,
/// above the toolbar button that may share its name. Closes what is left.
fn press(h: &mut Harness<'_, DmApp>, path: &[&str]) {
    // What the test closed by hand is gone from the screen first.
    h.run_steps(2);
    for (i, name) in path.iter().enumerate() {
        let found = if i == 0 {
            h.query_all_by_label(name).last()
        } else if i + 1 == path.len() {
            // A menu item with a key says it after its name, "Help F1"; a
            // key never starts lowercase, so "Find" is not "Find next".
            h.query_all_by_label_contains(name).rfind(|n| {
                let label = n.accesskit_node().label().unwrap_or_default();
                label == *name
                    || label
                        .strip_prefix(&format!("{name} "))
                        .is_some_and(|key| !key.starts_with(|c: char| c.is_lowercase()))
            })
        } else {
            // A submenu's name may carry its arrow. Text on screen that
            // merely mentions the name ("Speed limiter on: 100 KB/s." in
            // the status bar) is not the submenu.
            h.query_all_by_label_contains(name)
                .rfind(|n| n.accesskit_node().role() != egui::accesskit::Role::Label)
        };
        found
            .unwrap_or_else(|| panic!("{name:?} is not on screen (clicking {path:?})"))
            .click();
        h.run_steps(3);
    }
    // A menu still open (a leaf that is a submenu) is closed; Escape with
    // none open would close the dialog the item just opened, as it does
    // for a person.
    if h.ctx.any_popup_open() {
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
    }
}

/// Whether a menu item is shown greyed out, as one with nothing to act on.
fn greyed(h: &mut Harness<'_, DmApp>, path: &[&str]) -> bool {
    h.run_steps(2);
    for (i, name) in path[..path.len() - 1].iter().enumerate() {
        let menu = if i == 0 {
            h.query_all_by_label(name).last()
        } else {
            h.query_all_by_label_contains(name)
                .rfind(|n| n.accesskit_node().role() != egui::accesskit::Role::Label)
        };
        menu.unwrap_or_else(|| panic!("{name:?} is not on screen"))
            .click();
        h.run_steps(3);
    }
    let leaf = path[path.len() - 1];
    let disabled = h
        .query_all_by_label_contains(leaf)
        .last()
        .unwrap_or_else(|| panic!("{leaf:?} is not on screen"))
        .accesskit_node()
        .is_disabled();
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    disabled
}

fn row(h: &Harness<'_, DmApp>, name: &str) -> dm_core::Entry {
    h.state()
        .rows()
        .iter()
        .find(|r| dm_desktop::app::name_of(r) == name)
        .unwrap_or_else(|| panic!("no row {name}"))
        .clone()
}

fn is_there(h: &Harness<'_, DmApp>, name: &str) -> bool {
    h.state()
        .rows()
        .iter()
        .any(|r| dm_desktop::app::name_of(r) == name)
}

/// Every menu item, clicked by its name on screen, does what it says: a
/// dialog opens, a row changes state, a file opens, a setting changes.
/// "Check for updates" is found and enabled but not clicked: it asks GitHub,
/// and a test makes no request off this computer.
#[test]
fn every_menu_item_clicked_does_what_it_says() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
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
    press(&mut h, &["Program", "Find files on a site"]);
    assert!(
        h.state().dialogs.site_files.is_some(),
        "Program › Find files on a site"
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
    press(
        &mut h,
        &["Downloads", "Speed limiter", "Turn speed limiter on or off"],
    );
    assert_ne!(
        h.state().backend().settings().speed_limit,
        limit,
        "Turn speed limiter on or off"
    );
    press(
        &mut h,
        &["Downloads", "Speed limiter", "Turn speed limiter on or off"],
    );
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
    // Greyed where the computer has no such command (macOS).
    if dm_desktop::power::command(dm_desktop::power::WhenDone::Hibernate, std::env::consts::OS)
        .is_some()
    {
        when_done.insert(2, ("Hibernate", dm_desktop::power::WhenDone::Hibernate));
    }
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
        h.state().said.english().starts_with("Started"),
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
    press(&mut h, &["Downloads", "Clear all finished"]);
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
    click(&mut h, "Yes, delete");
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
        h.state().said.english().starts_with("Started"),
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
    tool(&mut h, "Delete");
    assert!(
        h.state().dialogs.confirm_remove.is_some(),
        "Delete asks first for an unfinished one"
    );
    click(&mut h, "Yes, delete");
    until(&mut h, "Delete", |app| {
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
    click(&mut h, "Yes, delete");
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
fn a_dialog_asked_for_while_open_keeps_what_was_typed_in_it() {
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", "https://e.com/typed.zip");
    harness.state_mut().run(Command::AddUrl);
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .add_url
            .as_ref()
            .map(|d| d.address.as_str()),
        Some("https://e.com/typed.zip"),
        "Add URL asked for again threw away the address"
    );
    harness.state_mut().run(Command::Options);
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .connections = 3;
    harness.state_mut().run(Command::Options);
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .options
            .as_ref()
            .map(|o| o.draft.connections),
        Some(3),
        "Options asked for again lost its unsaved change"
    );
    for (command, open) in [
        (Command::Scheduler, "Scheduler"),
        (Command::AddSeries, "Add numbered series"),
        (Command::SiteFiles, "Files on a site"),
        (Command::SpeedLimiterSettings, "Speed limiter"),
    ] {
        harness.state_mut().run(command);
        harness.state_mut().run(command);
        assert!(harness
            .state()
            .dialogs
            .open_names()
            .iter()
            .any(|n| n == open));
    }
}

#[test]
fn the_lists_keys_do_not_reach_it_from_behind_a_dialog() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    select(&mut harness, "file.zip");
    harness.state_mut().run(Command::Options);
    harness.run_steps(2);
    harness.key_press(egui::Key::Delete);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert_eq!(
        harness.state().rows().len(),
        1,
        "Delete behind Options removed it"
    );
    assert!(
        harness.state().dialogs.progress.is_empty(),
        "Enter reached the list"
    );
    assert!(harness.state().dialogs.confirm_remove.is_none());
    // With the dialog gone, Delete is the list's again.
    harness.state_mut().dialogs.options = None;
    harness.run_steps(2);
    harness.key_press(egui::Key::Delete);
    until(&mut harness, "Delete to act on the list", |app| {
        app.rows().is_empty() || app.dialogs.confirm_remove.is_some()
    });
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

#[test]
fn the_speed_limit_last_turned_off_comes_back_after_a_restart() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
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

#[test]
fn properties_fit_a_small_window_and_close_from_the_title_bar() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    let long = format!("https://example.com/{}", "very-long-page-name/".repeat(30));
    harness
        .state_mut()
        .add(
            &[world.url("file.zip")],
            dm_core::AddOptions {
                referrer: long,
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    select(&mut harness, "file.zip");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(4);
    let ok = harness.get_by_label("OK").rect();
    assert!(
        ok.max.x <= 720.0 && ok.max.y <= 460.0,
        "OK is off the window: {ok:?}"
    );
    harness.get_by_label("Close window").click();
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.properties.is_empty(),
        "no way to close it"
    );
}

#[test]
fn f3_finds_the_next_one_while_find_is_open() {
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
    let picked = |h: &Harness<'_, DmApp>| {
        h.state()
            .targets()
            .iter()
            .map(dm_desktop::app::name_of)
            .collect::<Vec<_>>()
    };
    // From Find's own field.
    harness.key_press(egui::Key::F3);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alpha.bin"], "F3 in the field");
    // Enter finds too, and leaves the field: F3 still finds behind Find.
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alphabet.bin"]);
    assert!(
        !harness.ctx.egui_wants_keyboard_input(),
        "the premise: the field let go"
    );
    harness.key_press(egui::Key::F3);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alpha.bin"], "F3 with Find open");
    assert!(harness.state().dialogs.find_open);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(
        !harness.state().dialogs.find_open,
        "Escape still closes Find"
    );
}

#[test]
fn options_holds_only_new_or_renamed_folders_to_every_computers_rules() {
    let _file = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    // Saved before by a dm that took them, and usable on this computer: a
    // kind's folder and a category with a colon in it.
    let mut saved = Settings {
        download_dir: world.dir.path().join("out"),
        connections: 1,
        show_progress: false,
        show_complete: false,
        language: "en".into(),
        ..Settings::default()
    };
    saved.category_folders[1] = "Audio: 2024".into();
    saved.custom_categories.push(dm_core::Custom {
        name: "Live: 2024".into(),
        extensions: vec!["mkv".into()],
        sites: Vec::new(),
    });
    let before = saved.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app_with(saved));
    // An unrelated change goes through.
    click(&mut harness, "Options");
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .connections = 4;
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.options.is_none(),
        "a name saved before blocked OK"
    );
    let now = harness.state().backend().settings();
    assert_eq!(now.connections, 4);
    assert_eq!(
        now.category_folders[1], "Audio: 2024",
        "a saved folder kept"
    );
    assert!(now.custom_categories.iter().any(|c| c.name == "Live: 2024"));
    // A kind's folder renamed to one Windows cannot make is refused.
    click(&mut harness, "Options");
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .category_folders[0] = "Clips|new".into();
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness
            .query_by_label_contains("not a folder name every computer can use")
            .is_some(),
        "{:?} {:?}",
        harness
            .state()
            .dialogs
            .options
            .as_ref()
            .map(|d| d.says.clone()),
        harness.state().backend().settings().category_folders
    );
    assert_eq!(
        harness.state().backend().settings().category_folders[0],
        before.category_folders[0]
    );
    // And a new category.
    {
        let d = harness.state_mut().dialogs.options.as_mut().unwrap();
        d.draft.category_folders[0] = before.category_folders[0].clone();
        d.custom
            .push(["Notes?".into(), "txt".into(), String::new()]);
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.options.is_some(),
        "a new bad name was taken"
    );
    harness.state_mut().dialogs.options = None;
}
