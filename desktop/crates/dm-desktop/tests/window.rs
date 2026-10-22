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

thread_local! {
    /// Whether this test's thread holds [`SETTINGS_FILE`].
    static HOLDS_SETTINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// [`SETTINGS_FILE`], held until dropped, and known to be held on this
/// thread, so a helper that writes the file too can take it only when the
/// test has not: Add URL saves its recent addresses there, and a test
/// reading the file back read another's (gap 51).
struct SettingsFile(Option<std::sync::MutexGuard<'static, ()>>);

impl Drop for SettingsFile {
    fn drop(&mut self) {
        if self.0.take().is_some() {
            HOLDS_SETTINGS.with(|h| h.set(false));
        }
    }
}

fn settings_file() -> SettingsFile {
    if HOLDS_SETTINGS.with(std::cell::Cell::get) {
        return SettingsFile(None);
    }
    let guard = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    HOLDS_SETTINGS.with(|h| h.set(true));
    SettingsFile(Some(guard))
}

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
    /// A desktop with no folder chooser to ask with.
    no_chooser: Mutex<bool>,
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
    fn choose_folder(&self) -> Option<std::path::PathBuf> {
        self.chosen.lock().unwrap().clone()
    }
    fn can_choose_folder(&self) -> bool {
        !*self.no_chooser.lock().unwrap()
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
                "/secret.zip",
                axum::routing::get(|| async {
                    Response::builder()
                        .status(401)
                        .body(Body::from("sign in first"))
                        .unwrap()
                }),
            )
            .route(
                "/locked.zip",
                axum::routing::get(|headers: axum::http::HeaderMap| async move {
                    // Only with a sign-in, as a server behind a login is.
                    if headers.contains_key("authorization") {
                        Response::new(Body::from(payload()))
                    } else {
                        Response::builder()
                            .status(401)
                            .header("www-authenticate", "Basic realm=\"dm\"")
                            .body(Body::from("sign in first"))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/named",
                axum::routing::get(|| async {
                    // Named by its server, not by its address (gap 138).
                    Response::builder()
                        .header(
                            "content-disposition",
                            "attachment; filename=\"report-2026.pdf\"",
                        )
                        .body(Body::from(payload()))
                        .unwrap()
                }),
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
        // A connection that ends after a thousand bytes of a million, every
        // time, with no ranges to resume from: retried, then failed.
        let app = app.route(
            "/drops.bin",
            axum::routing::get(|| async {
                let some = futures_util::stream::iter([
                    Ok::<Vec<u8>, std::io::Error>(vec![7u8; 1000]),
                    Err(std::io::Error::other("cut")),
                ]);
                Response::builder()
                    .header("content-length", "1000000")
                    .body(Body::from_stream(some))
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
    if harness.state().dialogs.file_info.is_some() {
        settle_details(harness);
    }
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
    // Add URL writes the settings file (its recent addresses).
    let _file = settings_file();
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

/// Waits for Download details to hear from the server: its answer can add
/// a line and move the buttons, so a click under load landed where a
/// button was a frame before. A person, too, sees the dialog settle.
fn settle_details(harness: &mut Harness<'_, DmApp>) {
    until(harness, "Download details to hear from the server", |app| {
        app.dialogs.file_info.as_ref().is_none_or(|d| {
            d.probed
                .as_ref()
                .is_none_or(|p| p.lock().map_or(true, |p| p.is_some()))
        })
    });
    harness.run_steps(2);
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

#[path = "../../../test-support/secrets.rs"]
mod test_secrets;

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
    let drag_at = |harness: &mut Harness<'_, DmApp>, at: egui::Pos2| {
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
    let drag = |harness: &mut Harness<'_, DmApp>, name: &str| {
        let at = harness.get_by_label(name).rect().left_center() + egui::vec2(8.0, 0.0);
        drag_at(harness, at)
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
    // A quick drag from a column's edge resizes the column; it carried the
    // row's file away (UI round 11).
    let edge = egui::pos2(
        harness.get_by_label("Status").rect().left() - 4.0,
        harness.get_by_label("file.zip").rect().center().y,
    );
    // Pressed and moved far in one frame, as a quick hand does.
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(edge));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: edge,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(edge + egui::vec2(60.0, 4.0)));
    harness.step();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(edge + egui::vec2(80.0, 6.0)));
    harness.step();
    let carried = std::mem::take(&mut harness.state_mut().drag_out);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: edge + egui::vec2(80.0, 6.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.run_steps(2);
    assert!(
        carried.is_empty(),
        "a drag from a column's edge carried the row"
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

    // Exit counts down as the others do, then quits (UI round 18: it quit
    // the moment the last download ended, with no word).
    harness.state_mut().run(Command::WhenDoneQuit);
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    quit_after_its_countdown(&mut harness);
}

/// Waits for Exit's countdown, says it has not quit yet, then runs it out.
fn quit_after_its_countdown(harness: &mut Harness<'_, DmApp>) {
    until(harness, "Exit's countdown", |app| {
        app.countdown
            .is_some_and(|(action, _)| action == dm_desktop::power::WhenDone::Quit)
    });
    assert!(!harness.state().quit_asked, "quit before its countdown");
    harness.state_mut().countdown =
        Some((dm_desktop::power::WhenDone::Quit, std::time::Instant::now()));
    until(harness, "dm to quit", |app| app.quit_asked);
}

/// Gap 83: "When done" waited for nothing Running or Queued, so a queue a
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

    // Done: Clear finished, from the menu (the toolbar has one too).
    harness.get_by_label_contains("Done (").click_secondary();
    harness.run_steps(2);
    harness
        .query_all_by_label("Clear finished")
        .last()
        .expect("the menu's Clear finished")
        .click();
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert!(harness.state().rows().is_empty());

    // A kind: categories are edited in Options › Save to.
    harness.get_by_label_contains("Videos (").click_secondary();
    harness.run_steps(2);
    click(&mut harness, "Add or edit categories…");
    let tab = harness.state().dialogs.options.as_ref().map(|o| o.tab);
    assert_eq!(tab, Some(dm_desktop::dialogs::Tab::SaveTo));
}

/// A page's links, handed over by the browser (gap 105): "Start now"
/// starts them, "Add for later" puts them in the Main queue, and the sizes
/// the dialog showed stay on the rows until each learns its own.
fn hand_links(harness: &mut Harness<'_, DmApp>, world: &World, links: Vec<String>) {
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("page.html"),
            referrer: world.url("page.html"),
            links,
            ask: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(2);
    // Titled for a page's every link or a selection's alike.
    assert!(harness.query_by_label("Download links").is_some());
}

#[test]
fn a_pages_links_open_the_download_links_dialog() {
    let world = world();
    let mut harness = world.harness();
    hand_links(
        &mut harness,
        &world,
        vec![
            world.url("file.zip"),
            world.url("other.pdf"),
            world.url("next.html"),
        ],
    );
    // Each link's server is asked for its kind and size.
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.query_by_label("Archives").is_none()
        || harness.query_all_by_label("256.0 KB").count() < 2
    {
        assert!(Instant::now() < deadline, "the links were never checked");
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(50));
    }
    // "Hide HTML files" is on by default: the page is not offered.
    assert!(harness.query_by_label("next.html").is_none());
    click(&mut harness, "other.pdf");
    click(&mut harness, "Add for later");
    harness.state_mut().refresh();
    let rows = harness.state().rows().to_vec();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].url, world.url("file.zip"));
    assert_eq!(rows[0].queue, dm_core::MAIN_QUEUE);
    assert_eq!(rows[0].state, State::Paused, "for later: not started");
    assert_eq!(rows[0].referrer, world.url("page.html"));
    // The size the dialog showed is on the row, though nothing was fetched.
    assert_eq!(rows[0].progress.total, None);
    assert_eq!(
        harness.state().visible()[0].progress.total,
        Some(PAYLOAD as u64),
        "the size the dialog showed was lost"
    );
    harness.run_steps(2);
    assert!(harness.query_by_label("256.0 KB").is_some());

    // OK's place is "Start now", and it starts them.
    hand_links(&mut harness, &world, vec![world.url("other.pdf")]);
    click(&mut harness, "Start now");
    until(&mut harness, "the started link to finish", |app| {
        app.rows()
            .iter()
            .any(|r| r.url.ends_with("/other.pdf") && r.state == State::Completed)
    });
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
    assert_eq!(
        *world.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );

    // Options › General's "opens the file": the double-click opens a
    // finished one straight away; one not finished still shows Properties.
    harness.state_mut().dialogs.properties.clear();
    world.fake.opened.lock().unwrap().clear();
    harness.state_mut().applied.double_click = "open".into();
    double_click(&mut harness, "file.zip");
    harness.run_steps(3);
    assert_eq!(*world.fake.opened.lock().unwrap(), [row.path]);
    assert!(
        harness.state().dialogs.properties.is_empty(),
        "Properties opened instead"
    );
    double_click(&mut harness, "stalled.bin");
    harness.run_steps(3);
    assert!(harness.query_by_label("File properties").is_some());
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

    // With no chooser to pick with, it says so rather than doing nothing.
    *world.fake.no_chooser.lock().unwrap() = true;
    harness.state_mut().run(Command::OpenWith);
    harness.run_steps(2);
    assert!(harness
        .state()
        .said
        .english()
        .starts_with("No file chooser on this desktop to pick an application"));
    assert_eq!(world.fake.opened.lock().unwrap().len(), 1);
    *world.fake.no_chooser.lock().unwrap() = false;
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
    // Save as holds the server's name once it answers: replaced, as a
    // person replaces it, by selecting it all first.
    until(&mut harness, "the server's name in Save as", |app| {
        app.dialogs
            .file_info
            .as_ref()
            .is_some_and(|d| d.name == "file.zip")
    });
    harness.get_by_label("Save as").click();
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("renamed.zip".into()));
    harness.run_steps(2);
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

    // The Queue column names the row's queue (gap 115), as the tree does.
    harness.run_steps(2);
    assert!(
        harness.query_by_label("Main queue").is_some(),
        "the Queue column does not name the queue"
    );
    harness.get_by_label_contains("Main queue (").click();
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

/// Move/Rename: a name taken in that folder is said, with the name given
/// instead; Cancel closes it untouched.
#[test]
fn renaming_onto_a_taken_name_says_what_it_is_called() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("other.pdf"), "Start now");
    until(&mut harness, "both to complete", |app| {
        app.rows().len() == 2 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    let pdf = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.filename == "other.pdf")
        .unwrap()
        .id;
    harness.state_mut().dialogs.rename = Some(dm_desktop::dialogs::MoveRename {
        ids: vec![pdf],
        name: "other.pdf".into(),
        folder: String::new(),
    });
    harness.run_steps(2);
    click(&mut harness, "Cancel");
    assert!(
        harness.state().dialogs.rename.is_none(),
        "Cancel did not close it"
    );
    harness.state_mut().dialogs.rename = Some(dm_desktop::dialogs::MoveRename {
        ids: vec![pdf],
        name: "file.zip".into(),
        folder: String::new(),
    });
    harness.run_steps(2);
    click(&mut harness, "OK");
    let given = harness.state().row(pdf).unwrap().filename.clone();
    assert_ne!(given, "file.zip", "the other file's name was taken over");
    assert_eq!(
        harness.state().said.english(),
        format!("file.zip is taken in that folder, so it is called {given}.")
    );
    // And a screen reader reads it out as it appears (gap 48).
    harness.run_steps(2);
    let said = harness
        .query_by_label_contains("is taken in that folder")
        .expect("the sentence is not in the status bar");
    assert_eq!(
        said.accesskit_node().live(),
        egui::accesskit::Live::Polite,
        "the status bar's sentence is not a live region"
    );
}

/// "Download all links" offers a link already in the list unticked, and
/// says so: it was fetched again as `name (1).zip` without a word (round 13).
#[test]
fn a_pages_link_already_listed_is_offered_unticked_and_said() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.state_mut().refresh();
    let request = dm_shell::host::Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![world.url("file.zip"), world.url("other.pdf")],
        ask: true,
        ..Default::default()
    };
    let app = harness.state_mut();
    let backend = app.backend.clone();
    app.dialogs.open_links(request, &backend);
    harness.run_steps(3);
    let ticks: Vec<bool> = harness
        .state()
        .dialogs
        .links
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.1)
        .collect();
    assert_eq!(ticks, [false, true]);
    assert!(harness
        .query_by_label_contains("(already in the list)")
        .is_some());
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
        .map(dm_desktop::app::name_of)
        .collect();
    assert!(names.contains(&"mine.zip".to_string()), "{names:?}");
    assert!(names.contains(&"other.php".to_string()), "{names:?}");
    assert!(harness.query_by_label("mine.zip").is_some());
}

/// A column is widened by dragging its heading's right edge, as in any
/// list: in the window it did nothing (round 13).
#[test]
fn a_column_is_widened_by_dragging_its_edge() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    // Status's heading starts where its column does (the headings of
    // numbers sit at their columns' right): dragging the edge just left of
    // it widens Size and moves Status along.
    let size_at = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(Role::Button, "Status")
            .rect()
            .left()
    };
    let before = size_at(&harness);
    let edge = egui::pos2(
        before - 5.0,
        harness
            .get_by_role_and_label(Role::Button, "Status")
            .rect()
            .center()
            .y,
    );
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(edge));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: edge,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for step in 1..=8 {
        harness.input_mut().events.push(egui::Event::PointerMoved(
            edge + egui::vec2(10.0 * step as f32, 0.0),
        ));
        harness.step();
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: edge + egui::vec2(80.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.run_steps(3);
    let after = size_at(&harness);
    assert!(after > before + 40.0, "Size moved from {before} to {after}");

    // And kept at the next launch: the window's memory, written out and
    // read back as eframe does, gives a new window the same widths (gap 47:
    // they were back to their first ones).
    let saved = harness
        .ctx
        .memory(ron::to_string)
        .expect("the window's memory is written out");
    drop(harness);
    // A new window over the same list, its row listed again.
    let mut again = world.harness();
    again.ctx.memory_mut(|m| {
        *m = ron::from_str(&saved).expect("and read back");
    });
    add_url(&mut again, &world.url("file.zip"), "Add for later");
    again.run_steps(3);
    let relaunched = size_at(&again);
    assert!(
        (relaunched - after).abs() < 1.0,
        "Size was {after} wide when left, {relaunched} at the next launch"
    );
}

/// Add links from clipboard offers an address already in the list
/// unticked and says so (round 14).
#[test]
fn a_clipboard_link_already_listed_is_offered_unticked() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.state_mut().refresh();
    *world.fake.clipboard.lock().unwrap() = Some(format!(
        "{} {}",
        world.url("file.zip"),
        world.url("other.pdf")
    ));
    harness.state_mut().run(Command::AddFromClipboard);
    harness.run_steps(3);
    let ticks: Vec<bool> = harness
        .state()
        .dialogs
        .clip_links
        .as_ref()
        .unwrap()
        .iter()
        .map(|l| l.1)
        .collect();
    assert_eq!(ticks, [false, true]);
    assert!(harness
        .query_by_label_contains("(already in the list)")
        .is_some());
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
    // And the control protocol names it by the title it shows (UI round 5).
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

    // In progress is what is under way (gap 124): the stalled download,
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
    // server never said what it was (UI round 6).
    harness.get_by_label_contains("Archives (2)").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("file.zip").is_some());
    assert!(harness.query_by_label("secret.zip").is_some());
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

