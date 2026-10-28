//! What every window test stands on: the fake operating system, the world a
//! test runs in (a real queue fetching from a loopback server), and the
//! helpers that click, type and wait.

pub use axum::body::Body;
pub use axum::response::Response;
pub use dm_core::{Queue, State};
pub use dm_desktop::app::{Column, DmApp, Place};
pub use dm_desktop::backend::{Backend, Platform};
pub use dm_desktop::commands::Command;
pub use dm_shell::Settings;
pub use egui_kittest::kittest::{NodeT, Queryable};
pub use egui_kittest::Harness;
pub use std::net::SocketAddr;
pub use std::sync::{Arc, Mutex, OnceLock};
pub use std::time::{Duration, Instant};

/// Held by every test that writes the settings file: they share one, and a
/// test reading back what it wrote must not read what another wrote.
pub(crate) static SETTINGS_FILE: Mutex<()> = Mutex::new(());

thread_local! {
    /// Whether this test's thread holds [`SETTINGS_FILE`].
    static HOLDS_SETTINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// [`SETTINGS_FILE`], held until dropped, and known to be held on this
/// thread, so a helper that writes the file too can take it only when the
/// test has not: Add URL saves its recent addresses there, and a test
/// reading the file back read another's.
pub(crate) struct SettingsFile(Option<std::sync::MutexGuard<'static, ()>>);

impl Drop for SettingsFile {
    fn drop(&mut self) {
        if self.0.take().is_some() {
            HOLDS_SETTINGS.with(|h| h.set(false));
        }
    }
}

pub(crate) fn settings_file() -> SettingsFile {
    if HOLDS_SETTINGS.with(std::cell::Cell::get) {
        return SettingsFile(None);
    }
    let guard = SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner());
    HOLDS_SETTINGS.with(|h| h.set(true));
    SettingsFile(Some(guard))
}

/// Every test's settings file and queue live here, never in the real home.
pub(crate) fn home() -> &'static std::path::Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: set once (`OnceLock`), before any test reads it: they all reach
        // the home through this function.
        unsafe { std::env::set_var("DM_HOME", dir.path()) };
        // Sizes and dates as the language writes them, on any machine.
        dm_desktop::region::pin(None);
        dir
    })
    .path()
}

