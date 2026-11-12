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
    _runtime: tokio::runtime::Runtime,
}

impl Harness {
    /// The window over an empty list, its settings `settings`.
    pub(crate) fn new(settings: dm_shell::Settings) -> Harness {
        // A folder of its own in the test home, for its parts.
        let dir = tempfile::tempdir_in(home()).unwrap().keep();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let fake = Arc::new(Fake::default());
        let queue = dm_core::Queue::new(
            dm_core::client(),
            settings.options(dir.join("parts"), dm_core::Limiter::unlimited()),
            1,
        );
        let backend = dm_shell::app::backend::Backend::new(
            runtime.handle().clone(),
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
            _runtime: runtime,
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
            self.wakes.send(Wake::Now);
            self.cx.run_until_parked();
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
        let mut h = Harness::new(dm_shell::Settings::default());
        assert!(h.shows("Nothing queued"), "{:?}", h.texts());
        assert!(h.view(|view| view.rows.is_empty()));
        h.click("Add URL");
        assert_eq!(h.dialogs(), ["Add URL"]);
        assert!(h.shows("Address"), "{:?}", h.texts());
    }
}
