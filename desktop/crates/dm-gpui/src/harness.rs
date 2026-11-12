// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The GPUI window opened in GPUI's test platform, for the window's tests: driven through the
//! control port's methods (`ui.click`, `ui.texts`, `command.run`…), as a script drives it.

use std::sync::{mpsc, Arc};

use gpui::{TestAppContext, WindowHandle};
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
        self.opened.lock().unwrap().push(format!("power {action:?}"));
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
    pub(crate) window: WindowHandle<Downloads>,
    /// What dm asked of the system; read by the tests ported from the egui window's.
    #[allow(dead_code)]
    pub(crate) fake: Arc<Fake>,
    asks: mpsc::Sender<dm_desktop_base::control::Request>,
    wakes: crate::wakes::Wakes,
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
        let window = cx.add_window(|window, cx| {
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
        cx.run_until_parked();
        Harness {
            cx,
            window,
            fake,
            asks,
            wakes,
        }
    }

    /// `read` of the window's own state.
    pub(crate) fn view<R>(&mut self, read: impl FnOnce(&mut Downloads) -> R) -> R {
        self.window
            .update(&mut self.cx, |view, _, _| read(view))
            .unwrap()
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
        self.click(button);
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
        self.cx.update(|cx| {
            if cx.active_window().is_none() {
                let _ = main.update(cx, |_, window, _| window.activate_window());
            }
        });
        self.wakes.send(Wake::Now);
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