/// Copy link puts the row's address on the clipboard, without the sign-in
/// that is the row's own; the row menu offers it.
#[test]
fn copy_link_copies_the_address_without_its_sign_in() {
    let password = test_secrets::secret("password");
    let world = world();
    let mut harness = world.harness();
    let plain = world.url("private.bin");
    harness
        .state()
        .backend()
        .add_with(
            &plain.replace("http://", &format!("http://me:{password}@")),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    select(&mut harness, "private.bin");
    assert!(harness.state().enabled(Command::CopyLink));
    harness.state_mut().run(Command::CopyLink);
    harness.run_steps(1);
    let copied: Vec<String> = harness
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(copied, [plain], "copied {copied:?}");
    assert!(
        !copied.concat().contains(&password),
        "the sign-in went with it"
    );
}

#[test]
fn properties_keep_the_login_out_of_the_address_box() {
    let password = test_secrets::secret("password");
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add_with(
            &world
                .url("private.bin")
                .replace("http://", &format!("http://me:{password}@")),
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
        !address.contains(&password),
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
    // A failed row is retried, and its button says so (gap 115).
    assert!(harness.query_by_label("Resume").is_none());
    click(&mut harness, "Retry");
    until(&mut harness, "the refreshed download to complete", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
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
    // second trip through View (UI round 14).
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

/// Gap 165: the series' error is in the language in force, says nothing
/// until From and To are typed, and keeps its room so the buttons stay put.
#[test]
fn the_series_error_waits_for_both_ends_and_speaks_german() {
    use dm_desktop::i18n::t;
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        show_progress: false,
        show_complete: false,
        language: "de".into(),
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    harness.run_steps(2);
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(2);
    let start = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(egui::accesskit::Role::Button, t("Start now"))
            .rect()
    };
    let before = start(&harness);
    type_into(&mut harness, t("Address with *"), &world.url("part.bin"));
    let star = t("The address needs exactly one * to fill in.");
    assert_ne!(star, "The address needs exactly one * to fill in.");
    assert!(
        harness.query_by_label(star).is_none(),
        "said before From/To"
    );
    type_into(&mut harness, t("From"), "5");
    assert!(harness.query_by_label(star).is_none(), "said before To");
    type_into(&mut harness, t("To"), "1");
    assert!(harness.query_by_label(star).is_some(), "not said in German");
    assert_eq!(start(&harness), before, "the dialog jumped");
}

/// What a press of Start said goes once the range is changed: it stood
/// over the count of what was typed since.
#[test]
fn the_series_error_from_start_goes_when_the_range_is_fixed() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(2);
    type_into(&mut harness, "Address with *", &world.url("part*.bin"));
    type_into(&mut harness, "From", "5");
    type_into(&mut harness, "To", "1");
    click(&mut harness, "Start now");
    let said = harness
        .state()
        .dialogs
        .series
        .as_ref()
        .unwrap()
        .says
        .clone();
    assert!(!said.is_empty(), "Start said nothing of a backwards range");
    type_into(&mut harness, "To", "9");
    assert!(
        harness.query_by_label(&said).is_none(),
        "the old error still stands"
    );
    assert!(harness.query_by_label_contains("15 addresses").is_some());
    assert!(harness.state().rows().is_empty());
}

/// Gap 167, kept to the column: a row added for later shows the folder it
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
    while harness.query_by_label_contains("Files found: 2 ").is_none() {
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
    // Below the found files, which scroll under the dialog's buttons:
    // scrolled to, as a person would.
    harness.get_by_label("Add for later").scroll_to_me();
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

/// A dialog as tall as the room lets it be, give or take: it turned
/// scrolling on and off on alternate frames, and no click on it took
/// (Site files on Windows' fonts, which stand it 28 points taller).
#[test]
fn a_dialog_at_the_edge_of_scrolling_still_takes_clicks() {
    let _file = settings_file();
    let world = world();
    for height in (684..=720).step_by(4) {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1100.0, height as f32))
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        harness.run_steps(2);
        click(&mut harness, "Site files");
        type_into(&mut harness, "Start page", &world.url("page.html"));
        click(&mut harness, "Find files");
        let started = harness
            .state()
            .dialogs
            .site_files
            .as_ref()
            .map(|d| !matches!(*dm_core::lock(&d.state), dm_desktop::dialogs::Grab::Idle));
        assert_eq!(
            started,
            Some(true),
            "Find files did not take at {height} points tall"
        );
    }
}

#[test]
fn a_site_search_reads_pages_deep_and_a_saved_one_runs_on_its_own() {
    let _file = settings_file();
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
    click(&mut harness, "Find files");
    // page.html and next.html on this site; the page elsewhere is not read.
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness
        .query_by_label_contains("Files found: 3 · pages read: 2")
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "the search did not go a page deep: {:?}",
            {
                let d = harness.state().dialogs.site_files.as_ref();
                d.map(|d| format!("{:?}", dm_core::lock(&d.state)))
            }
        );
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(25));
    }

    // The window grew to hold the list and, centred, moved: let it settle
    // before clicking where its fields now are.
    harness.run_steps(3);

    // Saved to run every hour: it runs now, and what it finds is added for
    // later; run again by hand it adds nothing twice.
    // Below the found files, which scroll under the dialog's buttons.
    harness.get_by_label("Save this search as").scroll_to_me();
    harness.run_steps(3);
    type_into(&mut harness, "Save this search as", "Nightly");
    if let Some(d) = harness.state_mut().dialogs.site_files.as_mut() {
        d.every_hours = 1;
    }
    harness.run_steps(1);
    harness.get_by_label("Save search").scroll_to_me();
    harness.run_steps(3);
    click(&mut harness, "Save search");
    let saved = harness.state().backend().settings().site_searches;
    assert_eq!(saved.len(), 1);
    assert_eq!(
        (saved[0].name.as_str(), saved[0].depth, saved[0].every_hours),
        ("Nightly", 1, 1)
    );
    assert_eq!(saved[0].types, ["zip", "pdf"]);
    assert!(harness.query_by_label_contains("Nightly").is_some());
    // Not at once: its first run on the timer is an hour from saving (UI
    // round 15: run at once, its files were queued with nothing said).
    harness.run_steps(5);
    assert!(harness.state().rows().is_empty(), "it ran as it was saved");
    // An hour on.
    let mut later = harness.state().backend().settings();
    later.site_searches[0].last_run -= 3600;
    harness.state_mut().apply_settings(later).unwrap();
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
    let _file = settings_file();
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
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    click(&mut harness, "Sort finished files into folders by kind");
    // At the foot of the page, scrolled to as a person would.
    harness.get_by_label("Add a category").scroll_to_me();
    harness.run_steps(3);
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
    // Under the page's fold, scrolled to as a person would.
    harness.get_by_label("Delete category").scroll_to_me();
    harness.run_steps(3);
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
fn the_file_types_the_browser_hands_over_are_set_in_options() {
    let _file = settings_file();
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
    // Gap 49: sites whose links work once.
    type_into(
        &mut harness,
        "Sites whose links work only once",
        "*.once.example",
    );
    click(&mut harness, "OK");
    let catch = harness.state().backend().settings().catch;
    assert_eq!(catch.extensions, ["zip", "iso", "mkv"]);
    assert_eq!(catch.skip_hosts, ["intranet.example"]);
    assert_eq!(catch.once_hosts, ["*.once.example"]);
    // What the extension is sent is what was typed.
    let rules = dm_protocol::CatchRules { ..catch };
    assert!(rules.enabled);
}

#[test]
fn site_limits_attempts_and_the_timeout_are_set_in_options_and_reach_the_queue() {
    let _file = settings_file();
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
    let _file = settings_file();
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
    let _file = settings_file();
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

    // The menu bar takes no clicks behind it (UI round 10: its menus
    // opened and their items did nothing).
    harness.get_by_label("View").click();
    harness.run_steps(3);
    assert!(
        !harness.ctx.any_popup_open(),
        "a menu opened behind Options"
    );
    assert!(harness.state().dialogs.options.is_some());

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

/// A double-click on a link in the browser hands it over twice in a moment:
/// one row, not `name (1)` and `name (2)` (round 15).
#[test]
fn the_same_address_handed_twice_at_once_is_taken_once() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.ask_panel_downloads = false;
    for _ in 0..2 {
        harness
            .state()
            .backend()
            .handed
            .lock()
            .unwrap()
            .push_back(dm_shell::host::Request {
                url: world.url("file.zip"),
                ..Default::default()
            });
        harness.state_mut().take_handed();
    }
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 1);
}

/// Under the Remove question the list takes no clicks: a row selected
/// there was not the one removed (round 15). The question names its row.
#[test]
fn the_list_behind_the_remove_question_takes_no_clicks() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    harness.state_mut().refresh();
    select(&mut harness, "file.zip");
    let chosen = harness.state().selected.clone();
    harness.state_mut().run(Command::Remove);
    harness.run_steps(3);
    assert!(harness.state().dialogs.confirm_remove.is_some());
    assert!(
        harness.query_all_by_label("file.zip").count() >= 2,
        "the question names no row"
    );
    harness.get_by_label("other.pdf").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().selected,
        chosen,
        "a row was selected under the question"
    );
}

/// Remove can take a finished file off the disk with its row, when asked;
/// left alone, the file stays.
#[test]
fn remove_deletes_the_file_too_only_when_asked() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("other.pdf"), "Start now");
    until(&mut harness, "both to complete", |app| {
        app.rows().len() == 2 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    let path_of = |h: &Harness<'_, DmApp>, name: &str| {
        h.state()
            .rows()
            .iter()
            .find(|r| r.filename == name)
            .unwrap()
            .path
            .clone()
    };
    let (zip, pdf) = (
        path_of(&harness, "file.zip"),
        path_of(&harness, "other.pdf"),
    );
    select(&mut harness, "file.zip");
    harness.state_mut().run(Command::Remove);
    harness.run_steps(3);
    click(&mut harness, "Remove it");
    harness.state_mut().refresh();
    assert!(
        std::path::Path::new(&zip).is_file(),
        "the file went without being asked"
    );
    select(&mut harness, "other.pdf");
    harness.state_mut().run(Command::Remove);
    harness.run_steps(3);
    click(&mut harness, "Also delete the file from the disk");
    click(&mut harness, "Yes, delete");
    harness.state_mut().refresh();
    assert!(!std::path::Path::new(&pdf).exists(), "the file stayed");
    assert!(harness.state().rows().is_empty());
}

/// Add URL remembers the addresses added through it, never with a login,
/// offers them again, and forgets them on Clear history.
#[test]
fn add_url_remembers_addresses_without_their_login_and_clears_them() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "Needs a sign-in");
    type_into(&mut harness, "Login", "me");
    click(&mut harness, "OK");
    assert_eq!(harness.state().applied.recent_urls, [world.url("file.zip")]);
    harness.state_mut().dialogs.file_info = None;
    harness.run_steps(2);
    click(&mut harness, "Add URL");
    assert!(harness.query_by_label("Recent").is_some());
    click(&mut harness, "Clear history");
    assert!(harness.state().applied.recent_urls.is_empty());
}

/// View › Text size makes everything larger, smaller, or normal again.
#[test]
fn view_text_size_zooms_the_window() {
    let world = world();
    let mut harness = world.harness();
    press(&mut harness, &["View", "Text size", "Larger"]);
    harness.run_steps(3);
    let zoom = harness.ctx.zoom_factor();
    assert!((zoom - 1.1).abs() < 0.01, "{zoom}");
    press(&mut harness, &["View", "Text size", "Normal size"]);
    harness.run_steps(3);
    assert!((harness.ctx.zoom_factor() - 1.0).abs() < 0.01);
}

/// Options › General saves the settings to a file without any password,
/// and loads them back into the dialog, to be used on OK.
#[test]
fn settings_are_saved_to_a_file_without_passwords_and_loaded_back() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::Options);
    harness.run_steps(3);
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.connections = 5;
        d.draft.proxy.username = "someone".into();
        d.draft.proxy.password = "not-in-the-file".into();
    }
    let file = world.dir.path().join("dm-settings.json");
    *world.fake.chosen.lock().unwrap() = Some(file.clone());

    // By the keyboard, as a person reaches a button below the fold: the
    // focus brings it into view, and Space presses it.
    let press_by_keyboard = |harness: &mut Harness<'_, DmApp>, name: &str| {
        harness.get_by_label(name).focus();
        harness.run_steps(3);
        harness.key_press(egui::Key::Space);
        harness.run_steps(3);
    };
    press_by_keyboard(&mut harness, "Save settings to a file…");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"connections\": 5"), "{text}");
    assert!(
        !text.contains("not-in-the-file"),
        "a password went into the file"
    );
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.connections = 2;
    }
    press_by_keyboard(&mut harness, "Load settings from a file…");
    let d = harness.state().dialogs.options.as_ref().unwrap();
    assert_eq!(d.draft.connections, 5);
    assert_eq!(
        d.draft.proxy.password, "not-in-the-file",
        "this computer's password was lost"
    );
    *world.fake.chosen.lock().unwrap() = None;
}

/// Download details' Category sends a file to the folder of the kind
/// picked, here a .zip to Documents rather than Archives.
#[test]
fn download_details_category_picks_the_folder() {
    let world = world();
    // With sorting into kind folders off, as it is unless turned on: the
    // pick is still that kind's folder.
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        categorize: false,
        show_progress: false,
        show_complete: false,
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 760.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    harness.run_steps(2);
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(4);
    click(&mut harness, "Category");
    harness.get_by_label("Documents").click();
    harness.run_steps(3);
    let folder = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .unwrap()
        .folder
        .clone();
    assert!(folder.ends_with("Documents"), "{folder}");
    click(&mut harness, "Start now");
    until(&mut harness, "it to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let path = harness.state().rows()[0].path.clone();
    assert!(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .ends_with("Documents"),
        "{path}"
    );
}

/// Move/Rename moves several finished files to another folder at once,
/// and their rows follow them.
#[test]
fn move_rename_moves_several_finished_files_together() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("other.pdf"), "Start now");
    until(&mut harness, "both to complete", |app| {
        app.rows().len() == 2 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    harness.state_mut().run(Command::SelectAll);
    harness.state_mut().run(Command::MoveRename);
    harness.run_steps(3);
    assert!(harness.query_by_label("Moves 2 finished files.").is_some());
    let to = world.dir.path().join("moved");
    harness.state_mut().dialogs.rename.as_mut().unwrap().folder = to.display().to_string();
    harness.run_steps(2);
    click(&mut harness, "OK");
    harness.state_mut().refresh();
    for row in harness.state().rows() {
        let path = std::path::Path::new(&row.path);
        assert_eq!(path.parent(), Some(to.as_path()), "{}", row.path);
        assert!(path.is_file(), "{} is not there", row.path);
    }
}

/// A dialog opened from the keyboard gives the keyboard back to what
/// opened it when it closes.
#[test]
fn closing_a_dialog_gives_the_keyboard_back_to_its_opener() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    harness
        .get_by_role_and_label(Role::Button, "Add URL")
        .focus();
    harness.run_steps(2);
    harness.key_press(egui::Key::Space);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.add_url.is_some(),
        "Space did not open Add URL"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(4);
    assert!(harness.state().dialogs.add_url.is_none());
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Add URL")
            .accesskit_node()
            .is_focused(),
        "the keyboard was not given back to Add URL"
    );
}