#[derive(Default)]
pub(crate) struct Fake {
    pub(crate) notified: Mutex<Vec<String>>,
    pub(crate) clipboard: Mutex<Option<String>>,
    pub(crate) login_fails: Mutex<bool>,
    pub(crate) power_fails: Mutex<bool>,
    pub(crate) login: Mutex<Option<bool>>,
    pub(crate) opened: Mutex<Vec<String>>,
    pub(crate) chosen: Mutex<Option<std::path::PathBuf>>,
    /// A desktop with no folder chooser to ask with.
    pub(crate) no_chooser: Mutex<bool>,
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
    fn trash(&self, path: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(format!("trash {path}"));
        std::fs::remove_file(path).map_err(|e| e.to_string())
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

pub(crate) struct World {
    pub(crate) runtime: tokio::runtime::Runtime,
    pub(crate) fake: Arc<Fake>,
    pub(crate) dir: tempfile::TempDir,
    pub(crate) addr: SocketAddr,
}

pub(crate) const PAYLOAD: usize = 256 * 1024;

pub(crate) fn payload() -> Vec<u8> {
    (0..PAYLOAD).map(|i| (i * 7 % 251) as u8).collect()
}

/// A server with one file that arrives at once and one that never does.
pub(crate) fn world() -> World {
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
                    // Named by its server, not by its address.
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
        // A video that sends its first 64 KiB and then nothing.
        let app = app.route(
            "/half.mp4",
            axum::routing::get(|| async {
                let first = futures_util::stream::iter([Ok::<Vec<u8>, std::io::Error>(
                    vec![9u8; 64 * 1024],
                )]);
                let never = futures_util::stream::pending();
                Response::builder()
                    .header("content-length", "1000000")
                    .body(Body::from_stream(futures_util::StreamExt::chain(
                        first, never,
                    )))
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
        // A signed link that runs out part-way: asked from the start, it
        // sends a fifth of a million bytes and is cut (more than the 64 KiB
        // a resume fetches again to check); asked to go on from part-way,
        // it has expired.
        let app = app.route(
            "/expires.bin",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                let from_start = headers
                    .get("range")
                    .and_then(|r| r.to_str().ok())
                    .is_none_or(|r| r.starts_with("bytes=0-"));
                if !from_start {
                    return Response::builder()
                        .status(403)
                        .body(Body::from("Request has expired"))
                        .unwrap();
                }
                // The engine's first look: one byte, and the size.
                if headers.get("range").is_some_and(|r| r == "bytes=0-0") {
                    return Response::builder()
                        .status(206)
                        .header("content-range", "bytes 0-0/1000000")
                        .header("accept-ranges", "bytes")
                        .body(Body::from(vec![7u8]))
                        .unwrap();
                }
                use futures_util::StreamExt;
                // The bytes first, and the cut a moment after, as a
                // connection that drops does.
                let some = futures_util::stream::iter([true, false]).then(|bytes| async move {
                    if bytes {
                        Ok::<Vec<u8>, std::io::Error>(vec![7u8; 200_000])
                    } else {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        Err(std::io::Error::other("cut"))
                    }
                });
                Response::builder()
                    .status(if headers.contains_key("range") { 206 } else { 200 })
                    .header("content-length", "1000000")
                    .header("content-range", "bytes 0-999999/1000000")
                    .header("accept-ranges", "bytes")
                    .body(Body::from_stream(some))
                    .unwrap()
            }),
        );
        // The same file at a fresh link, whole and in ranges.
        let app = app.route(
            "/renewed.bin",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                const SIZE: usize = 1_000_000;
                let range = headers
                    .get("range")
                    .and_then(|r| r.to_str().ok())
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.split_once('-'))
                    .and_then(|(a, b)| {
                        let a: usize = a.parse().ok()?;
                        let b: usize = b.parse().unwrap_or(SIZE - 1).min(SIZE - 1);
                        (a <= b).then_some((a, b))
                    });
                match range {
                    Some((a, b)) => Response::builder()
                        .status(206)
                        .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                        .header("accept-ranges", "bytes")
                        .body(Body::from(vec![7u8; b - a + 1]))
                        .unwrap(),
                    None => Response::builder()
                        .header("accept-ranges", "bytes")
                        .body(Body::from(vec![7u8; SIZE]))
                        .unwrap(),
                }
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
    pub(crate) fn url(&self, name: &str) -> String {
        format!("http://{}/{name}", self.addr)
    }

    pub(crate) fn app(&self) -> DmApp {
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
            // Files straight into the folder and no login item, as the tests
            // were written; a new install has both on (`new_installs_sort_by_kind_and_start_at_login`).
            categorize: false,
            start_on_login: false,
            ..Settings::default()
        })
    }

    /// A window over these settings, as a launch that read them would be.
    pub(crate) fn app_with(&self, settings: Settings) -> DmApp {
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

    pub(crate) fn harness(&self) -> Harness<'static, DmApp> {
        Harness::builder()
            .with_size(egui::vec2(1100.0, 720.0))
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), self.app())
    }

    /// A harness with 20 ms frames, so two clicks can land inside egui's
    /// 0.3 s double-click window (each input event takes a frame).
    pub(crate) fn quick_harness(&self) -> Harness<'static, DmApp> {
        Harness::builder()
            .with_size(egui::vec2(1100.0, 720.0))
            .with_step_dt(0.02)
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), self.app())
    }
}

