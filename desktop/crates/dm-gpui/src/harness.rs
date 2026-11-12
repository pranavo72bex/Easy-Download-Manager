// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The GPUI window opened in GPUI's test platform, for the window's tests: driven through the
//! control port's methods (`ui.click`, `ui.texts`, `command.run`…), as a script drives it.

use std::sync::{mpsc, Arc};

use gpui::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use serde_json::Value;

use crate::list::Downloads;
use dm_desktop_base::boot::Wake;

pub(crate) mod world;

/// Every test's settings file and queue live here, never in the real home.
pub(crate) fn home() -> &'static std::path::Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: set once (`OnceLock`), before any test reads it: they all reach the home
        // through this function.
        unsafe { std::env::set_var("DM_HOME", dir.path()) };
        // And no secrets left in the person's own store by a throw-away home.
        // SAFETY: as `DM_HOME` above, set once before any test reads it.
        unsafe { std::env::set_var("DM_NO_SECRET_STORE", "1") };
        // Sizes and dates as the language writes them, on any machine.
        dm_desktop_base::region::pin(None);
        dir
    })
    .path()
}

/// The operating system, faked: what dm asked of it is kept for the test to read.
#[derive(Default)]
pub(crate) struct Fake {
    pub(crate) notified: std::sync::Mutex<Vec<String>>,
    pub(crate) clipboard: std::sync::Mutex<Option<String>>,
    pub(crate) login: std::sync::Mutex<Option<bool>>,
    pub(crate) opened: std::sync::Mutex<Vec<String>>,
    pub(crate) chosen: std::sync::Mutex<Option<std::path::PathBuf>>,
}