/// The pointer says the same thing everywhere, as a desktop app's does: the
/// arrow over buttons, checkboxes, menus and rows; the text cursor over a
/// field; the resize arrows over a column's edge.
#[test]
fn the_pointer_is_the_same_kind_over_the_same_kind_of_thing() {
    use egui::accesskit::Role;
    use egui::CursorIcon;
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    let over = |harness: &mut Harness<'_, DmApp>, at: egui::Pos2| {
        harness.hover_at(at);
        harness.run_steps(2);
        harness.output().platform_output.cursor_icon
    };
    let button = harness
        .get_by_role_and_label(Role::Button, "Add URL")
        .rect()
        .center();
    assert_eq!(
        over(&mut harness, button),
        CursorIcon::Default,
        "a toolbar button"
    );
    let row = harness.get_by_label("file.zip").rect().center();
    assert_eq!(over(&mut harness, row), CursorIcon::Default, "a row");
    let status = harness.get_by_role_and_label(Role::Button, "Status").rect();
    let edge = egui::pos2(status.left() - 5.0, status.center().y);
    assert_eq!(
        over(&mut harness, edge),
        CursorIcon::ResizeColumn,
        "a column's edge"
    );
    harness.state_mut().run(Command::AddUrl);
    harness.run_steps(3);
    let field = harness.get_by_label("Address").rect().center();
    assert_eq!(over(&mut harness, field), CursorIcon::Text, "a text field");
    let check = harness.get_by_label("Needs a sign-in").rect().center();
    assert_eq!(over(&mut harness, check), CursorIcon::Default, "a checkbox");
}

/// The progress window's Cancel download asks first; Keep downloading
/// (and Enter) leaves it running.
#[test]
fn cancel_download_in_the_progress_window_asks_first() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("stalled.bin"))
        .unwrap();
    until(&mut harness, "it to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    harness.run_steps(3);
    click(&mut harness, "Cancel download");
    assert!(
        harness.query_by_label("Cancel this download?").is_some(),
        "nothing asked"
    );
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert_ne!(
        harness.state().rows()[0].state,
        State::Cancelled,
        "Enter cancelled it"
    );
    click(&mut harness, "Cancel download");
    click(&mut harness, "Yes, cancel");
    until(&mut harness, "it to be cancelled", |app| {
        app.rows()[0].state == State::Cancelled
    });
}

/// Options stays inside the window at large text sizes: at 200% in a
/// 960×660 window, and at 150% in the smallest, its tabs and OK were off
/// the window's left edge (UI rounds 3 and 4).
#[test]
fn options_fits_the_window_at_large_text() {
    use egui::accesskit::Role;
    for (zoom, size) in [
        (2.0, egui::vec2(960.0, 660.0)),
        (1.5, egui::vec2(720.0, 460.0)),
    ] {
        let world = world();
        let mut harness = Harness::builder()
            .with_size(size)
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        harness.ctx.set_zoom_factor(zoom);
        harness.run_steps(3);
        harness.state_mut().run(Command::Options);
        harness.run_steps(6);
        let points = size / zoom;
        for name in ["General", "OK"] {
            if name == "OK" {
                // In a window too short for the buttons beside the settings
                // the dialog scrolls as one page (UI round 6): its end is
                // brought into view as a person would, with the wheel.
                let general = harness
                    .get_by_role_and_label(Role::Button, "General")
                    .rect();
                harness.hover_at(general.center() + egui::vec2(0.0, 40.0));
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
                harness.run_steps(3);
            }
            let at = harness
                .query_all_by_label(name)
                .find(|n| {
                    matches!(
                        n.accesskit_node().role(),
                        Role::Button | Role::Tab | Role::Label | Role::ListBoxOption
                    )
                })
                .unwrap_or_else(|| harness.get_by_label(name))
                .rect();
            assert!(
                at.left() >= 0.0
                    && at.right() <= points.x
                    && at.top() >= 0.0
                    && at.bottom() <= points.y,
                "{zoom}x {size:?}: {name} at {at:?} is outside the window"
            );
        }
    }
}

/// F10 opens the menu bar's first menu, and Shift+F10 the menu of the row
/// with the keyboard's place, as on any desktop.
#[test]
fn menus_open_from_the_keyboard() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    // Menu items carry their keys after their names.
    assert!(harness
        .query_by_label_contains("Add numbered series")
        .is_none());
    harness.key_press(egui::Key::F10);
    harness.run_steps(4);
    assert!(
        harness
            .query_by_label_contains("Add numbered series")
            .is_some(),
        "F10 opened no menu"
    );
    // Right goes on to the next menu, Left back, round the bar.
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(4);
    assert!(
        harness.query_by_label_contains("Move to queue").is_some(),
        "Right did not open File"
    );
    assert!(harness
        .query_by_label_contains("Add numbered series")
        .is_none());
    harness.key_press(egui::Key::ArrowLeft);
    harness.run_steps(4);
    harness.key_press(egui::Key::ArrowLeft);
    harness.run_steps(4);
    assert!(
        harness
            .query_by_label_contains("Check for updates")
            .is_some(),
        "Left from the first did not reach Help"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    select(&mut harness, "file.zip");
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    harness.run_steps(4);
    let props = harness
        .query_by_label_contains("Properties")
        .expect("Shift+F10 opened no row menu")
        .rect();
    // Under the row, not over the places beside the list (UI round 5).
    let row = harness.get_by_label("file.zip").rect();
    assert!(
        props.left() >= row.left() - 30.0 && props.top() >= row.top(),
        "the menu at {props:?} is not under the row at {row:?}"
    );
}

#[test]
fn two_cancelled_hand_offs_from_a_site_offer_to_leave_it_to_the_browser() {
    let _file = settings_file();
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
    let _file = settings_file();
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
    // The made-up server cannot be asked, so its link is unticked: ticked
    // again, as a person would who wants it anyway.
    until(
        &mut harness,
        "the made-up server's link to be refused",
        |app| {
            app.dialogs
                .links
                .as_ref()
                .is_some_and(|d| d.refused.iter().any(|u| u.ends_with("/x.zip")))
        },
    );
    click(&mut harness, "Check all");
    click(&mut harness, "Add for later");
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
    let _file = settings_file();
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
    // and kept, as seven ticks (UI round 12).
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
fn a_bad_category_options_opened_with_does_not_fail_an_unrelated_ok() {
    let _file = settings_file();
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
    let _file = settings_file();
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
        dm_shell::settings::full_path("~/dm-test-downloads").unwrap()
    );
    let mut back = harness.state().backend().settings();
    back.download_dir = before.download_dir;
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
fn view_language_puts_the_window_in_another_language_and_back() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(2);
    assert!(harness.query_by_label("Program").is_some());
    // Everything shut stays shut in another language (UI round 10).
    assert!(harness.query_by_label("All (0)").is_some());
    click(&mut harness, "Everything");
    assert!(
        harness.query_by_label("All (0)").is_none(),
        "Everything did not shut"
    );
    click(&mut harness, "View");
    harness.get_by_label_contains("Language").click();
    harness.run_steps(3);
    harness.get_by_label("Deutsch").click();
    harness.run_steps(3);
    assert_eq!(harness.state().backend().settings().language, "de");
    harness.run_steps(10);
    assert!(harness.query_by_label("Alles").is_some());
    assert!(
        harness.query_by_label("Alle (0)").is_none(),
        "the language switch opened Everything again"
    );
    click(&mut harness, "Alles");
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
    let _file = settings_file();
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
    click(&mut harness, "Stop download");
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

    // The best of five runs of ten frames: the suite's other tests and
    // whatever else the machine is doing stretch one run, not the window's
    // own work (it read 106 ms beside a Docker build, 38 ms alone).
    let per_frame = (0..5)
        .map(|_| {
            let started = Instant::now();
            for _ in 0..10 {
                harness.step();
            }
            started.elapsed() / 10
        })
        .min()
        .unwrap_or_default();
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
            // A toolbar button of the same name (32 px tall or more) may
            // lie under the open menu; the menu's item is the one meant.
            let item = |n: &egui_kittest::Node<'_>| {
                n.accesskit_node()
                    .bounding_box()
                    .is_some_and(|b| b.height() < 30.0)
            };
            let matching: Vec<_> = h
                .query_all_by_label_contains(name)
                .filter(|n| {
                    let label = n.accesskit_node().label().unwrap_or_default();
                    label == *name
                        || label
                            .strip_prefix(&format!("{name} "))
                            .is_some_and(|key| !key.starts_with(|c: char| c.is_lowercase()))
                })
                .collect();
            let menu = matching.iter().rposition(item);
            menu.or(matching.len().checked_sub(1))
                .and_then(|at| matching.into_iter().nth(at))
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
    // Greyed where the computer has no such command (macOS).
    if dm_desktop::power::way(dm_desktop::power::WhenDone::Hibernate, std::env::consts::OS)
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
        (Command::SiteFiles, "Site files"),
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
    // Reached, scrolling the dialog's body where it is taller than the room.
    harness.get_by_label("OK").scroll_to_me();
    harness.run_steps(3);
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
    // Enter finds too, and the field keeps the keyboard (gap 120): the
    // next Enter finds the next, and typing goes on into the field.
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alphabet.bin"]);
    assert!(
        harness.ctx.egui_wants_keyboard_input(),
        "Find's field let go of the keyboard after Enter"
    );
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alpha.bin"], "a second Enter");
    harness.event(egui::Event::Text("bet".into()));
    harness.run_steps(2);
    assert_eq!(harness.state().dialogs.find_text, "alphabet");
    harness.key_press(egui::Key::F3);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alphabet.bin"], "F3 with Find open");
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
    let _file = settings_file();
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

/* ------------------------------------------- a human tester's session (6) */

/// Gap 107: a dialog opened to be typed in takes the keyboard at once, and
/// Enter presses its default button.
#[test]
fn a_dialog_is_typed_in_at_once_and_enter_presses_its_default_button() {
    let world = world();
    let mut harness = world.harness();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    harness.run_steps(3);
    assert!(harness.state().dialogs.add_url.is_some());
    // No click in the field first.
    harness.event(egui::Event::Text(world.url("file.zip")));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.add_url.is_none(),
        "Enter did not press OK"
    );
    let urls = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .map(|d| d.urls.clone());
    assert_eq!(urls, Some(vec![world.url("file.zip")]));
    harness.state_mut().dialogs = Default::default();
    harness.run_steps(2);

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    harness.run_steps(3);
    harness.event(egui::Event::Text("file".into()));
    harness.run_steps(2);
    assert_eq!(harness.state().dialogs.find_text, "file", "Find's field");

    harness.state_mut().dialogs = Default::default();
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(3);
    harness.event(egui::Event::Text(world.url("img*.jpg")));
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .series
            .as_ref()
            .map(|d| d.template.clone()),
        Some(world.url("img*.jpg")),
        "the series' address field"
    );
}

/// Gap 108: a closed schedule is said in the status bar with when it
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

/// Gap 109: a download that starts without the window starting it — a
/// browser's hand-off — opens its progress window too, when that is on.
#[test]
fn a_browsers_hand_off_opens_its_progress_window() {
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        connections: 1,
        show_progress: true,
        show_complete: false,
        language: "en".into(),
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    harness.run_steps(2);
    // As the native host's hand-off lands: in the queue, not through the
    // window's own Add.
    let id = harness
        .state()
        .backend()
        .add_with(&world.url("stalled.bin"), dm_core::AddOptions::default())
        .unwrap();
    until(&mut harness, "its progress window", |app| {
        app.dialogs.progress.contains(&id)
    });
    // One added for later has not started, and opens nothing.
    let later = harness
        .state()
        .backend()
        .add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    assert!(!harness.state().dialogs.progress.contains(&later));
}

/// Gap 110: several downloads finishing make one "Download complete" window
/// listing each, with its own Open and Open folder.
#[test]
fn downloads_finishing_together_share_one_complete_window() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    harness.run_steps(2);
    let urls = [
        world.url("file.zip"),
        world.url("other.pdf"),
        world.url("deep.pdf"),
    ];
    harness.state_mut().add(&urls, Default::default()).unwrap();
    until(&mut harness, "all three to finish", |app| {
        app.rows().len() == 3 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    assert_eq!(harness.query_all_by_label("Download complete").count(), 1);
    assert!(harness.query_by_label("3 downloads finished.").is_some());
    click(&mut harness, "Open other.pdf");
    let other = row(&harness, "other.pdf");
    assert_eq!(
        *world.fake.opened.lock().unwrap(),
        std::slice::from_ref(&other.path),
        "rows: {:?}",
        harness
            .state()
            .rows()
            .iter()
            .map(|r| (r.url.clone(), r.path.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(harness.state().dialogs.complete.len(), 2, "the rest stay");
    click(&mut harness, "Open the folder of deep.pdf");
    assert!(world.fake.opened.lock().unwrap()[1].starts_with("reveal "));
    click(&mut harness, "Close");
    assert!(harness.query_by_label("Download complete").is_none());
}

/// Gap 111: Exit while downloading asks first; a script's exit does not.
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

/// Gap 163: downloads finishing while the Exit question is open never quit
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

/// Gap 114: the sort is remembered, a status line naming a row goes with
/// it, and Options keeps its place whichever tab shows.
#[test]
fn the_sort_survives_a_restart_and_options_keeps_its_place() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(2);
    click(&mut harness, "Size");
    click(&mut harness, "Size");
    let saved = harness.state().backend().settings();
    assert_eq!(
        (saved.sort_column.as_str(), saved.sort_ascending),
        ("size", false)
    );
    let again = world.app_with(saved);
    assert_eq!(again.sort, Some((Column::Size, false)), "not remembered");
    // A third click puts the list back in the queue's order (UI round 6).
    click(&mut harness, "Size");
    assert_eq!(harness.state().sort, None);
    assert_eq!(harness.state().backend().settings().sort_column, "");
    click(&mut harness, "Size");
    click(&mut harness, "Size");

    // "Found file.zip." does not outlive file.zip.
    harness.state_mut().dialogs.find_text = "file".into();
    harness.state_mut().run(Command::FindNext);
    assert!(harness.state().said.english().contains("file.zip"));
    let id = harness.state().rows()[0].id;
    harness.state_mut().remove_now(&[id]);
    assert!(
        harness.state().said.is_empty(),
        "{:?}",
        harness.state().said
    );

    click(&mut harness, "Options");
    let ok = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(egui::accesskit::Role::Button, "OK")
            .accesskit_node()
            .bounding_box()
    };
    let first = ok(&harness);
    for tab in ["Folders", "Network", "Sign-ins", "Downloads", "General"] {
        // The tab, not the menu of that name: Options is drawn after it.
        harness.query_all_by_label(tab).last().unwrap().click();
        harness.run_steps(3);
        assert_eq!(ok(&harness), first, "Options moved on {tab}");
    }
    let mut wanted = harness.state().applied.clone();
    wanted.sort_column = String::new();
    harness.state_mut().apply_settings(wanted).unwrap();
}

/// Gap 115: a server that wants a sign-in says where to give one, and
/// Properties opens on its Login.
#[test]
fn a_refused_sign_in_says_where_to_give_one() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let failed = harness.state().rows()[0].clone();
    let said = dm_desktop::app::status_of(&failed);
    // In plain words, saying where a sign-in goes, without the address or
    // the server's page (QA checklist ER-02).
    assert!(said.contains("Properties › Login"), "{said}");
    assert!(!said.contains("http"), "{said}");
    select(&mut harness, "secret.zip");
    // Retry, not Resume, on the toolbar, in the File menu and the row menu.
    assert!(harness.query_by_label("Retry").is_some());
    assert_eq!(harness.state().title_of(Command::DownloadNow), "Retry");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(3);
    harness.event(egui::Event::Text("user".into()));
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .properties
            .get(&failed.id)
            .map(|d| d.login.clone()),
        Some("user".into()),
        "the keyboard is not in Login"
    );
}