/// Runs frames until `done` says so, or fails after a few seconds.
pub(crate) fn until(harness: &mut Harness<'_, DmApp>, what: &str, done: impl Fn(&DmApp) -> bool) {
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
pub(crate) fn double_click(harness: &mut Harness<'_, DmApp>, name: &str) {
    harness.run_steps(40);
    harness.get_by_label(name).click();
    harness.get_by_label(name).click();
    harness.run_steps(2);
}

/// Clicks something by its name and lets the window catch up.
pub(crate) fn click(harness: &mut Harness<'_, DmApp>, name: &str) {
    if harness.state().dialogs.file_info.is_some() {
        settle_details(harness);
    }
    // Scrolled to first, as a person scrolls to what is below the fold: how
    // far down a row is depends on the computer's type.
    harness.get_by_label(name).scroll_to_me();
    harness.run_steps(3);
    harness.get_by_label(name).click();
    harness.run_steps(3);
}

/// Types into a field found by its name.
pub(crate) fn type_into(harness: &mut Harness<'_, DmApp>, name: &str, text: &str) {
    harness.get_by_label(name).scroll_to_me();
    harness.run_steps(3);
    harness.get_by_label(name).click();
    harness.run_steps(2);
    harness.get_by_label(name).type_text(text);
    harness.run_steps(2);
}

/// Types into the text field of that name, where a column heading or a
/// label shares it.
pub(crate) fn type_role(harness: &mut Harness<'_, DmApp>, name: &str, text: &str) {
    use egui::accesskit::Role;
    harness
        .get_by_role_and_label(Role::TextInput, name)
        .scroll_to_me();
    harness.run_steps(3);
    harness.get_by_role_and_label(Role::TextInput, name).click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::TextInput, name)
        .type_text(text);
    harness.run_steps(2);
}

/// Add URL, the address, OK, then the File Info dialog's button.
pub(crate) fn add_url(harness: &mut Harness<'_, DmApp>, url: &str, button: &str) {
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
pub(crate) fn settle_details(harness: &mut Harness<'_, DmApp>) {
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
pub(crate) fn select(harness: &mut Harness<'_, DmApp>, name: &str) {
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

pub(crate) fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// Waits for Exit's countdown, says it has not quit yet, then runs it out.
pub(crate) fn quit_after_its_countdown(harness: &mut Harness<'_, DmApp>) {
    until(harness, "Exit's countdown", |app| {
        app.countdown
            .is_some_and(|(action, _)| action == dm_desktop::power::WhenDone::Quit)
    });
    assert!(!harness.state().quit_asked, "quit before its countdown");
    harness.state_mut().countdown =
        Some((dm_desktop::power::WhenDone::Quit, std::time::Instant::now()));
    until(harness, "dm to quit", |app| app.quit_asked);
}

/// Opens a menu path by clicking each name on screen, as a person does:
/// the top menu by its exact name, a submenu by the name its arrow follows,
/// the item by its exact name. The last match is the one in the open menu,
/// above the toolbar button that may share its name. Closes what is left.
pub(crate) fn press(h: &mut Harness<'_, DmApp>, path: &[&str]) {
    // What the test closed by hand is gone from the screen first.
    h.run_steps(2);
    for (i, name) in path.iter().enumerate() {
        let found = if i == 0 {
            h.query_all_by_label(name).last()
        } else if i + 1 == path.len() {
            // A menu item with a key says it after its name, "Help F1"; a
            // key never starts lowercase, so "Find" is not "Find next".
            // A toolbar button of the same name (44 px tall, picture over
            // words) may lie under the open menu; the menu's item, 32
            // tall, is the one meant.
            let item = |n: &egui_kittest::Node<'_>| {
                n.accesskit_node()
                    .bounding_box()
                    .is_some_and(|b| b.height() <= 33.0)
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
pub(crate) fn greyed(h: &mut Harness<'_, DmApp>, path: &[&str]) -> bool {
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

pub(crate) fn row(h: &Harness<'_, DmApp>, name: &str) -> dm_core::Entry {
    h.state()
        .rows()
        .iter()
        .find(|r| dm_desktop::app::name_of(r) == name)
        .unwrap_or_else(|| panic!("no row {name}"))
        .clone()
}

/// The bounding box of the last node of that name, in window points.
pub(crate) fn bounds(h: &Harness<'_, DmApp>, name: &str) -> egui::accesskit::Rect {
    h.query_all_by_label(name)
        .last()
        .unwrap_or_else(|| panic!("{name:?} is not on screen"))
        .accesskit_node()
        .bounding_box()
        .unwrap_or_else(|| panic!("{name:?} has no place on screen"))
}

/// Adds a row shown by that name (its address's last part), for later,
/// without fetching anything.
pub(crate) fn add_named(harness: &mut Harness<'_, DmApp>, world: &World, name: &str) {
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