impl dm_shell::app::platform::Platform for Fake {
    fn set_login_item(&self, wanted: bool) -> Result<(), String> {
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
    fn chime(&self) {
        self.opened.lock().unwrap().push("chime".into());
    }
    fn alert(&self) {
        self.opened.lock().unwrap().push("alert".into());
    }
    fn power(
        &self,
        action: dm_shell::app::power::WhenDone,
        _report: dm_shell::app::power::Report,
    ) -> Result<(), String> {
        self.opened
            .lock()
            .unwrap()
            .push(format!("power {action:?}"));
        Ok(())
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        self.chosen.lock().unwrap().clone()
    }
    fn open_with(&self, path: &str, app: &std::path::Path) -> Result<(), String> {
        self.opened
            .lock()
            .unwrap()
            .push(format!("{path} with {}", app.display()));
        Ok(())
    }
    fn choose_file(&self, _save: bool, _suggested: &str) -> Option<std::path::PathBuf> {
        self.chosen.lock().unwrap().clone()
    }
    fn choose_folder(&self) -> Option<std::path::PathBuf> {
        self.chosen.lock().unwrap().clone()
    }
}

/// The window, opened, and how to ask it things.
pub(crate) struct Harness {
    pub(crate) cx: TestAppContext,
    pub(crate) window: AnyWindowHandle,
    view: Entity<Downloads>,
    /// What dm asked of the system; read by the tests ported from the egui window's.
    #[allow(dead_code)]
    pub(crate) fake: Arc<Fake>,
    asks: mpsc::Sender<dm_desktop_base::control::Request>,
    wakes: crate::wakes::Wakes,
    /// When GPUI's clock was last moved on (`step`).
    clock: std::time::Instant,
    /// The windows open at the last step: one opened since is put in front (`step`).
    seen: Vec<AnyWindowHandle>,
}

impl Harness {
    /// The window over `settings` with its queue on `runtime`, the system `fake` and its parts
    /// in `dir`, as a launch that read them would open it.
    pub(crate) fn open(
        runtime: tokio::runtime::Handle,
        fake: Arc<Fake>,
        dir: &std::path::Path,
        settings: dm_shell::Settings,
    ) -> Harness {
        let queue = dm_core::Queue::new(
            dm_core::client(),
            settings.options(dir.join("parts"), dm_core::Limiter::unlimited()),
            settings.concurrency,
        );
        // The schedules and limits as a launch puts them on the queue (`shell::start`).
        queue.set_schedule(settings.schedule.clone());
        queue.set_queue_schedules(settings.queue_schedules.clone());
        queue.set_allowance(settings.allowance());
        queue.set_recheck_every(settings.recheck_every());
        queue.set_queue_limits(settings.queue_limit_map());
        queue.set_queue_starts(settings.queue_start_list());
        let backend = dm_shell::app::backend::Backend::new(
            runtime,
            queue,
            settings,
            Vec::new(),
            Arc::clone(&fake) as Arc<dyn dm_shell::app::platform::Platform>,
        );
        let (asks, asked) = mpsc::channel();
        let (wakes, woken) = crate::wakes::pair();
        let mut cx = TestAppContext::single();
        cx.update(gpui_component::init);
        crate::screen::record();
        // As `main` opens it: the list in gpui-component's root, then what the application
        // hears beside it (`wire`: the keys, the menus, Quit and the text size).
        let made = std::rc::Rc::new(std::cell::RefCell::new(None));
        let kept = std::rc::Rc::clone(&made);
        let window = cx.add_window(move |window, cx| {
            let view = cx.new(|cx| {
                Downloads::new(
                    backend,
                    woken,
                    Some(asked),
                    false,
                    Default::default(),
                    false,
                    window,
                    cx,
                )
            });
            *kept.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view: Entity<Downloads> = made.borrow_mut().take().unwrap();
        cx.update(|cx| crate::wire(&view, cx));
        let window: AnyWindowHandle = window.into();
        cx.run_until_parked();
        Harness {
            cx,
            window,
            view,
            fake,
            asks,
            wakes,
            clock: std::time::Instant::now(),
            seen: Vec::new(),
        }
    }

    /// `read` of the window's own state.
    pub(crate) fn view<R>(&mut self, read: impl FnOnce(&mut Downloads) -> R) -> R {
        self.view.update(&mut self.cx, |view, _| read(view))
    }

    /// Every piece of text on screen, in every window, nearest the front first.
    pub(crate) fn texts(&mut self) -> Vec<String> {
        let texts = self.call("ui.texts", Value::Null).unwrap();
        texts["texts"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|t| t["text"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether `words` are on screen, in any window.
    pub(crate) fn shows(&mut self, words: &str) -> bool {
        self.texts().iter().any(|t| t == words)
    }

    /// Selects the row `name` alone through the control port (`ui.select`), as a script does:
    /// it reaches the list where a dialog holds the window's own clicks (B42).
    pub(crate) fn select_by_port(&mut self, name: &str) {
        let id = self.row(name).id;
        self.call("ui.select", serde_json::json!({ "ids": [id] }))
            .unwrap();
    }

    /// Clicks the words `text` (a button, a tab, a menu), as a person does.
    pub(crate) fn click(&mut self, text: &str) {
        if let Err(why) = self.call("ui.click", serde_json::json!({ "text": text })) {
            panic!("{text} could not be clicked: {why}");
        }
    }

    /// Types `text` into the field that has the keyboard, as a person types.
    pub(crate) fn type_text(&mut self, text: &str) {
        if let Err(why) = self.call("ui.type", serde_json::json!({ "text": text })) {
            panic!("{text:?} could not be typed: {why}");
        }
    }

    /// Presses `key` (`"enter"`, `"escape"`, `"a"`…), with `modifiers` (`"cmd"`, `"shift"`…).
    #[allow(dead_code)]
    pub(crate) fn key(&mut self, key: &str, modifiers: &[&str]) {
        let asked = serde_json::json!({ "key": key, "modifiers": modifiers });
        if let Err(why) = self.call("ui.key", asked) {
            panic!("{key} could not be pressed: {why}");
        }
    }

    /// Wakes the window (it reads the list, runs its tick, answers and draws) until `done`
    /// says so, or fails after ten seconds.
    pub(crate) fn until(&mut self, what: &str, mut done: impl FnMut(&mut Harness) -> bool) {
        self.until_within(what, std::time::Duration::from_secs(10), &mut done);
    }

    /// [`Harness::until`], failing after `most`.
    pub(crate) fn until_within(
        &mut self,
        what: &str,
        most: std::time::Duration,
        done: &mut dyn FnMut(&mut Harness) -> bool,
    ) {
        let deadline = std::time::Instant::now() + most;
        loop {
            self.step();
            if done(self) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {what}; on screen: {:?}",
                self.texts()
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    /// Waits until `words` are on screen.
    pub(crate) fn until_shown(&mut self, words: &str) {
        self.until(words, |h| h.shows(words));
    }

    /// The rows of the list, as the window last read them.
    pub(crate) fn rows(&mut self) -> Vec<dm_core::Entry> {
        self.view(|view| view.rows.iter().map(|r| (**r).clone()).collect())
    }

    /// Add URL, the address typed, OK, then Download details' `button` ("Start now",
    /// "Download later"): a download added as a person adds one.
    pub(crate) fn add_url(&mut self, url: &str, button: &str) {
        self.add_url_open(url);
        self.click(button);
    }

    /// Add URL, the address typed, OK: Download details open for it, asked again where the
    /// address is listed already ("Download again").
    pub(crate) fn add_url_open(&mut self, url: &str) {
        self.click("Add URL");
        self.until_shown("Address");
        self.type_text(url);
        self.click("OK");
        // An address already in the list is asked about first; a test adding the same file
        // twice means it.
        self.until("Download details or Already in the list", |h| {
            h.dialog_open("Download details") || h.shows("Already in the list")
        });
        if self.shows("Already in the list") {
            self.click("Download again");
            self.until("Download details", |h| h.dialog_open("Download details"));
        }
    }

    /// Whether a dialog called `title` is open.
    pub(crate) fn dialog_open(&mut self, title: &str) -> bool {
        self.dialogs().iter().any(|d| d == title)
    }

    /// The dialogs open, by title.
    pub(crate) fn dialogs(&mut self) -> Vec<String> {
        let windows = self.call("ui.windows", Value::Null).unwrap();
        windows["dialogs"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|d| d["title"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One wake of the window, run until it parks. As the system does when a dialog closes,
    /// the main window is given the keys again where no window has them: the test platform
    /// hands them to none, and a question waits for the main window in front.
    fn step(&mut self) {
        let main = self.window;
        let seen = std::mem::take(&mut self.seen);
        self.seen = self.cx.update(|cx| {
            let open = cx.windows();
            // A dialog opened since is in front, as the system makes a new window the one with
            // the keys (its Tab walks its own controls); one closed hands them back.
            let dialogs: Vec<AnyWindowHandle> = crate::screen::Screens::all(cx)
                .into_iter()
                .filter(|(_, _, dialog)| *dialog)
                .map(|(handle, _, _)| handle)
                .collect();
            let new = open
                .iter()
                .rev()
                .find(|w| dialogs.contains(w) && !seen.contains(w));
            match new {
                Some(dialog) => {
                    let _ = dialog.update(cx, |_, window, _| window.activate_window());
                }
                None if cx.active_window().is_none_or(|a| !open.contains(&a)) => {
                    let _ = main.update(cx, |_, window, _| window.activate_window());
                }
                None => {}
            }
            open
        });
        // GPUI's test clock stands still unless moved: moved on by the time that really
        // passed, the window's own timers (its redraw while a download runs, a progress
        // window's look at the pieces) fire as they would.
        let now = std::time::Instant::now();
        self.cx.executor().advance_clock(now - self.clock);
        self.clock = now;
        self.wakes.send(Wake::Now);
        self.cx.run_until_parked();
        // The test platform has no frame loop: each window's next-frame work (a dialog grown
        // to fit what it shows, `fit.rs`) is run here, as the display's next frame runs it.
        self.cx.update(|cx| {
            for handle in cx.windows() {
                let _ = handle.update(cx, |_, window, cx| window.simulate_next_frame(cx));
            }
        });
        self.cx.run_until_parked();
        // A window that asked for another size (a dialog grown to fit what it shows, `fit.rs`)
        // is told it has it, as the system tells it: the test platform keeps the size it was
        // asked for and says nothing.
        let resized: Vec<(AnyWindowHandle, gpui::Size<gpui::Pixels>)> = self.cx.update(|cx| {
            cx.windows()
                .into_iter()
                .filter_map(|handle| {
                    handle
                        .update(cx, |_, window, _| {
                            let asked = window.bounds().size;
                            (asked != window.viewport_size()).then_some((handle, asked))
                        })
                        .ok()
                        .flatten()
                })
                .collect()
        });
        for (handle, size) in resized {
            self.cx.simulate_window_resize(handle, size);
        }
        // And every window drawn again, as a display's refresh does.
        self.cx.update(|cx| cx.refresh_windows());
        self.cx.run_until_parked();
    }

    /// Asks the window `method` with `params`, as the control port does, and its answer.
    pub(crate) fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let (reply, answer) = mpsc::channel();
        self.asks
            .send(dm_desktop_base::control::Request {
                via_mcp: false,
                method: method.into(),
                params,
                reply,
            })
            .unwrap();
        for _ in 0..100 {
            self.step();
            if let Ok(answered) = answer.try_recv() {
                return answered;
            }
        }
        Err(format!("{method} was not answered"))
    }

    /// `act` on the window's view, with its context (a command run, settings put in force),
    /// then a wake.
    pub(crate) fn act<R>(
        &mut self,
        act: impl FnOnce(&mut Downloads, &mut gpui::Context<Downloads>) -> R,
    ) -> R {
        let done = self.view.update(&mut self.cx, act);
        self.step();
        done
    }

    /// Runs the command `id` (`"file.properties"`…) as a script's `command.run` does.
    pub(crate) fn command(&mut self, id: &str) {
        if let Err(why) = self.call("command.run", serde_json::json!({ "id": id })) {
            panic!("{id} did not run: {why}");
        }
    }

    /// The settings in force, `change`d and put in force as Options' OK puts them.
    pub(crate) fn settings_to(&mut self, change: impl FnOnce(&mut dm_shell::Settings)) {
        let mut wanted = self.view(|view| view.backend.settings());
        change(&mut wanted);
        self.act(|view, cx| view.apply_settings(wanted, cx))
            .unwrap();
    }

    /// What the status line says now.
    pub(crate) fn said(&mut self) -> String {
        self.view(|view| view.said.as_ref().map(|(s, _)| s.clone()))
            .unwrap_or_default()
    }

    /// A download handed over by the browser, as the native host leaves one in the inbox.
    pub(crate) fn hand(&mut self, request: dm_shell::host::Request) {
        self.view(|view| dm_core::lock(&view.backend.handed).push_back(request));
        self.step();
    }

    /// Whether a piece of text on screen holds `words`.
    pub(crate) fn shows_part(&mut self, words: &str) -> bool {
        self.texts().iter().any(|t| t.contains(words))
    }

    /// Clicks the words `text` as a right-click.
    pub(crate) fn right_click(&mut self, text: &str) {
        let asked = serde_json::json!({ "text": text, "button": "right" });
        if let Err(why) = self.call("ui.click", asked) {
            panic!("{text} could not be right-clicked: {why}");
        }
    }

    /// Clicks the text field beside the label `label` (a form's row: the label at the left,
    /// its field just right of it), selects what it holds and types `text` over it, as a
    /// person replaces a field's words.
    pub(crate) fn type_into(&mut self, label: &str, text: &str) {
        self.type_into_nth(label, 0, text);
    }

    /// [`Harness::type_into`] beside the `nth` label of these words, nearest the front first
    /// (a part's "to" behind Add for later's "to" its queue).
    pub(crate) fn type_into_nth(&mut self, label: &str, nth: usize, text: &str) {
        let (window, at) = self.drawn(label, nth);
        let beside = gpui::point(
            at.origin.x + at.size.width + gpui::px(16.0),
            at.origin.y + at.size.height / 2.0,
        );
        self.press_at(window, beside, 1);
        self.key("a", &["cmd"]);
        if text.is_empty() {
            self.key("backspace", &[]);
        } else {
            self.type_text(text);
        }
    }

    /// Clicks (`clicks` 2: double-clicks) the words `text` as a person's pointer does, not a
    /// script's: what runs a downloaded file takes a person's click alone.
    pub(crate) fn person_click(&mut self, text: &str, clicks: usize) {
        let (window, at) = self.drawn(text, 0);
        self.press_at(window, at.center(), clicks);
    }

    /// Waits until a script's last act is long enough ago for a person's click to run a file.
    pub(crate) fn until_a_persons_moment(&mut self) {
        self.until_within(
            "the script's moment to pass",
            std::time::Duration::from_secs(10),
            &mut |h| !h.view(|view| view.scripted.by_script()),
        );
    }

    /// The next of dm's own file or folder panels (Import, Export, Options' Browse…) is
    /// answered with `path`, as a script's `ui.choose` answers it.
    pub(crate) fn choose_ahead(&mut self, path: &std::path::Path) {
        crate::services::choose_ahead(path.to_path_buf());
    }

    /// The folder panel a dialog's Browse… opened (GPUI's own path prompt) answered with
    /// `path`.
    pub(crate) fn answer_folder_panel(&mut self, path: &std::path::Path) {
        let path = path.to_path_buf();
        self.until("a folder panel", |h| h.cx.did_prompt_for_paths());
        self.cx
            .simulate_path_prompt_response(move |_| Some(vec![path]));
        self.step();
    }

    /// The dialogs open by the name each was opened with (a progress window is "Download
    /// progress", whatever its title says of how far it is), the oldest first.
    pub(crate) fn dialog_names(&mut self) -> Vec<String> {
        let seen = self.call("ui.inspect", Value::Null).unwrap();
        seen["dialogs"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|d| d.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The row the list names `name`.
    pub(crate) fn row(&mut self, name: &str) -> dm_core::Entry {
        self.view(|view| {
            view.rows
                .iter()
                .find(|r| view.name_shown(r) == name)
                .map(|r| (**r).clone())
        })
        .unwrap_or_else(|| panic!("no row {name}"))
    }

    /// Clicks the row `name`, which selects it alone.
    pub(crate) fn select(&mut self, name: &str) {
        self.click(name);
        let id = self.row(name).id;
        self.until(&format!("{name} to be selected"), |h| {
            h.view(|view| view.selected.len() == 1 && view.selected.contains(&id))
        });
    }

    /// Every dialog's window closed, as its close button closes it, and a menu the port opened.
    pub(crate) fn close_dialogs(&mut self) {
        let _ = self.call("ui.key", serde_json::json!({ "key": "escape" }));
        let main = self.window;
        self.cx.update(|cx| {
            for window in cx.windows() {
                if window != main {
                    let _ = window.update(cx, |_, window, _| window.remove_window());
                }
            }
        });
        self.step();
    }

    /// Whether `words` are on screen in a dialog's window, not the main window's.
    pub(crate) fn shows_in_dialog(&mut self, words: &str) -> bool {
        let texts = self.call("ui.texts", Value::Null).unwrap();
        texts["texts"].as_array().is_some_and(|all| {
            all.iter()
                .any(|t| t["text"] == words && t["window"] == "dialog")
        })
    }

    /// Where the `nth` piece of text saying `text` was drawn, nearest the front first, and in
    /// which window.
    fn drawn(
        &mut self,
        text: &str,
        nth: usize,
    ) -> (gpui::AnyWindowHandle, gpui::Bounds<gpui::Pixels>) {
        self.step();
        let found = self.cx.update(|cx| {
            crate::screen::Screens::all(cx)
                .into_iter()
                .flat_map(|(handle, screen, _)| {
                    screen
                        .drawn()
                        .into_iter()
                        .rev()
                        .filter(|d| d.text.as_ref() == text)
                        .map(move |d| (handle, d.bounds))
                        .collect::<Vec<_>>()
                })
                .nth(nth)
        });
        found.unwrap_or_else(|| panic!("nothing on screen says {text:?}: {:?}", self.texts()))
    }

    /// A press and release of the pointer at `at` in `window`, `clicks` times.
    fn press_at(
        &mut self,
        window: gpui::AnyWindowHandle,
        at: gpui::Point<gpui::Pixels>,
        clicks: usize,
    ) {
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                let none = Modifiers::default();
                window.dispatch_event(
                    gpui::PlatformInput::MouseMove(MouseMoveEvent {
                        position: at,
                        pressed_button: None,
                        modifiers: none,
                    }),
                    cx,
                );
                for count in 1..=clicks {
                    window.dispatch_event(
                        gpui::PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Left,
                            position: at,
                            modifiers: none,
                            click_count: count,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        gpui::PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position: at,
                            modifiers: none,
                            click_count: count,
                        }),
                        cx,
                    );
                }
            });
        });
        self.step();
    }

    /// The settings changed by `alter`, through the window's own way in, as Options' OK
    /// puts them in force.
    pub(crate) fn apply(&mut self, alter: impl FnOnce(&mut dm_shell::Settings)) {
        let mut wanted = self.view(|view| view.backend.settings());
        alter(&mut wanted);
        if let Err(why) = self.update(|view, cx| view.apply_settings(wanted, cx)) {
            panic!("the settings were refused: {why}");
        }
    }

    /// Clicks the `nth` words `text` as a person's pointer does, where the port will not
    /// (Open, Open with…: what runs a downloaded file): pressed and let go over them, in the
    /// window they are in, nearest the front first.
    pub(crate) fn click_as_person(&mut self, text: &str, nth: usize) {
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
        let found = self.cx.update(|cx| {
            crate::screen::Screens::all(cx)
                .into_iter()
                .flat_map(|(handle, screen, _)| {
                    screen.drawn().into_iter().rev().map(move |d| (handle, d))
                })
                .filter(|(_, d)| d.text.as_ref() == text)
                .nth(nth)
        });
        let Some((window, drawn)) = found else {
            panic!("{text} ({nth}) is not on screen: {:?}", self.texts());
        };
        let at = drawn.bounds.center();
        // A person's, not the port's: what the port clicked a moment ago is not held against it.
        self.view(|view| view.scripted = Default::default());
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                let none = Modifiers::default();
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: none,
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: none,
                        click_count: 1,
                    }),
                    cx,
                );
            });
        });
        self.step();
    }

    /// Clicks the field beside the words `label` (its name, at its left), as a person clicks
    /// into it: the keyboard goes there.
    pub(crate) fn click_beside(&mut self, label: &str) {
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
        let found = self.cx.update(|cx| {
            crate::screen::Screens::all(cx)
                .into_iter()
                .flat_map(|(handle, screen, _)| {
                    screen.drawn().into_iter().rev().map(move |d| (handle, d))
                })
                .find(|(_, d)| d.text.as_ref() == label)
        });
        let Some((window, drawn)) = found else {
            panic!("{label} is not on screen: {:?}", self.texts());
        };
        let bounds = drawn.bounds;
        let at = gpui::point(
            bounds.origin.x + bounds.size.width + gpui::px(30.0),
            bounds.origin.y + bounds.size.height / 2.0,
        );
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                let none = Modifiers::default();
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: none,
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: none,
                        click_count: 1,
                    }),
                    cx,
                );
            });
        });
        self.step();
    }

    /// Double-clicks the words `text`, as a person does.
    pub(crate) fn double_click(&mut self, text: &str) {
        let asked = serde_json::json!({ "text": text, "double": true });
        if let Err(why) = self.call("ui.click", asked) {
            panic!("{text} could not be double-clicked: {why}");
        }
    }

    /// Whether `command` is offered for the rows selected, as its menu item is.
    pub(crate) fn enabled(&mut self, command: dm_shell::app::commands::Command) -> bool {
        self.view(|view| view.enabled(command))
    }

    /// A download the browser hands over, as dm's inbox leaves it for the window.
    pub(crate) fn hand_over(&mut self, request: dm_shell::host::Request) {
        self.view(|view| view.backend.handed.lock().unwrap().push_back(request));
        self.step();
    }

    /// Runs `command` as a person's pick of it in a menu does: Open and Preview too, which the
    /// port leaves to a person (the port's clicks a moment ago are not held against it).
    pub(crate) fn run(&mut self, command: dm_shell::app::commands::Command) {
        self.update(|view, cx| {
            view.scripted = Default::default();
            view.run(command, cx)
        });
    }

    /// Turns the wheel `lines` down (up where negative) with the pointer over the words `text`.
    pub(crate) fn scroll(&mut self, text: &str, lines: f32) {
        let asked = serde_json::json!({ "text": text, "lines": lines });
        if let Err(why) = self.call("ui.scroll", asked) {
            panic!("{text} could not be scrolled: {why}");
        }
    }

    /// Scrolls the dialog in front down, a few lines at a time with the pointer over its
    /// words, until `words` are in sight.
    pub(crate) fn scroll_to(&mut self, words: &str) {
        for _ in 0..60 {
            if self.shows(words) {
                return;
            }
            let texts = self.call("ui.texts", Value::Null).unwrap();
            let in_dialog: Vec<String> = texts["texts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|t| t["window"] == "dialog")
                .filter_map(|t| t["text"].as_str().map(str::to_string))
                .collect();
            // One from the middle: the page's, not its buttons' at the foot.
            let Some(over) = in_dialog.get(in_dialog.len() / 2).cloned() else {
                break;
            };
            self.scroll(&over, 3.0);
        }
        assert!(
            self.shows(words),
            "{words} never came in sight: {:?}",
            self.texts()
        );
    }

    /// `alter` of the window's own state, with its context: what a menu's item or a hand-over
    /// does to it.
    pub(crate) fn update<R>(
        &mut self,
        alter: impl FnOnce(&mut Downloads, &mut gpui::Context<Downloads>) -> R,
    ) -> R {
        let done = self.view.update(&mut self.cx, |view, cx| alter(view, cx));
        self.step();
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The harness itself: the window opens over an empty list and says so, and a click on
    /// Add URL opens its dialog.
    #[test]
    fn the_window_opens_and_a_click_opens_add_url() {
        let world = world::world();
        let mut h = world.harness();
        assert!(h.shows("Nothing queued"), "{:?}", h.texts());
        assert!(h.view(|view| view.rows.is_empty()));
        h.click("Add URL");
        assert_eq!(h.dialogs(), ["Add URL"]);
        assert!(h.shows("Address"), "{:?}", h.texts());
    }
}