/// Gap 115: the speed limiter's menu item is ticked while a limit is on.
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

/// Gap 115: links on the clipboard can be started at once, or not at all.
#[test]
fn links_on_the_clipboard_can_start_now_or_be_cancelled() {
    let world = world();
    let mut harness = world.harness();
    *world.fake.clipboard.lock().unwrap() = Some(world.url("file.zip"));
    harness.state_mut().run(Command::AddFromClipboard);
    harness.run_steps(2);
    click(&mut harness, "Cancel");
    assert!(harness.state().dialogs.clip_links.is_none());
    assert!(harness.state().rows().is_empty());
    harness.state_mut().run(Command::AddFromClipboard);
    harness.run_steps(2);
    click(&mut harness, "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
}

/// Gap 115: a folder's field has a Browse… button, and what it picks is
/// where the file goes.
#[test]
fn browse_picks_the_folder_a_download_goes_to() {
    let world = world();
    let mut harness = world.harness();
    let picked = world.dir.path().join("picked");
    std::fs::create_dir_all(&picked).unwrap();
    *world.fake.chosen.lock().unwrap() = Some(picked.clone());
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    click(&mut harness, "Browse…");
    let folder = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .map(|d| d.folder.clone());
    assert_eq!(folder, Some(picked.display().to_string()));
    click(&mut harness, "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    assert!(std::path::Path::new(&harness.state().rows()[0].path).starts_with(&picked));

    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    assert_eq!(harness.query_all_by_label("Browse…").count(), 2);
}

/// Gap 115: at the narrowest window and in German, every toolbar button
/// still shows its name, rather than a picture to guess at.
#[test]
fn the_toolbar_keeps_its_names_narrow_and_in_german() {
    // And in the classic look, whose buttons were a fixed 72 points and
    // ran German's names into each other when the toolbar wrapped (gap 145).
    // And at 200% text, where even its icons alone ran past the right
    // edge (UI round 5): they go on to a second row.
    for (look, zoom) in [("light", 1.0), ("classic", 1.0), ("light", 2.0)] {
        let world = world();
        let app = world.app_with(Settings {
            download_dir: world.dir.path().join("out"),
            show_progress: false,
            show_complete: false,
            language: "de".into(),
            theme: look.into(),
            ..Settings::default()
        });
        let mut harness = Harness::builder()
            .with_size(egui::vec2(720.0, 460.0))
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
        harness.ctx.set_zoom_factor(zoom);
        harness.run_steps(4);
        let mut seen: Vec<(String, egui::accesskit::Rect)> = Vec::new();
        for (_, _, label) in dm_desktop::app::toolbar_in(&[]) {
            let name = dm_desktop::i18n::t(label);
            let node = harness.get_by_role_and_label(egui::accesskit::Role::Button, name);
            let bounds = node.accesskit_node().bounding_box().unwrap();
            let shown = harness
                .ctx
                .fonts_mut(|f| {
                    f.layout_no_wrap(
                        name.into(),
                        egui::FontId::proportional(11.5),
                        egui::Color32::WHITE,
                    )
                })
                .size()
                .x;
            // Where names are drawn, each fits its button: the modern look
            // shows icons alone when its names would not fit on one row.
            if look == "classic" {
                assert!(
                    bounds.width() as f32 >= shown,
                    "{name}: {} wide, its name {shown}",
                    bounds.width()
                );
            }
            assert!(
                // In the window's pixels, whatever the text size.
                bounds.x1 <= 720.0,
                "{look} {zoom}x: {name} is past the window's edge"
            );
            for (other, at) in &seen {
                let meet = bounds.x0 < at.x1 - 0.5
                    && at.x0 < bounds.x1 - 0.5
                    && bounds.y0 < at.y1 - 0.5
                    && at.y0 < bounds.y1 - 0.5;
                assert!(!meet, "{look}: {name} runs into {other}");
            }
            seen.push((name.to_string(), bounds));
        }
    }
    dm_desktop::i18n::set("en");
}

/// Gap 104, in the window: Clear finished takes completed and cancelled
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

/// The bounding box of the last node of that name, in window points.
fn bounds(h: &Harness<'_, DmApp>, name: &str) -> egui::accesskit::Rect {
    h.query_all_by_label(name)
        .last()
        .unwrap_or_else(|| panic!("{name:?} is not on screen"))
        .accesskit_node()
        .bounding_box()
        .unwrap_or_else(|| panic!("{name:?} has no place on screen"))
}

/// Gap 117: the arrow keys, Home and End move the list's selection, Shift
/// extends it, and the keyboard's place is ringed.
#[test]
fn the_arrow_keys_move_the_selection_and_the_focus_shows() {
    let world = world();
    let mut harness = world.harness();
    for name in ["a.bin", "b.bin", "c.bin"] {
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
    let picked = |h: &Harness<'_, DmApp>| {
        h.state()
            .targets()
            .iter()
            .map(dm_desktop::app::name_of)
            .collect::<Vec<_>>()
    };
    harness.key_press(egui::Key::ArrowDown);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["a.bin"]);
    assert!(
        harness.state().focus_ring.is_some(),
        "no ring where the keys are"
    );
    harness.key_press(egui::Key::ArrowDown);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowDown);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["b.bin", "c.bin"]);
    harness.key_press(egui::Key::Home);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["a.bin"]);
    harness.key_press(egui::Key::End);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["c.bin"]);
    // A click puts the ring away; Tab brings it to the control focused.
    click(&mut harness, "a.bin");
    assert!(harness.state().focus_ring.is_none(), "a ring after a click");
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    assert!(harness.state().focus_ring.is_some(), "no ring after Tab");
    // With the keyboard's focus on a control, not a text field, the list's
    // keys still work: Delete asks about the selected row (round 14).
    harness.key_press(egui::Key::Delete);
    harness.run_steps(2);
    assert!(
        harness.state().dialogs.confirm_remove.is_some(),
        "Delete did nothing while a control had the focus"
    );
}

/// Gap 118: "Download complete" reaches every file (its list scrolls), cuts
/// a long name so Close stays in view, sits in the corner rather than over
/// the rows, and Escape closes all of it.
#[test]
fn the_complete_window_scrolls_cuts_long_names_and_closes_on_escape() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    harness.run_steps(2);
    let long = format!("{}.zip", "very-long-name-".repeat(20));
    harness
        .state_mut()
        .add(
            &[world.url("file.zip")],
            dm_core::AddOptions {
                save_name: Some(long.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    let urls: Vec<String> = (0..11).map(|_| world.url("file.zip")).collect();
    harness.state_mut().add(&urls, Default::default()).unwrap();
    until(&mut harness, "all twelve to finish", |app| {
        app.rows().len() == 12 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    harness.run_steps(4);
    assert!(harness.query_by_label("12 downloads finished.").is_some());
    // Several at a time, not one (UI round 6).
    let seen = harness
        .query_all_by_label_contains("Open the folder of")
        .filter(|n| {
            n.accesskit_node()
                .bounding_box()
                .is_some_and(|b| b.y1 <= 720.0)
        })
        .count();
    assert!(seen >= 4, "only {seen} of twelve finished files in view");
    let close = bounds(&harness, "Close");
    assert!(close.x1 <= 1100.0, "Close is off the window: {close:?}");
    // In the corner, out of the middle of the list.
    assert!(close.x0 > 1100.0 / 2.0, "not in the corner: {close:?}");
    // The entry lowest in the window: it lists the newest first, so which
    // row that is depends on the order they finished in, and under load the
    // list's last row could finish last and stand at the top.
    let open_last = harness
        .query_all_by_label_contains("Open the folder of")
        .max_by(|a, b| {
            let y = |n: &egui_kittest::Node<'_>| {
                n.accesskit_node().bounding_box().map_or(0.0, |r| r.y0)
            };
            y(a).total_cmp(&y(b))
        })
        .and_then(|n| n.accesskit_node().label().map(|l| l.to_string()))
        .expect("no Open the folder of");
    let before = bounds(&harness, &open_last);
    assert!(
        before.y0 > close.y0,
        "the premise: the last file is below the fold"
    );
    // The wheel over the list scrolls it.
    let list = bounds(&harness, "12 downloads finished.");
    harness.hover_at(egui::pos2(list.x0 as f32 + 20.0, list.y1 as f32 + 60.0));
    for _ in 0..20 {
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
    let after = bounds(&harness, &open_last);
    assert!(
        after.y1 < close.y0,
        "the last file is not reachable: {after:?} under {close:?}"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(
        harness.state().dialogs.complete.is_empty(),
        "Escape closed one file of the window, not the window"
    );
}

/// Gap 119: Tab goes round a dialog's own controls, never to the menu bar
/// behind it, and Enter with none of them focused presses the default.
#[test]
fn tab_stays_in_the_dialog_and_enter_presses_its_default() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    let dialog = harness
        .query_by_label("Start now")
        .expect("Download details is open");
    let dialog = dialog.accesskit_node().bounding_box().unwrap();
    let _ = dialog;
    for _ in 0..12 {
        harness.key_press(egui::Key::Tab);
        harness.run_steps(1);
        let focused = harness.ctx.memory(|m| m.focused());
        let layer = focused.and_then(|id| harness.ctx.read_response(id).map(|r| r.layer_id));
        if let Some(layer) = layer {
            assert_eq!(
                layer.order,
                egui::Order::Middle,
                "Tab left the dialog for {layer:?}"
            );
        }
    }
    assert!(!harness.ctx.any_popup_open(), "Tab opened a menu behind");
    // Nothing focused: Enter is Start now.
    harness.ctx.memory_mut(|m| m.stop_text_input());
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.file_info.is_none(),
        "Enter did not press Start now"
    );
    assert_eq!(harness.state().rows().len(), 1);
}

/// Gap 121: Browse… on a desktop with no folder chooser says so.
#[test]
fn browse_with_no_folder_chooser_says_to_type_the_path() {
    let world = world();
    let mut harness = world.harness();
    *world.fake.no_chooser.lock().unwrap() = true;
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    click(&mut harness, "Browse…");
    assert!(harness
        .query_by_label("No folder chooser on this desktop; type the folder's path.")
        .is_some());
}

/// Gap 123: a long refusal wraps in the progress window, which stays on
/// the screen; the status bar cuts a long sentence and keeps it on hover.
#[test]
fn long_status_text_wraps_and_stays_on_screen() {
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        connections: 1,
        show_progress: true,
        show_complete: false,
        language: "en".into(),
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 600.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    let long = format!("{}secret.zip", "a-long-path/".repeat(12));
    let url = world.url(&long);
    add_url(&mut harness, &url, "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(4);
    let retry = bounds(&harness, "Retry");
    assert!(retry.x1 <= 720.0 && retry.y1 <= 600.0, "{retry:?}");
    for node in harness.query_all_by_label_contains("needs a sign-in") {
        let b = node.accesskit_node().bounding_box().unwrap();
        assert!(b.x1 <= 720.5, "the status ran off the window: {b:?}");
    }
    harness.state_mut().dialogs = Default::default();
    harness.state_mut().said = format!(
        "this computer could not open {}: xdg-open: {}\nmore",
        "/x".repeat(30),
        "lynx: not found ".repeat(10)
    )
    .into();
    harness.run_steps(3);
    for node in harness.query_all_by_label_contains("this computer could not open") {
        let b = node.accesskit_node().bounding_box().unwrap();
        assert!(b.x1 <= 720.5, "the status bar ran off the window: {b:?}");
    }
}

/// Gap 126: Resume on a row the schedule holds offers to start it now,
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
    // Nothing is said about waiting once nothing waits (gap 129).
    harness.state_mut().said = dm_desktop::i18n::Said::new(
        "Waiting for the schedule or the download limit; it starts by itself when allowed.",
        &[],
    );
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains("Waiting for the schedule or the download limit")
            .is_none(),
        "a stale 'Waiting for the schedule' with nothing held"
    );
    let mut off = harness.state().applied.clone();
    off.schedule.enabled = false;
    harness.state_mut().apply_settings(off).unwrap();
}

/// Gap 127: a sign-in saved in Properties on a download refused for want
/// of one tries it again, and it downloads.
#[test]
fn a_sign_in_saved_in_properties_retries_the_download() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("locked.zip"), "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    select(&mut harness, "locked.zip");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(3);
    // The keyboard is in Login already.
    harness.event(egui::Event::Text("user".into()));
    harness.run_steps(2);
    let id = harness.state().rows()[0].id;
    if let Some(d) = harness.state_mut().dialogs.properties.get_mut(&id) {
        assert_eq!(d.login, "user", "the keyboard is not in Login");
        d.password = "pass".into();
    }
    click(&mut harness, "OK");
    assert!(harness
        .state()
        .said
        .english()
        .contains("again with the sign-in"));
    until(&mut harness, "it to download with the sign-in", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
    // Gap 140: "Trying … again" goes once it has been tried.
    until(&mut harness, "the sentence to go", |app| {
        !app.said.english().contains("again with the sign-in")
    });
}

/// Gap 128: the Delete key asks as the toolbar's Delete does, for a
/// finished row too; the speed limiter's field has the keyboard and Enter
/// applies it.
#[test]
fn the_delete_key_asks_and_the_limiter_takes_enter() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    select(&mut harness, "file.zip");
    harness.key_press(egui::Key::Delete);
    harness.run_steps(3);
    assert_eq!(harness.state().rows().len(), 1, "Delete removed it at once");
    assert!(harness.query_by_label("Remove from the list").is_some());
    click(&mut harness, "Keep it");
    click(&mut harness, "Remove");
    assert!(
        harness.state().dialogs.confirm_remove.is_some(),
        "the toolbar's Delete asks the same"
    );
    click(&mut harness, "Remove it");
    assert!(harness.state().rows().is_empty());

    harness.state_mut().run(Command::SpeedLimiterSettings);
    harness.run_steps(3);
    assert!(
        harness.ctx.egui_wants_keyboard_input(),
        "the limit's field does not have the keyboard"
    );
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("321".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.limiter.is_none(),
        "Enter did not apply"
    );
    assert_eq!(harness.state().applied.speed_limit, 321);
    let mut off = harness.state().applied.clone();
    off.speed_limit = 0;
    harness.state_mut().apply_settings(off).unwrap();
}

/// Gap 125: Options › Folders gives each kind's folder field the room its
/// name needs.
#[test]
fn the_kinds_folder_fields_fit_their_names() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    let mut wanted = harness.state().applied.clone();
    wanted.categorize = true;
    harness.state_mut().applied = wanted.clone();
    harness.state_mut().dialogs.open_options(&wanted);
    harness.run_steps(2);
    click(&mut harness, "Folders");
    harness.run_steps(3);
    for kind in ["Videos", "Documents", "Archives", "Other"] {
        let field = harness.get_by_role_and_label(Role::TextInput, kind);
        let b = field.accesskit_node().bounding_box().unwrap();
        assert!(b.width() >= 120.0, "{kind}'s field is {} wide", b.width());
    }
}

