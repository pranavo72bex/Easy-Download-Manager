// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a dm with no window answers over the control port. Only answers
//! that save nothing: outside dm-shell's own tests the settings and the
//! list are kept in the real home, so adding a download or changing a
//! setting is checked on the real binary, on a home of its own
//! (`scripts/app-check.py`, `scripts/smoke.py`).

use super::*;
use dm_shell::app::power::WhenDone;

/// A platform that does nothing.
struct Nothing;

impl dm_shell::app::backend::Platform for Nothing {
    fn set_login_item(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str) {}
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn open(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn chime(&self) {}
    fn power(
        &self,
        _: dm_shell::app::power::WhenDone,
        _: dm_shell::app::power::Report,
    ) -> Result<(), String> {
        Ok(())
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
    fn choose_file(&self, _: bool, _: &str) -> Option<std::path::PathBuf> {
        None
    }
}

/// dm with no window over an empty queue of its own under `dir`.
fn headless(runtime: &tokio::runtime::Runtime, dir: &std::path::Path) -> Headless {
    let settings = dm_shell::Settings {
        download_dir: dir.join("out"),
        ..Default::default()
    };
    let queue = dm_core::Queue::new(
        dm_core::client(),
        settings.options(dir.join("parts"), dm_core::Limiter::unlimited()),
        settings.concurrency,
    );
    let backend = Backend::new(
        runtime.handle().clone(),
        queue,
        settings,
        Vec::new(),
        Arc::new(Nothing),
    );
    Headless::new(backend, None, false)
}

#[test]
fn with_no_window_a_picture_and_a_step_are_refused_in_words() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    let shot = dm.answer("ui.screenshot", &json!({"name": "x"}));
    assert_eq!(shot, Err(probe::NO_WINDOW_TO_PICTURE.to_string()));
    for (method, params) in [
        ("ui.click", json!({"text": "Options"})),
        ("ui.type", json!({"text": "a"})),
        ("ui.key", json!({"key": "Escape"})),
        ("ui.scroll", json!({"text": "a"})),
    ] {
        assert_eq!(
            dm.answer(method, &params),
            Err(NO_WINDOW.to_string()),
            "{method}"
        );
    }
    assert_eq!(
        dm.answer("ui.window", &json!({"to": "tray"})),
        Err("there is no window".to_string())
    );
    // Nothing is on a screen: an assistant's dm_screen reads that, not an
    // error.
    let texts = dm.answer("ui.texts", &Value::Null).unwrap();
    assert_eq!(texts["texts"], json!([]));
    let windows = dm.answer("ui.windows", &Value::Null).unwrap();
    assert_eq!(
        (windows["main"].clone(), windows["dialogs"].clone()),
        (Value::Null, json!([]))
    );
}

#[test]
fn the_view_and_the_commands_answer_as_under_a_window() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    let listed = dm.answer("commands.list", &Value::Null).unwrap();
    let ids: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(ids.contains(&"file.stop_download"), "{ids:?}");
    let inspected = dm.answer("ui.inspect", &Value::Null).unwrap();
    assert_eq!(
        (inspected["rows"].clone(), inspected["hidden"].clone()),
        (json!([]), json!(true))
    );
    assert!(dm.answer("ui.place", &json!({"place": "finished"})).is_ok());
    assert!(dm.answer("ui.place", &json!({"place": "nowhere"})).is_err());
    assert_eq!(
        dm.answer("ui.select", &json!({"ids": [7]})).unwrap()["selected"],
        json!([]),
        "a row that is not listed is not chosen"
    );
    // A file is opened by a person, never over the port.
    assert!(dm
        .answer("command.run", &json!({"id": "file.open"}))
        .is_err());
    // What needs a window is refused, said why.
    let add_url = dm
        .answer("command.run", &json!({"id": "program.add_url"}))
        .unwrap_err();
    assert!(add_url.contains("needs dm's window"), "{add_url}");
    assert!(dm
        .answer("command.run", &json!({"id": "no.such.command"}))
        .is_err());
}

#[test]
fn when_done_is_set_and_exit_over_the_port_ends_dm_without_asking() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    dm.answer("command.run", &json!({"id": "downloads.when_done.exit"}))
        .unwrap();
    assert_eq!(dm.session.when_done, WhenDone::Quit);
    // With nobody to see a countdown, Exit when done is at once.
    assert_eq!(
        dm.session.when_done_fires(true, Instant::now()),
        Fired::QuitNow
    );
    assert!(!dm.quit_asked());
    dm.answer("command.run", &json!({"id": "program.exit"}))
        .unwrap();
    assert!(dm.quit_asked());
}

#[test]
fn a_batch_reaches_the_view_but_not_a_step() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    let answered = dm
        .answer(
            "batch",
            &json!({"steps": [
                {"method": "ui.inspect"},
                {"method": "ui.click", "params": {"text": "Options"}},
            ]}),
        )
        .unwrap();
    let steps = answered.as_array().cloned().unwrap_or_default();
    assert_eq!(steps.len(), 2, "{answered}");
    assert_eq!(steps[0]["ok"], json!(true), "{answered}");
    assert_eq!(steps[1]["ok"], json!(false), "{answered}");
}