/* ------------------------------------------- the eighth hands-on round */

/// Every piece of text the last frame painted.
fn painted_text(harness: &Harness<'_, DmApp>) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &harness.output().shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// Adds a row shown by that name (its address's last part), for later,
/// without fetching anything.
fn add_named(harness: &mut Harness<'_, DmApp>, world: &World, name: &str) {
    let url = url::Url::parse(&world.url("x"))
        .unwrap()
        .join(name)
        .unwrap();
    harness
        .state_mut()
        .add(
            &[url.to_string()],
            dm_core::AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            },
        )
        .unwrap();
}

/// Gap 130: an Arabic name is drawn with its words in reading order, and a
/// screen reader is still given it as stored.
#[test]
fn an_arabic_name_is_drawn_in_reading_order() {
    let world = world();
    let mut harness = world.harness();
    // Drawn as a box on the one frame before the computer's font for it is
    // read, or on a computer with none: the order is what is checked here.
    harness
        .ctx
        .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
    add_named(&mut harness, &world, "ملف عربي.zip");
    harness.run_steps(4);
    let drawn = painted_text(&harness);
    assert!(
        drawn.iter().any(|t| t == "عربي ملف.zip"),
        "not drawn in reading order: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "ملف عربي.zip"),
        "drawn in stored order"
    );
    assert!(harness.query_by_label("ملف عربي.zip").is_some());
}

/// Gap 133: Download details forgets an earlier opening's warning, and a
/// warning wraps inside a 720 px window rather than pushing Start now off it.
#[test]
fn download_details_starts_afresh_and_its_warning_wraps() {
    let world = world();
    let app = world.app();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 600.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    *world.fake.no_chooser.lock().unwrap() = true;
    let warning = "No folder chooser on this desktop; type the folder's path.";
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    click(&mut harness, "Browse…");
    harness.run_steps(2);
    assert!(harness.query_by_label(warning).is_some());
    let start = harness.get_by_label("Start now");
    let b = start.accesskit_node().bounding_box().unwrap();
    assert!(
        b.x1 <= 720.0 && b.x0 >= 0.0,
        "Start now is off the window: {b:?}"
    );
    click(&mut harness, "Cancel");
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    assert!(
        harness.query_by_label(warning).is_none(),
        "the last opening's warning stayed"
    );
}

/// Gap 134: a right click moves the keyboard's place to the row clicked.
#[test]
fn a_right_click_moves_the_keyboards_place() {
    let world = world();
    let mut harness = world.harness();
    for name in ["one.zip", "two.zip", "three.zip"] {
        add_named(&mut harness, &world, name);
    }
    harness.run_steps(3);
    harness.key_press(egui::Key::Home);
    harness.run_steps(2);
    let first = harness.state().cursor;
    harness.get_by_label("three.zip").click_secondary();
    harness.run_steps(2);
    let three = harness
        .state()
        .rows()
        .iter()
        .find(|r| dm_desktop::app::name_of(r) == "three.zip")
        .map(|r| r.id);
    assert_ne!(first, three);
    assert_eq!(harness.state().cursor, three, "the cursor stayed behind");
    assert_eq!(
        harness.state().anchor,
        three,
        "Shift would extend from elsewhere"
    );
}

/// Gap 135: Enter presses Start now in "Links on the clipboard" and in
/// "Download links".
#[test]
fn enter_starts_the_links_dialogs() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.clip_links = Some(vec![(world.url("file.zip"), true)]);
    harness.run_steps(3);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.clip_links.is_none(),
        "Enter did nothing"
    );
    assert_eq!(harness.state().rows().len(), 1);
    harness.state_mut().dialogs.links = Some(dm_desktop::dialogs::LinksImport {
        page: world.url("page.html"),
        sign_in: None,
        cookies: String::new(),
        items: vec![(world.url("other.pdf"), true)],
        listed: Default::default(),
        refused: Default::default(),
        hide_html: true,
        found: Default::default(),
    });
    harness.run_steps(3);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(harness.state().dialogs.links.is_none(), "Enter did nothing");
    assert_eq!(harness.state().rows().len(), 2);
}

/// Gap 136: Find never covers the row it found.
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

/// Gap 138: Save as takes the name the server gives, and leaves one a
/// person typed alone.
#[test]
fn save_as_takes_the_servers_name() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("named")]);
    until(&mut harness, "the server's name", |app| {
        app.dialogs
            .file_info
            .as_ref()
            .is_some_and(|d| d.name == "report-2026.pdf")
    });
    // Laid out once it is open, before it is clicked.
    harness.run_steps(3);
    click(&mut harness, "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    assert!(harness.state().rows()[0].path.ends_with("report-2026.pdf"));

    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("named")]);
    harness.run_steps(1);
    if let Some(d) = harness.state_mut().dialogs.file_info.as_mut() {
        d.name = String::new();
        d.name_typed = true;
    }
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        harness.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        harness
            .state()
            .dialogs
            .file_info
            .as_ref()
            .map(|d| d.name.as_str()),
        Some(""),
        "what a person typed was replaced"
    );
}

/// Gap 139: Start now in Download details while the schedule is shut asks
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

/// Gap 140: the limiter's field takes what is typed in place of what it
/// held; the toolbar's buttons stay put when Resume becomes Retry; the
/// progress window keeps its place and its percentage in view, and says
/// what a refusal leaves unknown; the empty list's words are under the
/// headings.
#[test]
fn the_small_things_of_round_eight() {
    let _file = settings_file();
    let world = world();
    // Under the headings, with nothing in the list.
    {
        let mut harness = world.harness();
        harness.run_steps(3);
        let heading = harness
            .get_by_label("Name")
            .accesskit_node()
            .bounding_box()
            .unwrap();
        let words = harness
            .get_by_label("Nothing queued")
            .accesskit_node()
            .bounding_box()
            .unwrap();
        assert!(
            words.y0 > heading.y1,
            "the empty list's words are above its headings"
        );
    }

    // The limiter: 500 typed over 64 is 500, not 64500.
    let mut harness = world.harness();
    let mut wanted = harness.state().applied.clone();
    wanted.speed_limit = 64;
    harness.state_mut().apply_settings(wanted).unwrap();
    harness.state_mut().run(Command::SpeedLimiterSettings);
    harness.run_steps(4);
    harness.event(egui::Event::Text("500".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert_eq!(harness.state().applied.speed_limit, 500);
    let mut off = harness.state().applied.clone();
    off.speed_limit = 0;
    harness.state_mut().apply_settings(off).unwrap();

    // The toolbar: one finished row, one refused.
    let mut wanted = harness.state().applied.clone();
    wanted.show_progress = true;
    harness.state_mut().apply_settings(wanted).unwrap();
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    let progress = |h: &Harness<'_, DmApp>| {
        let id = h.state().rows()[0].id;
        h.ctx
            .memory(|m| m.area_rect(egui::Id::unique(("progress", id))))
    };
    harness.run_steps(3);
    let before = progress(&harness).expect("the progress window is open");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(4);
    let after = progress(&harness).expect("the progress window is open");
    assert!(
        (after.min - before.min).length() < 1.0,
        "the progress window moved from {before:?} to {after:?}"
    );
    // The server answered, with a refusal: said as one (UI round 17).
    assert!(harness
        .query_by_label("No — the server refused it")
        .is_some());
    // The bar no longer says the percentage beside itself (UI round 9:
    // Status above says it); no words of its own to be cut off.
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.remove(&id);
    let mut quiet = harness.state().applied.clone();
    quiet.show_progress = false;
    harness.state_mut().apply_settings(quiet).unwrap();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let delete_at = |h: &Harness<'_, DmApp>| {
        h.get_by_label("Remove")
            .accesskit_node()
            .bounding_box()
            .unwrap()
            .x0
    };
    select(&mut harness, "file.zip");
    harness.run_steps(2);
    assert!(harness.query_by_label("Resume").is_some());
    let resume = delete_at(&harness);
    select(&mut harness, "secret.zip");
    harness.run_steps(2);
    assert!(harness.query_by_label("Retry").is_some());
    let retry = delete_at(&harness);
    assert_eq!(resume, retry, "Delete moved when Resume became Retry");
}

/// Gap 151: in the classic look at 720 px, the progress window's bar keeps
/// its width after an error rather than shrinking to a pill.
#[test]
fn the_progress_bar_keeps_its_width_in_the_classic_look() {
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        connections: 1,
        show_progress: true,
        show_complete: false,
        language: "en".into(),
        theme: "classic".into(),
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 600.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(6);
    let bar = harness
        .query_all_by_role(egui::accesskit::Role::ProgressIndicator)
        .map(|n| n.accesskit_node().bounding_box().unwrap())
        .fold(0.0_f64, |w, b| w.max(b.width()));
    assert!(bar >= 300.0, "the bar is {bar} wide");
    // And its track can be seen: at 0% the bar was drawn in the dialog's
    // own colour, all but its words' pill gone.
    let at = harness
        .query_all_by_role(egui::accesskit::Role::ProgressIndicator)
        .map(|n| n.accesskit_node().bounding_box().unwrap())
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .unwrap();
    // Drawn to pixels where the machine has a GPU to draw with; CI's Linux
    // runner has none, and wgpu panics finding no adapter (CI, 0.2.0). The
    // width above holds either way.
    let Ok(Ok(image)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.render()))
    else {
        eprintln!("no GPU to render with here; the track's colour is not checked");
        return;
    };
    let scale = image.width() as f64 / 720.0;
    let pixel = |x: f64, y: f64| *image.get_pixel((x * scale) as u32, (y * scale) as u32);
    let track = pixel(at.x0 + at.width() * 0.8, (at.y0 + at.y1) / 2.0);
    let dialog = pixel(at.x0 + at.width() * 0.8, at.y1 + 4.0);
    assert_ne!(track, dialog, "the bar's track is the dialog's own colour");
}

/// Gap 141: OK refuses a proxy turned on with no host, or with an address
/// where its host goes, and says why on the Proxy tab; nothing is put in force.
#[test]
fn options_refuses_a_proxy_with_no_host() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Proxy");
    click(&mut harness, "Connect through a proxy");
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.options.is_some(),
        "OK took a proxy with no host"
    );
    assert!(harness
        .query_by_label_contains("A proxy needs a host")
        .is_some());
    assert!(!harness.state().backend().settings().proxy.enabled);
    type_into(&mut harness, "Host", "http://proxy.example:3128");
    click(&mut harness, "OK");
    assert!(harness
        .query_by_label_contains("is not a proxy host")
        .is_some());
    assert!(!harness.state().backend().settings().proxy.enabled);
    click(&mut harness, "Cancel");
}

/// Gap 142: the link Add URL opens with is selected, so a link pasted over
/// it replaces it; two addresses run together are refused with a reason.
#[test]
fn add_url_selects_its_link_and_refuses_two_run_together() {
    let world = world();
    let mut harness = world.harness();
    let first = world.url("file.zip");
    let second = world.url("other.pdf");
    harness
        .state_mut()
        .dialogs
        .open_add_url(Some(first.clone()));
    harness.run_steps(4);
    harness.event(egui::Event::Text(second.clone()));
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .add_url
            .as_ref()
            .map(|d| d.address.clone()),
        Some(second.clone()),
        "the pasted link joined the one already there"
    );
    if let Some(d) = harness.state_mut().dialogs.add_url.as_mut() {
        d.address = format!("{first}{second}");
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness
        .query_by_label_contains("two addresses run together")
        .is_some());
    assert!(harness.query_by_label("Download details").is_none());
    assert!(harness.state().rows().is_empty());
    // The address keeps the keyboard: what is typed next replaces it.
    harness.run_steps(3);
    harness.event(egui::Event::Text(second.clone()));
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .add_url
            .as_ref()
            .map(|d| d.address.clone()),
        Some(second),
        "typing after the warning went nowhere"
    );
}

/// Gap 143: Import asks, once, about addresses already in the list: Skip
/// them adds only the new ones, Add them again adds them all.
#[test]
fn import_asks_about_addresses_already_listed() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    let file = world.dir.path().join("two.txt");
    std::fs::write(
        &file,
        format!("{}\n{}\n", world.url("file.zip"), world.url("other.pdf")),
    )
    .unwrap();
    *world.fake.chosen.lock().unwrap() = Some(file.clone());
    harness.state().backend().pause_all();
    harness.state_mut().run(Command::Import);
    harness.run_steps(3);
    assert_eq!(harness.state().rows().len(), 1, "imported without asking");
    assert!(harness
        .query_by_label_contains("1 of the 2 addresses in this list")
        .is_some());
    click(&mut harness, "Skip them");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 2, "only the new one is added");

    harness.state_mut().run(Command::Import);
    harness.run_steps(3);
    click(&mut harness, "Add them again");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 4);
}

/// Gap 144: where there is no file chooser, Import and Export ask for the
/// file's path, rather than doing nothing.
#[test]
fn import_and_export_take_a_typed_path_without_a_file_chooser() {
    let world = world();
    let mut harness = world.harness();
    *world.fake.no_chooser.lock().unwrap() = true;
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    let file = world.dir.path().join("typed.dmlist");
    harness.state_mut().run(Command::Export);
    harness.run_steps(3);
    assert!(harness
        .query_by_label("No file chooser on this desktop; type the file's path.")
        .is_some());
    if let Some(d) = harness.state_mut().dialogs.list_file.as_mut() {
        assert!(d.path.ends_with("downloads.dmlist"), "{}", d.path);
        d.path = file.display().to_string();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(file.is_file(), "nothing was exported");

    let mut other = world.harness();
    other.state_mut().run(Command::Import);
    other.run_steps(3);
    type_into(&mut other, "File's path", "relative.dmlist");
    click(&mut other, "OK");
    assert!(other.query_by_label_contains("full path").is_some());
    if let Some(d) = other.state_mut().dialogs.list_file.as_mut() {
        d.path = file.display().to_string();
    }
    other.run_steps(2);
    click(&mut other, "OK");
    other.state_mut().refresh();
    assert_eq!(other.state().rows().len(), 1, "nothing was imported");
    *world.fake.no_chooser.lock().unwrap() = false;
}

/// Gaps 147 and 148: Page Down and Page Up move a screenful; End brings the
/// last row wholly into view, clear of the sideways scroll bar; the focus
/// ring stays inside the list.
#[test]
fn page_keys_move_a_page_and_end_shows_the_last_row_whole() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    let links: Vec<String> = (0..40)
        .map(|i| world.url(&format!("stalled-{i:02}.bin")))
        .collect();
    harness.state().backend().pause_all();
    harness.state().backend().add(&links.join(" ")).unwrap();
    harness.state().backend().pause_all();
    harness.state_mut().refresh();
    harness.run_steps(3);
    let at = |h: &Harness<'_, DmApp>| {
        let ids: Vec<_> = h.state().visible().iter().map(|r| r.id).collect();
        h.state()
            .cursor
            .and_then(|c| ids.iter().position(|i| *i == c))
    };
    harness.key_press(egui::Key::Home);
    harness.run_steps(2);
    assert_eq!(at(&harness), Some(0));
    let page = harness.state().list_page;
    assert!(page > 2, "a page of {page} rows");
    harness.key_press(egui::Key::PageDown);
    harness.run_steps(3);
    assert_eq!(at(&harness), Some(page), "Page Down did not move a page");
    harness.key_press(egui::Key::PageUp);
    harness.run_steps(3);
    assert_eq!(at(&harness), Some(0), "Page Up did not move back");

    harness.key_press(egui::Key::End);
    harness.run_steps(4);
    assert_eq!(at(&harness), Some(39));
    let clip = harness.state().list_clip.expect("the list was drawn");
    let last = harness.state().visible()[39].id;
    let row = harness.state().row_rects[&last];
    assert!(
        row.bottom() <= clip.bottom() + 0.5,
        "the last row {row:?} is under the list's edge {clip:?}"
    );
    let ring = harness
        .state()
        .focus_ring
        .expect("a ring where the keys are");
    assert!(
        clip.expand(0.5).contains_rect(ring),
        "the ring {ring:?} runs past the list {clip:?}"
    );
}

/// Gap 149: the Remove dialog's buttons agree with its sentence, Enter
/// keeps, and after a removal the next row is selected.
#[test]
fn remove_says_one_thing_enter_keeps_and_the_next_row_is_selected() {
    let world = world();
    let mut harness = world.harness();
    for name in ["a.bin", "b.bin", "c.bin"] {
        harness
            .state()
            .backend()
            .add_with(
                &world.url(&format!("stalled-{name}")),
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.run_steps(3);
    let ids: Vec<_> = harness.state().visible().iter().map(|r| r.id).collect();
    harness.state_mut().selected = [ids[1]].into();
    harness.run_steps(1);
    harness.key_press(egui::Key::Delete);
    harness.run_steps(3);
    assert!(harness.query_by_label("Keep it").is_some());
    assert!(harness.query_by_label("Keep them").is_none());
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(harness.state().dialogs.confirm_remove.is_none());
    assert_eq!(harness.state().rows().len(), 3, "Enter deleted it");

    harness.key_press(egui::Key::Delete);
    harness.run_steps(3);
    click(&mut harness, "Yes, remove");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 2);
    assert_eq!(
        harness.state().selected,
        [ids[2]].into(),
        "the selection was lost"
    );
}

/// Gap 150: a drop with no link in it says how links are added, rather
/// than nothing; the empty list says so too.
#[test]
fn a_drop_with_no_link_says_how_to_add_one() {
    let world = world();
    let mut harness = world.harness();
    assert!(harness
        .query_by_label_contains(dm_desktop::app::drop_hint())
        .is_some());
    let file = world.dir.path().join("picture.png");
    std::fs::write(&file, [0u8, 1, 2, 3]).unwrap();
    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(Dropped(file)));
    harness.run_steps(3);
    assert!(harness.state().rows().is_empty());
    assert!(
        harness
            .query_all_by_label_contains(dm_desktop::app::drop_hint())
            .count()
            >= 2,
        "the drop was ignored without a word"
    );
}

/// Gap 152: the list's up-and-down bar is at its visible right edge when
/// the columns run past the window, and dragging it scrolls the list.
#[test]
fn the_lists_bar_stays_in_view_when_the_columns_run_past_it() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    harness.state_mut().hidden_columns.clear();
    let links: Vec<String> = (0..40)
        .map(|i| world.url(&format!("gone-{i:02}.bin")))
        .collect();
    harness.state().backend().pause_all();
    for link in &links {
        harness
            .state()
            .backend()
            .add_with(
                link,
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.run_steps(4);
    let bar = harness.state().list_bar.expect("a bar for 40 rows");
    let clip = harness.state().list_clip.expect("the list was drawn");
    assert!(bar.right() <= 720.0, "the bar {bar:?} is past the window");
    assert!(
        bar.left() >= clip.right() - 1.0,
        "the bar {bar:?} is not beside the list {clip:?}"
    );
    let first = harness.state().visible()[0].id;
    let top = harness.state().row_rects[&first].top();
    // Dragged from its top to its bottom, as a person drags it.
    let from = egui::pos2(bar.center().x, bar.top() + 6.0);
    let to = egui::pos2(bar.center().x, bar.bottom() - 2.0);
    let input = |h: &mut Harness<'_, DmApp>, event: egui::Event| {
        h.input_mut().events.push(event);
        h.run_steps(2);
    };
    input(&mut harness, egui::Event::PointerMoved(from));
    input(
        &mut harness,
        egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    );
    for step in 1..=8 {
        let at = from + (to - from) * (step as f32 / 8.0);
        input(&mut harness, egui::Event::PointerMoved(at));
    }
    input(
        &mut harness,
        egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    );
    harness.run_steps(4);
    let moved = harness
        .state()
        .row_rects
        .get(&first)
        .map(|r| r.top())
        .unwrap_or(f32::NEG_INFINITY);
    assert!(
        moved < top - 100.0,
        "the list did not scroll: {top} to {moved}"
    );
}

/// Gap 153: a failed row's status is a plain reason, in Properties held to
/// its column; the engine's words are kept for hover.
#[test]
fn a_cut_connection_is_said_plainly() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(
            |ui, app: &mut DmApp| app.ui(ui),
            world.app_with(Settings {
                download_dir: world.dir.path().join("out"),
                connections: 1,
                attempts: 1,
                show_progress: false,
                show_complete: false,
                language: "en".into(),
                ..Settings::default()
            }),
        );
    harness
        .state()
        .backend()
        .add(&world.url("drops.bin"))
        .unwrap();
    until(&mut harness, "the cut download to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let row = harness.state().rows()[0].clone();
    let status = dm_desktop::app::status_of(&row);
    assert_eq!(
        status, "Error: the server ended the connection before the file was complete",
        "{:?}",
        row.state
    );
    assert!(dm_desktop::app::failure_as_said(&row).is_some());
    harness.state_mut().selected = [row.id].into();
    harness.state_mut().queued.push(Command::Properties);
    harness.run_steps(4);
    // In the list's row and in Properties.
    for shown in harness.get_all_by_label(&status) {
        let rect = shown.rect();
        assert!(rect.height() < 60.0, "the status stands {rect:?} tall");
    }
}

/// Gap 155: while a range waits to be tried again, the row says which
/// attempt it is on.
#[test]
fn a_retry_says_its_attempt() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(
            |ui, app: &mut DmApp| app.ui(ui),
            world.app_with(Settings {
                download_dir: world.dir.path().join("out"),
                connections: 1,
                attempts: 5,
                show_progress: false,
                show_complete: false,
                language: "en".into(),
                ..Settings::default()
            }),
        );
    harness
        .state()
        .backend()
        .add(&world.url("drops.bin"))
        .unwrap();
    until(&mut harness, "the row to say it retries", |app| {
        app.rows()
            .first()
            .is_some_and(|r| dm_desktop::app::status_of(r).starts_with("Retrying (attempt 2 of 5)"))
    });
    // A new address is taken while it retries, and finishes it (UI round 8:
    // the field was greyed as for a running download).
    let id = harness.state().rows()[0].id;
    harness.state_mut().selected = [id].into();
    assert!(harness.state().enabled(Command::UpdateLink));
    let good = world.url("file.zip");
    harness.state().backend().set_url(id, &good).unwrap();
    until(&mut harness, "the new address to finish it", |app| {
        app.rows()
            .first()
            .is_some_and(|r| r.state == State::Completed && r.url == good)
    });
}

/// Gap 154, then round 15: Enter answers "Already in the list", with the
/// answer that changes nothing — Show it — since an Enter meant for the
/// dialog before it fetched the file again.
#[test]
fn enter_shows_the_row_when_already_listed() {
    let world = world();
    let mut harness = world.harness();
    let url = world.url("file.zip");
    add_url(&mut harness, &url, "Add for later");
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    assert!(harness.query_by_label("Already in the list").is_some());
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(harness.query_by_label("Already in the list").is_none());
    assert!(harness.query_by_label("Download details").is_none());
    assert_eq!(harness.state().rows().len(), 1);
    assert_eq!(harness.state().selected.len(), 1, "the row is not shown");
}

/// Gap 157: a row added for later is queued, not stopped; Exit counts the
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

/// Gap 159: a hand-off of an address already listed is added, and said.
#[test]
fn a_hand_off_of_a_listed_address_is_said_to_be_added_again() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.state_mut().refresh();
    harness.run_steps(2);
    // As the inbox adds a browser's hand-off: to the queue, not by the
    // window.
    harness
        .state()
        .backend()
        .add_with(&world.url("file.zip"), dm_core::AddOptions::default())
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    assert_eq!(
        harness.state().said.english(),
        "Added file.zip again: that address was already in the list."
    );
}

/// Gap 160: Add URL keeps its place when its warning appears, and
/// Download details shows the size once the server says it.
#[test]
fn dialogs_keep_their_place_and_show_the_size() {
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    harness.run_steps(4);
    let before = harness.get_by_label("OK").rect();
    let two = format!("{}{}", world.url("a.zip"), world.url("b.zip"));
    type_into(&mut harness, "Address", &two);
    click(&mut harness, "OK");
    harness.run_steps(4);
    assert!(harness.query_by_label_contains("two addresses").is_some());
    let after = harness.get_by_label("OK").rect();
    assert!(
        (before.top() - after.top()).abs() < 1.0,
        "OK moved from {before:?} to {after:?}"
    );
    harness.state_mut().dialogs.add_url = None;
    harness.run_steps(2);
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    until(&mut harness, "the size to be shown", |_| true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.query_by_label("256.0 KB").is_none() {
        assert!(Instant::now() < deadline, "no size in Download details");
        harness.run_steps(2);
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Gap 167: a row added for later says it waits for Start queue, is counted
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

/// Gap 156: a finished row's size is its file's on disk, as a stream's
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

/// Gap 48: the bar under a running download's status is read by a screen
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

/// Gap 47: a file added again, while its first copy is in the folder, is
/// listed under the name it will be placed under, not the one it will not.
#[test]
fn a_file_added_again_is_listed_under_the_name_it_will_get() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the first copy to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    assert!(
        harness.query_by_label("file (1).zip").is_some(),
        "the second copy is not listed as file (1).zip"
    );
    // And the control protocol says so too (UI round 6).
    let seen = harness
        .state_mut()
        .control_call("ui.inspect", &serde_json::json!({}))
        .unwrap();
    let names: Vec<&str> = seen["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert!(names.contains(&"file (1).zip"), "{names:?}");
    // And it is placed there once fetched.
    click(&mut harness, "Start queue");
    until(&mut harness, "the second copy to finish", |app| {
        app.rows()
            .iter()
            .filter(|r| r.state == State::Completed)
            .count()
            == 2
    });
    assert!(harness
        .state()
        .rows()
        .iter()
        .any(|r| r.path.ends_with("file (1).zip")));
}

/// Gap 47: the list's sideways scroll bar is dragged like any other; its
/// thumb did nothing (Shift and the wheel worked).
#[test]
fn the_lists_sideways_bar_is_dragged() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(760.0, 560.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    let name_at =
        |h: &Harness<'_, DmApp>| h.get_by_role_and_label(Role::Button, "Name").rect().left();
    let before = name_at(&harness);
    // The bar runs along the list's foot, a little over the status bar.
    let foot = harness
        .get_by_label_contains("waiting for Start queue")
        .rect()
        .top();
    let mut grabbed = None;
    for up in (4..40).step_by(3).map(|u| u as f32) {
        let at = egui::pos2(300.0, foot - up);
        let events = [
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ];
        harness.input_mut().events.extend(events);
        harness.step();
        for step in 1..=8 {
            harness.input_mut().events.push(egui::Event::PointerMoved(
                at + egui::vec2(25.0 * step as f32, 0.0),
            ));
            harness.step();
        }
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at + egui::vec2(200.0, 0.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        harness.run_steps(3);
        if name_at(&harness) < before - 20.0 {
            grabbed = Some(up);
            break;
        }
    }
    assert!(
        grabbed.is_some(),
        "dragging along the list's foot did not move the columns (Name stayed at {before})"
    );
}

/// Gap 50: a row waiting in the queue is dragged to another place in it,
/// as Move up and Move down move it a place at a time.
#[test]
fn a_row_is_dragged_to_another_place_in_the_queue() {
    let world = world();
    let mut harness = world.harness();
    for file in ["file.zip", "other.pdf", "x.bin"] {
        add_url(&mut harness, &world.url(file), "Add for later");
    }
    harness.run_steps(3);
    let order = |h: &Harness<'_, DmApp>| -> Vec<String> {
        h.state()
            .rows
            .iter()
            .map(dm_desktop::app::name_of)
            .collect()
    };
    assert_eq!(order(&harness), ["file.zip", "other.pdf", "x.bin"]);
    let rect = |h: &Harness<'_, DmApp>, name: &str| h.get_by_label(name).rect();
    let from = rect(&harness, "x.bin").center();
    let onto = rect(&harness, "file.zip").center();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for step in 1..=10 {
        let at = from + (onto - from) * (step as f32 / 10.0);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(at));
        harness.step();
    }
    assert_eq!(
        harness.state().reordering.map(|_| ()),
        Some(()),
        "the drag did not take the row"
    );
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: onto,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.run_steps(3);
    assert_eq!(order(&harness), ["x.bin", "file.zip", "other.pdf"]);
    assert!(harness.state().reordering.is_none());
}

/// UI round 5: at 200% text every tab of Options stays inside the window
/// and the dialog does not move from one tab to the next; Browser's long
/// rows pushed it off the window's left edge, OK past the right.
#[test]
fn every_options_tab_stays_put_inside_the_window_at_large_text() {
    use dm_desktop::dialogs::TABS;
    use egui_kittest::kittest::By;
    for (zoom, size) in [
        (2.0, egui::vec2(960.0, 660.0)),
        (1.5, egui::vec2(720.0, 460.0)),
        // Shorter still (UI round 6: OK and Cancel lay over the settings).
        (2.0, egui::vec2(720.0, 560.0)),
        (2.0, egui::vec2(720.0, 460.0)),
        (3.0, egui::vec2(1280.0, 800.0)),
    ] {
        let world = world();
        let mut harness = Harness::builder()
            .with_size(size)
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        harness.ctx.set_zoom_factor(zoom);
        harness.run_steps(3);
        harness.state_mut().run(Command::Options);
        harness.run_steps(6);
        let points = size / zoom;
        // Where the buttons are in view: in a window too short for them
        // beside the settings, they come at the end of a page that scrolls.
        let ok_at = |h: &Harness<'_, DmApp>| {
            h.query_by_role_and_label(egui::accesskit::Role::Button, "OK")
                .map(|n| n.rect())
        };
        let first = ok_at(&harness);
        for (tab, label) in TABS {
            if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
                d.tab = tab;
                // A row in each list, so the widest rows are there.
                d.draft.queue_limits.push(Default::default());
                d.draft.host_limits.push(dm_core::HostLimit {
                    host: String::new(),
                    connections: 1,
                });
                d.draft.site_logins.push(Default::default());
            }
            harness.run_steps(6);
            let ok = ok_at(&harness);
            // Put, where the window has room for the buttons beside the
            // settings (the first two sizes); in a shorter one the page
            // scrolls and its end is where each tab's settings end.
            let roomy = size.y >= 660.0 && zoom <= 2.0;
            if let (Some(ok), Some(first), true) = (ok, first, roomy) {
                assert!(
                    (ok.center() - first.center()).length() < 1.0,
                    "{zoom}x {size:?}: OK moved from {first:?} to {ok:?} on {label}"
                );
            }
            // Options' own: a node with the dialog among its ancestors.
            let in_options = |n: &egui_kittest::kittest::AccessKitNode<'_>| {
                let mut up = n.parent();
                while let Some(p) = up {
                    if p.label().as_deref() == Some("Options") {
                        // The tabs scroll sideways by design.
                        let tab = TABS.iter().any(|(_, l)| n.label().as_deref() == Some(*l));
                        // A field's text runs on inside it, clipped there.
                        let inside_a_field = n.role() == egui::accesskit::Role::TextRun;
                        return n.bounding_box().is_some() && !tab && !inside_a_field;
                    }
                    up = p.parent();
                }
                false
            };
            let mut seen = 0;
            for node in harness.query_all(By::new().predicate(in_options)) {
                seen += 1;
                let at = node.rect();
                // Cut by a scroll area is fine; drawn past the window is not.
                assert!(
                    at.left() >= -0.5 && at.right() <= points.x + 0.5,
                    "{zoom}x {size:?}: on {label}, {:?} {:?} (in {:?}) at {at:?} runs past the window",
                    node.accesskit_node().role(),
                    node.accesskit_node().label(),
                    node.accesskit_node().parent().map(|p| (p.role(), p.label()))
                );
            }
            assert!(seen > 5, "{label}: Options' own controls were not found");
            // The first setting above the buttons, not under them (UI round
            // 6: OK and Cancel were drawn over the Language row).
            if let (Some(ok), dm_desktop::dialogs::Tab::General) = (ok, tab) {
                let language = harness
                    .query_all_by_label("Language")
                    .map(|n| n.rect())
                    .next_back()
                    .expect("General's Language");
                assert!(
                    language.bottom() <= ok.top(),
                    "{zoom}x {size:?}: OK at {ok:?} lies over Language at {language:?}"
                );
            }
        }
    }
}

/// UI round 5: at 150% and 200% text in the smallest window, a dialog
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
            // Below the menu bar, which dialogs opened over (UI round 5).
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

/// UI round 6: rows added for later are counted under their kind by their
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
        .map(dm_desktop::app::name_of)
        .collect();
    assert_eq!(shown, ["clip.mp4"]);
}

/// UI round 7: the columns follow the window's width until one is dragged.
/// Kept from the first draw, a list opened narrow and widened left the
/// name at 120 points beside empty room.
#[test]
fn the_columns_follow_the_window_until_one_is_dragged() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    let name_width = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(Role::Button, "Queue").rect().left()
            - h.get_by_role_and_label(Role::Button, "Name").rect().left()
    };
    let narrow = name_width(&harness);
    harness.set_size(egui::vec2(1600.0, 800.0));
    harness.run_steps(4);
    let wide = name_width(&harness);
    assert!(
        wide > narrow + 40.0,
        "the name stayed {narrow} wide, now {wide}"
    );
}

/// UI round 8: Options opens at its top however far down it was left,
/// in a window with room for its buttons and in one where it scrolls as a
/// page.
#[test]
fn options_opens_at_its_top_wherever_it_was_left() {
    use egui::accesskit::Role;
    for (zoom, size) in [
        (1.0, egui::vec2(800.0, 480.0)),
        (2.0, egui::vec2(720.0, 460.0)),
    ] {
        let world = world();
        let mut harness = Harness::builder()
            .with_size(size)
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        harness.ctx.set_zoom_factor(zoom);
        harness.run_steps(3);
        harness.state_mut().run(Command::Options);
        harness.run_steps(6);
        let general = harness
            .get_by_role_and_label(Role::Button, "General")
            .rect();
        // Over what scrolls: the settings below the tabs where they scroll
        // on their own, the tabs themselves where the page scrolls.
        harness.hover_at(if zoom > 1.0 {
            general.center() + egui::vec2(0.0, 12.0)
        } else {
            egui::pos2(general.center().x, general.bottom() + 60.0)
        });
        for _ in 0..15 {
            harness.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -300.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run_steps(1);
        }
        harness.run_steps(3);
        // Below the dialog's title bar, where its scrolled part is seen.
        let language_seen = |h: &Harness<'_, DmApp>| {
            let dialog = h.get_by_role_and_label(Role::Window, "Options").rect();
            // The first thing the page holds: Language where there is room
            // for a row of settings, the tabs where there is not.
            h.query_all_by_label(if zoom > 1.0 { "General" } else { "Language" })
                .map(|n| n.rect())
                .any(|at| at.top() >= dialog.top() + 40.0 && at.bottom() <= size.y / zoom)
        };
        assert!(
            !language_seen(&harness),
            "{zoom}x: the premise, scrolled past Language"
        );
        harness.state_mut().dialogs.options = None;
        harness.run_steps(2);
        harness.state_mut().run(Command::Options);
        harness.run_steps(6);
        assert!(
            language_seen(&harness),
            "{zoom}x {size:?}: reopened scrolled"
        );
    }
}

/// UI round 8: a right-click in a text field offers Cut, Copy and Paste,
/// and Paste puts the clipboard in the field.
#[test]
fn a_text_field_has_cut_copy_and_paste_on_a_right_click() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    let link = world.url("file.zip");
    *world.fake.clipboard.lock().unwrap() = Some(link.clone());
    let field = harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .rect();
    harness.hover_at(field.center());
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: field.center(),
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
        harness.step();
    }
    harness.run_steps(2);
    assert!(
        harness.query_by_label("Cut").is_some(),
        "no menu on a right-click"
    );
    assert!(harness.query_by_label("Copy").is_some());
    click(&mut harness, "Paste");
    harness.run_steps(3);
    let typed = harness
        .state()
        .dialogs
        .add_url
        .as_ref()
        .map(|d| d.address.clone());
    assert_eq!(typed.as_deref(), Some(link.as_str()));
    assert!(harness.query_by_label("Cut").is_none(), "the menu stayed");

    // Cut acts on what was selected before the right-click moved the
    // caret (UI round 9: Cut and Copy did nothing).
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(2);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: field.center(),
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
        harness.step();
    }
    harness.run_steps(2);
    click(&mut harness, "Cut");
    harness.run_steps(3);
    let left = harness
        .state()
        .dialogs
        .add_url
        .as_ref()
        .map(|d| d.address.clone());
    assert_eq!(left.as_deref(), Some(""), "Cut left the text");

    // Escape closes the menu, and leaves the dialog open.
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: field.center(),
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
        harness.step();
    }
    harness.run_steps(2);
    assert!(harness.query_by_label("Paste").is_some());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Paste").is_none(),
        "Escape left the menu"
    );
    assert!(
        harness.state().dialogs.add_url.is_some(),
        "Escape closed the dialog too"
    );
}

/// UI round 10: a progress window opens below the toolbar, not over Add
/// URL; and Download complete waits while the Exit question is asked,
/// rather than opening over it with Open folder under the pointer.
#[test]
fn windows_a_download_opens_keep_clear_of_the_toolbar_and_questions() {
    let world = world();
    // The usual window's size, where a tall progress window was pushed back
    // up over the toolbar to fit (UI round 11).
    let mut harness = Harness::builder()
        .with_size(egui::vec2(960.0, 660.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    for _ in 0..2 {
        harness
            .state()
            .backend()
            .add(&world.url("stalled.bin"))
            .unwrap();
    }
    until(&mut harness, "both to run", |app| {
        app.rows()
            .iter()
            .filter(|r| r.state == State::Running)
            .count()
            == 2
    });
    let ids: Vec<_> = harness.state().rows().iter().map(|r| r.id).collect();
    harness.state_mut().dialogs.progress.extend(ids);
    harness.run_steps(3);
    let add = harness.get_by_label("Add URL").rect();
    let windows: Vec<egui::Rect> = harness
        .query_all_by_label_contains("Download progress")
        .map(|n| n.rect())
        .collect();
    assert_eq!(windows.len(), 2);
    for window in &windows {
        assert!(
            window.top() > add.bottom(),
            "{window:?} over Add URL at {add:?}"
        );
    }
    assert!(
        (windows[0].top() - windows[1].top()).abs() > 10.0,
        "they do not cascade: {windows:?}"
    );

    harness.state_mut().dialogs.progress.clear();
    harness.state_mut().applied.show_complete = true;
    harness.state_mut().dialogs.confirm_exit = true;
    harness
        .state_mut()
        .add(&[world.url("file.zip")], Default::default())
        .unwrap();
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Download complete").is_none(),
        "opened over the Exit question"
    );
    harness.state_mut().dialogs.confirm_exit = false;
    harness.run_steps(3);
    assert!(harness.query_by_label("Download complete").is_some());
}

/// UI round 10: Download details says the name a file will really get
/// where its folder has one of that name already, and that the server has
/// no file at an address, before Start now rather than after.
#[test]
fn download_details_says_the_name_it_will_get_and_a_server_refusal() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    if harness.query_by_label("Already in the list").is_some() {
        click(&mut harness, "Download again");
    }
    let said = "A file of that name is in the folder already; this one will be file (1).zip.";
    for _ in 0..50 {
        if harness.query_by_label(said).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        harness.run_steps(1);
    }
    assert!(
        harness.query_by_label(said).is_some(),
        "the renaming was not said"
    );
    click(&mut harness, "Cancel");

    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("missing.bin"));
    click(&mut harness, "OK");
    let refused = "Not available: the server has no file at this address (404).";
    for _ in 0..100 {
        if harness.query_by_label(refused).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        harness.run_steps(1);
    }
    assert!(
        harness.query_by_label(refused).is_some(),
        "the 404 was not said"
    );

    // The first copy still under way, not yet on disk: said as well.
    click(&mut harness, "Cancel");
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("stalled.bin"));
    click(&mut harness, "OK");
    if harness.query_by_label("Already in the list").is_some() {
        click(&mut harness, "Download again");
    }
    let pending = "Another download in the list has this name; this one will be given a number.";
    for _ in 0..100 {
        if harness.query_by_label(pending).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        harness.run_steps(1);
    }
    assert!(
        harness.query_by_label(pending).is_some(),
        "the numbering was not said"
    );
}

/// UI round 11: behind Options the toolbar and the places take no clicks,
/// and a progress window takes none under a question.
#[test]
fn the_window_behind_a_dialog_takes_no_clicks() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("stalled.bin"))
        .unwrap();
    until(&mut harness, "it to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    click(&mut harness, "Options");
    harness
        .get_by_role_and_label(Role::Button, "Add URL")
        .click();
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.add_url.is_none(),
        "Add URL opened behind Options"
    );
    let before = harness.state().place.clone();
    harness.get_by_label("Done (0)").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().place,
        before,
        "a place was picked behind Options"
    );
    // A progress window behind Options takes none either (UI round 12).
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    harness.run_steps(3);
    harness.get_by_label("Stop download").click();
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert_eq!(
        harness.state().rows()[0].state,
        State::Running,
        "Stop download worked behind Options"
    );
    click(&mut harness, "Cancel");

    harness.state_mut().dialogs.confirm_exit = true;
    harness.run_steps(3);
    harness.get_by_label("Stop download").click();
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert_eq!(
        harness.state().rows()[0].state,
        State::Running,
        "Stop download worked under the Exit question"
    );
}

/// UI round 11: the columns start where they did at the last launch
/// whether or not a row has a queue yet; Queue started narrower in an
/// empty list, so a relaunch moved Size and the rest and cut "Time left".
#[test]
fn the_columns_start_in_one_place_whatever_the_rows_queues() {
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(3);
    let before = harness.get_by_label("Size").rect().left();
    add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    assert!(harness.state().rows().iter().any(|r| !r.queue.is_empty()));
    // A launch: nothing of the list's widths remembered, no column dragged.
    harness.ctx.memory_mut(|m| m.data.clear());
    harness.run_steps(3);
    let after = harness.get_by_label("Size").rect().left();
    assert!(
        (before - after).abs() < 1.0,
        "Size moved from {before} to {after}"
    );
}

/// UI round 11: Properties of a row not yet started says its kind and
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

/// UI round 11: Find has a Close button, as every other dialog has.
#[test]
fn find_closes_with_its_close_button() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::Find);
    harness.run_steps(3);
    assert!(harness.state().dialogs.find_open);
    click(&mut harness, "Close");
    assert!(!harness.state().dialogs.find_open, "Close left Find open");
}

/// UI round 12: copies of one file added for later are each named as they
/// will be saved, not all "other (1).pdf".
#[test]
fn copies_waiting_in_the_list_each_get_their_own_name() {
    let world = world();
    let mut harness = world.harness();
    for _ in 0..3 {
        add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    }
    harness.run_steps(3);
    for name in ["other.pdf", "other (1).pdf", "other (2).pdf"] {
        assert!(
            harness.query_by_label(name).is_some(),
            "no row reads {name}"
        );
    }
}

/// UI round 12: Export to a folder that is not there keeps its dialog
/// open, with the path as typed and why, rather than closing and saying
/// it only in the status bar.
#[test]
fn a_list_that_cannot_be_written_keeps_its_dialog_open() {
    let world = world();
    let mut harness = world.harness();
    let path = world.dir.path().join("no such folder").join("list.dmlist");
    harness.state_mut().dialogs.list_file = Some(dm_desktop::dialogs::ListFile {
        save: true,
        path: path.display().to_string(),
        says: String::new(),
    });
    harness.run_steps(3);
    click(&mut harness, "OK");
    let d = harness.state().dialogs.list_file.as_ref();
    assert!(d.is_some(), "the dialog closed on a failure");
    let d = d.unwrap();
    assert_eq!(d.path, path.display().to_string());
    assert!(d.says.contains("could not be written"), "{}", d.says);
}

/// UI round 12: a row whose kind is not known yet is under Other, so the
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

/// UI round 12: a queue has one limit; Add picks a queue without one and
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
        harness.get_by_label("Add a queue limit").click();
        harness.run_steps(2);
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

/// UI round 13: Escape cancels the When-done countdown, even with Download
/// complete open behind it, and the keyboard starts on Cancel.
#[test]
fn escape_cancels_the_when_done_countdown() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.state_mut().countdown = Some((
        dm_desktop::power::WhenDone::Sleep,
        std::time::Instant::now() + std::time::Duration::from_secs(30),
    ));
    harness.run_steps(3);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Cancel")
            .accesskit_node()
            .is_focused(),
        "the keyboard is not on Cancel"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(
        harness.state().countdown.is_none(),
        "Escape did not cancel it"
    );
    assert!(
        !world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.contains("sleep")),
        "it slept"
    );
}

/// UI round 13: Import of a plain list says how many lines it skipped for
/// having no address dm can read, rather than passing over them.
#[test]
fn import_says_the_lines_it_could_not_read() {
    let world = world();
    let mut harness = world.harness();
    let text = format!(
        "{}\n127.0.0.1:8000/notes.txt\n# a comment\n\n{}\n",
        world.url("other.pdf"),
        world.url("deep.pdf")
    );
    harness.state_mut().import_text(&text, false);
    harness.run_steps(2);
    let said = harness.state().said.shown();
    assert!(said.starts_with("Imported: 2."), "{said}");
    assert!(said.ends_with(": 1."), "{said}");
}

/// UI round 14: a download refused for want of a sign-in is retried with
/// one saved in Options afterwards, as a fresh copy of the address was.
#[test]
fn retry_uses_a_sign_in_saved_since_it_failed() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("locked.zip"))
        .unwrap();
    until(&mut harness, "it to be refused", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let mut wanted = harness.state().backend().settings();
    wanted.site_logins = vec![dm_shell::settings::SiteLogin {
        host: "127.0.0.1".into(),
        username: "me".into(),
        password: test_secrets::secret("password"),
    }];
    harness.state_mut().apply_settings(wanted).unwrap();
    let id = harness.state().rows()[0].id;
    harness.state().backend().retry(id);
    until(
        &mut harness,
        "it to complete with the saved sign-in",
        |app| app.rows().iter().any(|r| r.state == State::Completed),
    );
    let mut back = harness.state().backend().settings();
    back.site_logins.clear();
    harness.state_mut().apply_settings(back).unwrap();
}

/// UI round 14: several links copied at once are all offered, through the
/// dialog that lists them; only the first was, and the rest dropped.
#[test]
fn the_clipboard_watcher_offers_every_link_copied_together() {
    let _file = settings_file();
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut harness = world.harness();
    let mut on = harness.state().backend().settings();
    on.watch_clipboard = true;
    harness.state_mut().apply_settings(on).unwrap();
    let links = [
        world.url("file.zip"),
        world.url("other.pdf"),
        world.url("deep.pdf"),
    ];
    *world.fake.clipboard.lock().unwrap() = Some(links.join("\n"));
    harness.state_mut().check_clipboard();
    harness.run_steps(3);
    assert!(harness
        .query_by_label("Links on your clipboard: 3")
        .is_some());
    click(&mut harness, "Download…");
    let offered: Vec<String> = harness
        .state()
        .dialogs
        .clip_links
        .as_ref()
        .expect("the links dialog did not open")
        .iter()
        .map(|l| l.0.clone())
        .collect();
    assert_eq!(offered, links);
    let mut off = harness.state().backend().settings();
    off.watch_clipboard = false;
    harness.state_mut().apply_settings(off).unwrap();
}

/// UI round 14: where the places give way at large text, a drop-down
/// above the list reaches them; nothing did.
#[test]
fn the_places_are_reachable_when_they_give_way_at_large_text() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    harness.ctx.set_zoom_factor(2.0);
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Everything").is_none(),
        "the places did not give way"
    );
    harness
        .get_by_role_and_label(Role::ComboBox, "Place")
        .click();
    harness.run_steps(3);
    // One in view of the drop-down at this size; the rest scroll.
    harness.get_by_label("Videos (0)").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().place,
        dm_desktop::app::Place::Kind(dm_core::Category::Videos)
    );
}

/// UI round 15: at large text, with the Place drop-down above the list,
/// the last rows could never be scrolled into view.
#[test]
fn the_last_row_can_be_reached_at_large_text() {
    last_row_reached(false);
    // And with the clipboard's bar above the list (UI round 18).
    last_row_reached(true);
}

fn last_row_reached(clipboard_bar: bool) {
    let world = world();
    // A maximized window, where it was seen.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    let links: Vec<String> = (0..34)
        .map(|i| world.url(&format!("stalled-{i:02}.bin")))
        .collect();
    harness.state().backend().pause_all();
    harness.state().backend().add(&links.join(" ")).unwrap();
    harness.state().backend().pause_all();
    harness.state_mut().refresh();
    if clipboard_bar {
        harness.state_mut().clipboard.offered = Some(world.url("offered.zip"));
    }
    harness.ctx.set_zoom_factor(3.0);
    harness.run_steps(3);
    let last = harness.state().rows().last().unwrap().id;
    harness.state_mut().scroll_to = Some(last);
    harness.run_steps(4);
    let name = dm_desktop::app::name_of(harness.state().rows().last().unwrap());
    let screen = harness.ctx.content_rect();
    let row = harness
        .query_all_by_label(&name)
        .map(|n| n.rect())
        .find(|r| r.width() > 0.0)
        .unwrap_or_else(|| panic!("{name} was never drawn"));
    assert!(
        row.bottom() <= screen.bottom() + 0.5 && row.top() >= screen.top(),
        "{name} at {row:?} outside the window {screen:?}"
    );
    // And whole above the list's sideways bar, which sits on the status
    // bar: the row ended under it at 300% (UI round 16).
    let status = harness
        .query_all_by_label_contains("Nothing downloading")
        .map(|n| n.rect())
        .next()
        .expect("the status bar");
    let bar = 8.0;
    assert!(
        row.bottom() + bar <= status.top(),
        "{name} at {row:?} runs under the sideways bar above the status bar at {status:?}"
    );
}

/// UI round 15: a new queue's name typed in Move to queue is the name, not
/// "Main queueNight" after the picked queue's name standing in the field.
#[test]
fn move_to_queue_makes_the_queue_named() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    select(&mut harness, "other.pdf");
    harness.state_mut().run(Command::MoveToQueue);
    harness.run_steps(3);
    type_into(&mut harness, "Or a new queue", "Night");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows()[0].queue, "Night");
}

/// UI round 16: at 230% in the smallest window the toolbar wrapped to
/// three rows and no download showed; it gives way and a row shows.
#[test]
fn a_row_shows_at_230_percent_in_the_smallest_window() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    harness
        .state()
        .backend()
        .add(&world.url("other.pdf"))
        .unwrap();
    harness.state_mut().refresh();
    harness.ctx.set_zoom_factor(2.3);
    harness.run_steps(4);
    let screen = harness.ctx.content_rect();
    // By the name the list gives it, which may be numbered.
    let row = harness
        .query_all_by_label_contains("other")
        .map(|n| n.rect())
        .find(|r| r.height() > 0.0)
        .expect("no row drawn");
    assert!(row.bottom() <= screen.bottom(), "{row:?} below {screen:?}");
}

/// UI round 16: removing finished and unfinished rows together says so,
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
        .query_by_label_contains("Some of these are finished and some are not")
        .is_some());
    assert!(harness
        .query_by_label("The finished one's file stays where it is.")
        .is_some());
    assert!(harness
        .query_by_label_contains("have not fetched anything yet")
        .is_none());
}

/// UI round 17: a .torrent on this computer is added by its path or its
/// file address in Add URL; only a drop took it.
#[test]
fn add_url_takes_a_torrent_on_this_computer() {
    let world = world();
    let mut harness = world.harness();
    let file = world.dir.path().join("a set.torrent");
    std::fs::write(&file, b"d4:infodee").unwrap();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", file.to_str().unwrap());
    click(&mut harness, "OK");
    let urls = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .map(|d| d.urls.clone())
        .expect("Download details did not open");
    assert_eq!(urls, [dm_core::torrent::file_url(&file).unwrap()]);
}

/// UI round 17: a long link on the clipboard is cut short, and its
/// Download and Dismiss stay in the window.
#[test]
fn the_clipboard_bar_keeps_its_buttons_in_the_window() {
    let _file = settings_file();
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut harness = world.harness();
    let mut on = harness.state().backend().settings();
    on.watch_clipboard = true;
    harness.state_mut().apply_settings(on).unwrap();
    let long = world.url(&format!("{}.zip", "a-very-long-name".repeat(8)));
    *world.fake.clipboard.lock().unwrap() = Some(long);
    harness.state_mut().check_clipboard();
    harness.run_steps(3);
    let screen = harness.ctx.content_rect();
    for name in ["Download", "Dismiss"] {
        let button = harness.get_by_label(name).rect();
        assert!(
            button.right() <= screen.right(),
            "{name} at {button:?} past {screen:?}"
        );
    }
    let mut off = harness.state().backend().settings();
    off.watch_clipboard = false;
    harness.state_mut().apply_settings(off).unwrap();
}

/// Speed check, 2026-10-19: the progress window was held at the room on
/// the screen, 84% of the window's height for three connections. It is as
/// tall as what it holds, its connections shut until asked for.
#[test]
fn the_progress_window_is_as_tall_as_what_it_holds() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("stalled.bin"))
        .unwrap();
    until(&mut harness, "it to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    harness.run_steps(4);
    let window = harness
        .query_all_by_label_contains("Download progress")
        .map(|n| n.rect())
        .next()
        .expect("no progress window");
    let screen = harness.ctx.content_rect();
    assert!(
        window.height() < screen.height() * 0.5,
        "{window:?} is {:.0}% of the window's height",
        100.0 * window.height() / screen.height()
    );
}

/// Gap 71: Escape closes the window in front, not one behind it. Download
/// complete and a progress window stay open side by side; Escape closed
/// Download complete whichever was in front.
#[test]
fn escape_closes_the_window_in_front() {
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(3);
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
    let progress = egui::LayerId::new(egui::Order::Middle, egui::Id::unique(("progress", id)));
    let complete = egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::unique(("dm-dialog", "Download complete")),
    );
    for front in [progress, complete] {
        harness.state_mut().dialogs.progress.insert(id);
        harness.state_mut().dialogs.complete.insert(id);
        harness.run_steps(3);
        harness.ctx.move_to_top(front);
        harness.run_steps(2);
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        let dialogs = &harness.state().dialogs;
        let (progress_open, complete_open) =
            (dialogs.progress.contains(&id), !dialogs.complete.is_empty());
        if front == progress {
            assert_eq!(
                (progress_open, complete_open),
                (false, true),
                "progress was in front"
            );
        } else {
            assert_eq!(
                (progress_open, complete_open),
                (true, false),
                "complete was in front"
            );
        }
        harness.state_mut().dialogs = Default::default();
        harness.run_steps(2);
    }
}

/// Gap 71: Properties opens in the middle, as the other dialogs do; it
/// opened in the window's top left corner.
#[test]
fn properties_open_in_the_middle() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .add(
            &[world.url("file.zip")],
            dm_core::AddOptions {
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
    let title = harness.get_by_label("File properties").rect();
    // The title is at the dialog's top left; the dialog is about 620 wide.
    assert!(
        title.left() > 150.0,
        "Properties opened at the left: {title:?}"
    );
    assert!(
        title.top() > 100.0,
        "Properties opened at the top: {title:?}"
    );
}

/// Gap 71: at 150% text in the window's first size, Options' tabs all fit
/// inside the dialog; Sign-ins stood 19 points past its edge.
#[test]
fn options_tabs_fit_at_150_percent() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(960.0, 660.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    harness.ctx.set_zoom_factor(1.5);
    harness.run_steps(3);
    harness.state_mut().run(Command::Options);
    harness.run_steps(6);
    let edge = harness.get_by_label("Close window").rect().right();
    let first = harness
        .get_by_role_and_label(Role::Button, "General")
        .rect();
    let last = harness
        .get_by_role_and_label(Role::Button, "Sign-ins")
        .rect();
    assert!(
        last.right() <= edge + 0.5,
        "Sign-ins ends at {} past {edge}",
        last.right()
    );
    assert!(first.left() >= 40.0, "General moved: {first:?}");
}

/// A stream server's address goes in through Add URL like any other, is
/// named as it will be saved, and is queued; ffmpeg fetches it (the engine's
/// `tests/live.rs`).
#[test]
fn a_stream_servers_address_is_taken_through_add_url() {
    let world = world();
    let mut harness = world.harness();
    add_url(
        &mut harness,
        "rtmp://127.0.0.1:1/live/show",
        "Add for later",
    );
    harness.state_mut().refresh();
    harness.run_steps(3);
    let rows = harness.state().rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].url, "rtmp://127.0.0.1:1/live/show");
    assert!(harness.query_by_label_contains("show").is_some());
    assert!(dm_desktop::app::looks_downloadable("rtmp://h/live/x").is_some());
    assert!(dm_desktop::app::looks_downloadable("mms://h/clip.asf").is_some());
}

/// Letters typed into a number box leave its number as it was, and the box
/// says so once it is left, rather than appearing to have taken them (UI
/// round 14: "abc" in a size floor silently stayed 0).
#[test]
fn a_number_box_says_when_what_was_typed_is_not_a_number() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Network");
    let label = "Give up on a connection that sends nothing for, seconds";
    let before = harness
        .state()
        .dialogs
        .options
        .as_ref()
        .map(|d| d.draft.read_timeout)
        .unwrap();
    harness.get_by_label(label).click();
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label(label).type_text("abc");
    harness.run_steps(2);
    assert!(
        harness
            .query_by_label("Numbers only: the one before was kept")
            .is_none(),
        "said while still typing"
    );
    harness.key_press(egui::Key::Tab);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Numbers only: the one before was kept")
            .is_some(),
        "nothing said about the letters"
    );
    let after = harness
        .state()
        .dialogs
        .options
        .as_ref()
        .map(|d| d.draft.read_timeout)
        .unwrap();
    assert_eq!(after, before, "the number changed");
    // Typed in again, the word goes.
    harness.get_by_label(label).click();
    harness.run_steps(3);
    assert!(harness
        .query_by_label("Numbers only: the one before was kept")
        .is_none());
}

/// Someone who installed only the app is shown how to add dm to their
/// browser, one click from its store page, until a browser connects or
/// they say not now.
#[test]
fn a_new_user_is_shown_how_to_add_dm_to_their_browser() {
    let _file = settings_file();
    let seen = home().join("browser.seen");
    let hidden = home().join("browser-hint.hidden");
    let _ = std::fs::remove_file(&seen);
    let _ = std::fs::remove_file(&hidden);
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(2);
    click(&mut harness, "Microsoft Edge");
    assert!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o == dm_desktop::app::EXTENSION_EDGE),
        "{:?}",
        world.fake.opened.lock().unwrap()
    );
    click(&mut harness, "Not now");
    assert!(harness.query_by_label("Microsoft Edge").is_none());
    assert!(hidden.exists(), "not now is remembered");

    // A browser that has connected: nothing offered, from the start.
    std::fs::remove_file(&hidden).unwrap();
    std::fs::write(&seen, b"").unwrap();
    let mut harness = world.harness();
    harness.run_steps(2);
    let offered = harness.query_by_label("Microsoft Edge").is_some();
    std::fs::remove_file(&seen).unwrap();
    assert!(!offered, "offered to someone whose browser has connected");
}

/// A failure is announced as a finish is, saying why in plain words; it was
/// only a sound, if that was on (QA checklist UX-09).
#[test]
fn a_failed_download_is_announced_with_why() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(3);
    let told = world.fake.notified.lock().unwrap().clone();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("secret.zip — "), "{told:?}");
    assert!(!told[0].contains("http"), "{told:?}");
}

/// Download complete can be turned off from itself, as IDM's "Don't show
/// this dialog again", and offers Open with beside Open (QA checklist UX-07).
#[test]
fn download_complete_offers_open_with_and_not_again() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    assert!(harness.query_by_label("Open with…").is_some());
    click(&mut harness, "Don't show this window again");
    harness.run_steps(2);
    assert!(harness.query_by_label("Download complete").is_none());
    assert!(
        !harness.state().applied.show_complete,
        "the setting stays on"
    );
    // Put back for the tests after.
    let mut wanted = harness.state().applied.clone();
    wanted.show_complete = true;
    harness.state_mut().apply_settings(wanted).unwrap();
}

/// The tray says how many downloads run and how fast; nothing to say when
/// none do (QA checklist UX-08).
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
