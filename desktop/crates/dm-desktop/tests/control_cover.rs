// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The control protocol and the AI client's tools, method by method: what
//! each answers, what it refuses and why, and what the window shows after.
//!
//! The window is driven here as dm's own frame drives it (`src/window.rs`):
//! the steps a script asked for are handed to egui one painted pass at a
//! time, and the words drawn are read back at the end of each pass. The
//! AI client's side is tried against the window and against a stand-in for
//! it on loopback that says exactly what it was asked.

#[path = "window/support.rs"]
#[allow(dead_code, unused_imports)]
mod support;

use dm_desktop::control::Request;
use dm_desktop::mcp::{self, Control};
use serde_json::{json, Value};
use std::io::Write;
use std::net::TcpListener;
use std::sync::mpsc;
use support::*;

/// Dialogs drawn as windows of their own are listed in one place for the
/// whole process, and every window's frame clears that list: two windows
/// drawn at once on two test threads read each other's dialogs. Held by
/// every test that draws frames.
static ONE_WINDOW: Mutex<()> = Mutex::new(());

fn one_window() -> std::sync::MutexGuard<'static, ()> {
    ONE_WINDOW.lock().unwrap_or_else(|e| e.into_inner())
}

/// A window `width` points wide over `settings`, drawn as dm's frame draws
/// it, and the end of the channel its control requests come in by.
fn person_window_with(
    world: &World,
    width: f32,
    settings: Settings,
) -> (Harness<'static, DmApp>, mpsc::Sender<Request>) {
    let mut h = Harness::builder()
        .with_size(egui::vec2(width, 720.0))
        // Short passes, so a double-click's four steps land inside egui's
        // double-click time.
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, app: &mut DmApp| {
                app.ui(ui);
                app.scan_screen(ui.ctx());
                app.painted();
            },
            world.app_with(settings),
        );
    let (to_window, requests) = mpsc::channel();
    h.state_mut().attach_control(requests);
    frames(&mut h, 4);
    (h, to_window)
}

fn person_window(world: &World) -> (Harness<'static, DmApp>, mpsc::Sender<Request>) {
    person_window_with(world, 1100.0, world.settings())
}

/// One painted pass, given first the step of a script that waits for it,
/// as `raw_input_hook` gives it.
fn frame(h: &mut Harness<'_, DmApp>) {
    let ctx = h.ctx.clone();
    let events = h.state_mut().synthetic_events(&ctx);
    h.input_mut().events.extend(events);
    h.state_mut().refresh();
    h.step();
}

fn frames(h: &mut Harness<'_, DmApp>, n: usize) {
    for _ in 0..n {
        frame(h);
    }
}

/// Passes until `done` holds, or a failure after ten seconds.
fn settle(h: &mut Harness<'_, DmApp>, what: &str, done: impl Fn(&DmApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(h);
        if done(h.state()) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// A person's kind of request, answered as the window answers it.
fn ask(h: &mut Harness<'_, DmApp>, method: &str, params: Value) -> Result<Value, String> {
    let ctx = h.ctx.clone();
    h.state_mut()
        .human_call(method, &params, Some(&ctx))
        .unwrap_or_else(|| panic!("{method} is a person's kind of request"))
}

/// Any other request.
fn call(h: &mut Harness<'_, DmApp>, method: &str, params: Value) -> Result<Value, String> {
    h.state_mut().control_call(method, &params)
}

/// The words on screen, nearest the front first.
fn words(h: &mut Harness<'_, DmApp>) -> Vec<String> {
    ask(h, "ui.texts", json!({})).unwrap()["texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["text"].as_str().unwrap().to_string())
        .collect()
}

fn shows(h: &mut Harness<'_, DmApp>, text: &str) -> bool {
    words(h).iter().any(|w| w == text)
}

/// Every step asked for handed over, and the passes after it run.
fn drain(h: &mut Harness<'_, DmApp>) {
    for _ in 0..40 {
        frame(h);
    }
}

fn id_of(h: &Harness<'_, DmApp>, name: &str) -> u64 {
    h.state()
        .rows()
        .iter()
        .find(|r| r.url.ends_with(name))
        .unwrap_or_else(|| panic!("no row for {name}"))
        .id
}

fn state_of(h: &Harness<'_, DmApp>, id: u64) -> Option<State> {
    h.state().backend().row(id).map(|r| r.state)
}

fn later(h: &mut Harness<'_, DmApp>, urls: Vec<String>) {
    let said = call(h, "downloads.add", json!({ "urls": urls, "later": true })).unwrap();
    assert!(said["added"].as_u64().unwrap() > 0, "{said}");
    h.state_mut().refresh();
}

fn start(h: &mut Harness<'_, DmApp>, url: String) {
    let said = call(h, "downloads.add", json!({ "urls": [url] })).unwrap();
    assert_eq!(said["added"], 1, "{said}");
    h.state_mut().refresh();
}

// The window's person-like requests (`app/human.rs`).

#[test]
fn a_script_reads_the_screen_clicks_types_and_presses_keys_as_a_person() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);

    let seen = ask(&mut h, "ui.texts", json!({})).unwrap();
    assert_eq!(seen["note"], "nearest the front first");
    let add = seen["texts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["text"] == "Add URL")
        .unwrap_or_else(|| panic!("no Add URL on screen: {seen}"))
        .clone();
    assert_eq!(add["window"], "main");
    assert_eq!(add["rect"].as_array().unwrap().len(), 4);

    // The click is the hover, the press and the release, a pass apart.
    let clicked = ask(&mut h, "ui.click", json!({ "text": "add url" })).unwrap();
    assert_eq!(clicked["clicked"], "Add URL");
    assert_eq!(clicked["at"].as_array().unwrap().len(), 2);
    settle(&mut h, "Add URL to open", |app| {
        app.dialogs.add_url.is_some()
    });

    // Typed where the keyboard is: Add URL's address field.
    let typed = ask(&mut h, "ui.type", json!({ "text": "héllo-typed" })).unwrap();
    assert_eq!(typed["typed"], 11);
    drain(&mut h);
    assert!(
        words(&mut h).iter().any(|w| w.contains("héllo-typed")),
        "{:?}",
        words(&mut h)
    );

    let pressed = ask(&mut h, "ui.key", json!({ "key": "Escape" })).unwrap();
    assert_eq!(pressed["pressed"], "Escape");
    settle(&mut h, "Escape to close Add URL", |app| {
        app.dialogs.add_url.is_none()
    });
}

#[test]
fn a_click_is_refused_without_words_on_words_not_shown_and_on_what_runs_a_file() {
    let _one = one_window();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    for params in [json!({}), json!({ "text": "" }), json!({ "text": 3 })] {
        let refused = ask(&mut h, "ui.click", params.clone()).unwrap_err();
        assert!(refused.contains("give `text`"), "{params}: {refused}");
    }
    for run in ["Open", "open with…", "Open with", " Preview "] {
        let refused = ask(&mut h, "ui.click", json!({ "text": run })).unwrap_err();
        assert!(
            refused.contains("runs a downloaded file"),
            "{run}: {refused}"
        );
    }
    let missing = ask(&mut h, "ui.click", json!({ "text": "no such words" })).unwrap_err();
    assert!(missing.contains("nothing on screen says"), "{missing}");
    assert!(
        missing.contains("Add URL"),
        "what is shown is named: {missing}"
    );
    // A later match that is not there.
    let missing = ask(&mut h, "ui.click", json!({ "text": "Add URL", "nth": 7 })).unwrap_err();
    assert!(missing.contains("nothing on screen says"), "{missing}");
    // A refused click is no step.
    drain(&mut h);
    assert!(h.state().dialogs.add_url.is_none());
}

#[test]
fn a_word_drawn_shortened_is_clicked_by_the_whole_of_it() {
    let _one = one_window();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    assert!(
        words(&mut h).iter().any(|w| w == "Scheduler"),
        "{:?}",
        words(&mut h)
    );
    // Found as a part of the words drawn.
    let clicked = ask(&mut h, "ui.click", json!({ "text": "chedule" })).unwrap();
    assert_eq!(clicked["clicked"], "Scheduler");
    drain(&mut h);
    assert!(
        h.state()
            .dialogs
            .open_names()
            .iter()
            .any(|d| d.contains("Schedul")),
        "{:?}",
        h.state().dialogs.open_names()
    );
    h.state_mut().dialogs = Default::default();
    drain(&mut h);
    // "Delete Co…" in the toolbar, asked for as a person reads it whole.
    let shortened = words(&mut h)
        .into_iter()
        .find(|w| w.ends_with('…') && w.starts_with("Delete"))
        .expect("the toolbar shortens Delete Completed at this width");
    let clicked = ask(&mut h, "ui.click", json!({ "text": "Delete Completed" })).unwrap();
    assert_eq!(clicked["clicked"], shortened);
    // Too short a beginning is not taken for the rest.
    assert!(ask(&mut h, "ui.click", json!({ "text": "Dexyz" })).is_err());
}

#[test]
fn a_right_click_opens_the_rows_menu_and_its_open_is_refused_by_what_it_says() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    start(&mut h, world.url("file.zip"));
    settle(&mut h, "file.zip to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    drain(&mut h);
    let clicked = ask(
        &mut h,
        "ui.click",
        json!({ "text": "file.zip", "button": "right" }),
    )
    .unwrap();
    assert_eq!(clicked["clicked"], "file.zip");
    drain(&mut h);
    let shown = words(&mut h);
    let open_with = shown
        .iter()
        .find(|w| w.starts_with("Open with"))
        .unwrap_or_else(|| panic!("no menu: {shown:?}"))
        .clone();
    // Asked for by a part of it, it is still what runs a file.
    let refused = ask(&mut h, "ui.click", json!({ "text": "pen wit" })).unwrap_err();
    assert!(refused.contains("is done by a person"), "{refused}");
    assert!(refused.contains(&open_with), "{refused}");
    assert!(world.fake.opened.lock().unwrap().is_empty());
}

#[test]
fn a_double_click_on_a_row_opens_its_progress() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    later(&mut h, vec![world.url("double.bin")]);
    drain(&mut h);
    let clicked = ask(
        &mut h,
        "ui.click",
        json!({ "text": "double.bin", "double": true, "button": "left" }),
    )
    .unwrap();
    assert_eq!(clicked["clicked"], "double.bin");
    let id = id_of(&h, "double.bin");
    settle(&mut h, "its progress window", |app| {
        app.dialogs.progress.contains(&id)
    });
    assert!(h.state().selected.contains(&id));
    assert!(world.fake.opened.lock().unwrap().is_empty());
}

/// A double-click opens a finished download where Options says so: that
/// runs the file, and a script's double-click must not, whatever its words.
#[test]
fn a_scripts_double_click_on_a_finished_row_does_not_run_its_file() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.double_click = "open".into();
    let (mut h, _to_window) = person_window_with(&world, 1100.0, settings);
    start(&mut h, world.url("file.zip"));
    settle(&mut h, "file.zip to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    drain(&mut h);
    ask(
        &mut h,
        "ui.click",
        json!({ "text": "file.zip", "double": true }),
    )
    .unwrap();
    settle(&mut h, "the refusal to be said", |app| {
        app.said.english().contains("done by a person")
    });
    assert!(
        world.fake.opened.lock().unwrap().is_empty(),
        "the file was run"
    );
}

#[test]
fn a_scroll_turns_the_list_under_the_words_it_is_held_over() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    let urls: Vec<String> = (0..80)
        .map(|i| world.url(&format!("row{i:02}.bin")))
        .collect();
    later(&mut h, urls);
    drain(&mut h);
    assert!(shows(&mut h, "row00.bin"), "{:?}", words(&mut h));
    for params in [json!({}), json!({ "text": "" })] {
        let refused = ask(&mut h, "ui.scroll", params).unwrap_err();
        assert!(refused.contains("give `text`"), "{refused}");
    }
    let missing = ask(&mut h, "ui.scroll", json!({ "text": "row99.bin" })).unwrap_err();
    assert!(missing.contains("nothing on screen says"), "{missing}");
    let scrolled = ask(
        &mut h,
        "ui.scroll",
        json!({ "text": "row03.bin", "lines": 12 }),
    )
    .unwrap();
    assert_eq!(scrolled, json!({ "scrolled": "row03.bin", "lines": 12.0 }));
    drain(&mut h);
    assert!(
        !shows(&mut h, "row00.bin"),
        "the list did not turn: {:?}",
        words(&mut h)
    );
    // Three lines unless told.
    let scrolled = ask(&mut h, "ui.scroll", json!({ "text": "bin" })).unwrap();
    assert_eq!(scrolled["lines"], 3.0);
}

#[test]
fn keys_with_modifiers_reach_the_window_and_unknown_keys_are_refused() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    later(&mut h, vec![world.url("k1.bin"), world.url("k2.bin")]);
    drain(&mut h);
    assert!(ask(&mut h, "ui.key", json!({}))
        .unwrap_err()
        .contains("give `key`"));
    assert!(ask(&mut h, "ui.key", json!({ "key": "Hyper" }))
        .unwrap_err()
        .contains("no key is called Hyper"));
    assert!(ask(&mut h, "ui.type", json!({ "text": 5 }))
        .unwrap_err()
        .contains("give `text`"));
    // Select all, as the platform writes it.
    let command = if cfg!(target_os = "macos") {
        "CMD"
    } else {
        "Ctrl"
    };
    let pressed = ask(
        &mut h,
        "ui.key",
        json!({ "key": "A", "modifiers": [command, 7] }),
    )
    .unwrap();
    assert_eq!(pressed["pressed"], "A");
    settle(&mut h, "every row to be selected", |app| {
        app.selected.len() == 2
    });
    // Every modifier is read, whichever the key.
    assert!(ask(
        &mut h,
        "ui.key",
        json!({ "key": "F13", "modifiers": ["alt", "shift", "ctrl", "cmd"] })
    )
    .is_ok());
    drain(&mut h);
}

#[test]
fn steps_wait_for_a_painted_pass_and_a_window_kept_waiting_is_brought_forward() {
    let _one = one_window();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    let ctx = h.ctx.clone();
    ask(&mut h, "ui.key", json!({ "key": "B" })).unwrap();
    let first = h.state_mut().synthetic_events(&ctx);
    assert!(
        matches!(first.as_slice(), [egui::Event::Key { pressed: true, .. }]),
        "{first:?}"
    );
    // No pass painted since: the release waits.
    assert!(h.state_mut().synthetic_events(&ctx).is_empty());
    std::thread::sleep(Duration::from_millis(450));
    assert!(h.state_mut().synthetic_events(&ctx).is_empty());
    h.state_mut().painted();
    let second = h.state_mut().synthetic_events(&ctx);
    assert!(
        matches!(second.as_slice(), [egui::Event::Key { pressed: false, .. }]),
        "{second:?}"
    );
    h.state_mut().painted();
    assert!(h.state_mut().synthetic_events(&ctx).is_empty());
}

#[test]
fn steps_that_waited_too_long_are_dropped_not_replayed() {
    let world = world();
    let mut app = world.app();
    let ctx = egui::Context::default();
    assert!(matches!(
        app.human_call("ui.type", &json!({ "text": "stale" }), None),
        Some(Ok(_))
    ));
    std::thread::sleep(Duration::from_millis(5100));
    assert!(app.synthetic_events(&ctx).is_empty());
    app.painted();
    assert!(app.synthetic_events(&ctx).is_empty(), "replayed later");
}

#[test]
fn a_click_on_words_gone_by_its_pass_lands_on_nothing() {
    let _one = one_window();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    call(&mut h, "command.run", json!({ "id": "program.add_url" })).unwrap();
    drain(&mut h);
    let shown = words(&mut h);
    let cancel = shown
        .iter()
        .find(|w| w.as_str() == "Cancel")
        .unwrap_or_else(|| panic!("no Cancel: {shown:?}"))
        .clone();
    ask(&mut h, "ui.click", json!({ "text": cancel })).unwrap();
    // Closed before the click reaches a pass.
    h.state_mut().dialogs.add_url = None;
    h.step();
    let ctx = h.ctx.clone();
    for _ in 0..5 {
        assert!(h.state_mut().synthetic_events(&ctx).is_empty());
        h.state_mut().painted();
    }
}

#[test]
fn steps_are_capped_and_refused_while_the_window_is_in_the_tray() {
    let world = world();
    let mut app = world.app();
    let mut taken = 0;
    let refused = loop {
        match app.human_call("ui.type", &json!({ "text": "x" }), None) {
            Some(Ok(_)) => taken += 1,
            other => break other,
        }
    };
    // As many as `MAX_WAITING_STEPS` lets wait, and no more.
    assert_eq!(taken, 65);
    assert!(matches!(&refused, Some(Err(why)) if why.contains("too many steps")));
    app.hidden = true;
    for method in ["ui.click", "ui.type", "ui.key", "ui.scroll"] {
        let refused = app.human_call(method, &json!({}), None);
        assert!(
            matches!(&refused, Some(Err(why)) if why.contains("hidden in the tray")),
            "{method}: {refused:?}"
        );
    }
    // Reading the screen is not a step.
    assert!(matches!(
        app.human_call("ui.texts", &json!({}), None),
        Some(Ok(_))
    ));
}

#[test]
fn the_window_is_moved_and_put_away_only_where_there_is_one() {
    let world = world();
    let mut app = world.app();
    for method in ["ui.window", "ui.move_window"] {
        let refused = app.human_call(method, &json!({ "to": "tray" }), None);
        assert!(
            matches!(&refused, Some(Err(why)) if why.contains("there is no window")),
            "{method}: {refused:?}"
        );
    }
    let ctx = egui::Context::default();
    let to = |app: &mut DmApp, to: Value| {
        app.human_call("ui.window", &json!({ "to": to }), Some(&ctx))
            .unwrap()
    };
    assert_eq!(
        to(&mut app, json!("tray")).unwrap(),
        json!({ "to": "tray" })
    );
    assert!(app.hide_asked);
    assert!(to(&mut app, json!("minimized")).is_ok());
    assert!(to(&mut app, json!("shown")).is_ok());
    assert!(app.show_asked);
    assert!(to(&mut app, json!(1)).unwrap_err().contains("give `to`"));
}

#[test]
fn dialogs_in_windows_of_their_own_are_listed_read_and_moved() {
    let _one = one_window();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    // As eframe has it: each dialog a window of its own.
    h.ctx.set_embed_viewports(false);
    let windows = ask(&mut h, "ui.windows", json!({})).unwrap();
    assert!(windows["pixels_per_point"].as_f64().unwrap() > 0.0);
    assert_eq!(windows["dialogs"], json!([]));
    call(&mut h, "command.run", json!({ "id": "program.add_url" })).unwrap();
    drain(&mut h);

    let windows = ask(&mut h, "ui.windows", json!({})).unwrap();
    let dialogs = windows["dialogs"].as_array().unwrap();
    assert!(dialogs.iter().any(|d| d["title"] == "Add URL"), "{windows}");
    let seen = ask(&mut h, "ui.texts", json!({})).unwrap();
    assert!(
        seen["texts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["window"] == "dialog"),
        "{seen}"
    );
    // The front dialog takes the keys.
    ask(&mut h, "ui.key", json!({ "key": "Tab" })).unwrap();
    ask(&mut h, "ui.click", json!({ "text": "Cancel" })).unwrap();
    drain(&mut h);

    let moved = ask(
        &mut h,
        "ui.move_window",
        json!({ "title": "add u", "x": 40, "y": 60.5 }),
    )
    .unwrap();
    assert_eq!(moved, json!({ "moved": "Add URL", "to": [40.0, 60.5] }));
    for (params, why) in [
        (json!({ "x": 1, "y": 1 }), "give `title`"),
        (json!({ "title": "Add URL", "y": 1 }), "give `x` and `y`"),
        (json!({ "title": "Add URL", "x": 1 }), "give `x` and `y`"),
        (
            json!({ "title": "Nowhere", "x": 1, "y": 1 }),
            "no dialog window",
        ),
    ] {
        let refused = ask(&mut h, "ui.move_window", params.clone()).unwrap_err();
        assert!(refused.contains(why), "{params}: {refused}");
    }
    h.state_mut().dialogs = Default::default();
    drain(&mut h);
    h.ctx.set_embed_viewports(true);
    drain(&mut h);
}

#[test]
fn the_screen_reader_tree_is_read_only_where_it_can_be() {
    let world = world();
    let mut app = world.app();
    match app
        .human_call("ui.accessibility", &json!({}), None)
        .unwrap()
    {
        Ok(tree) => assert!(tree["windows"].is_array(), "{tree}"),
        Err(why) => assert!(
            why.contains("main thread") || why.contains("macOS only"),
            "{why}"
        ),
    }
    // Asked without a window, there is nothing to measure.
    let windows = app
        .human_call("ui.windows", &json!({}), None)
        .unwrap()
        .unwrap();
    assert_eq!(windows["pixels_per_point"], Value::Null);
    assert!(app.human_call("queue.list", &json!({}), None).is_none());
}

// The rest of the control protocol (`app/control.rs`).

fn send(
    to_window: &mpsc::Sender<Request>,
    via_mcp: bool,
    method: &str,
    params: Value,
) -> mpsc::Receiver<Result<Value, String>> {
    let (reply, answer) = mpsc::channel();
    to_window
        .send(Request {
            via_mcp,
            method: method.into(),
            params,
            reply,
        })
        .unwrap();
    answer
}

fn answered(
    h: &mut Harness<'_, DmApp>,
    answer: &mpsc::Receiver<Result<Value, String>>,
) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(h);
        if let Ok(said) = answer.try_recv() {
            return said;
        }
        assert!(Instant::now() < deadline, "never answered");
    }
}

#[test]
fn requests_from_the_port_are_answered_on_the_windows_own_pass() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, to_window) = person_window(&world);
    let asked = send(&to_window, false, "ui.texts", json!({}));
    assert!(answered(&mut h, &asked).unwrap()["texts"].is_array());
    let asked = send(&to_window, true, "queue.list", json!({}));
    assert_eq!(answered(&mut h, &asked).unwrap(), json!([]));

    // A picture of the window, under dm's home, by a name made safe.
    let asked = send(
        &to_window,
        false,
        "ui.screenshot",
        json!({ "name": "../../the shot" }),
    );
    let path = answered(&mut h, &asked).unwrap()["path"]
        .as_str()
        .unwrap()
        .to_string();
    let path = std::path::PathBuf::from(path);
    assert!(
        path.starts_with(dm_shell::home().join("screenshots")),
        "{path:?}"
    );
    assert!(path.is_file(), "{path:?}");
    let picture = image::open(&path).unwrap();
    assert!(picture.width() > 100 && picture.height() > 100);
    let asked = send(&to_window, false, "ui.screenshot", json!({ "name": "///" }));
    let path = answered(&mut h, &asked).unwrap()["path"].clone();
    assert!(path.as_str().unwrap().ends_with("dm.png"), "{path}");

    h.state_mut().headless = true;
    let asked = send(&to_window, false, "ui.screenshot", json!({}));
    assert!(answered(&mut h, &asked).unwrap_err().contains("headless"));
    h.state_mut().headless = false;

    // AI assistants only while Options lets them.
    let mut off = h.state().applied.clone();
    off.allow_mcp = false;
    h.state_mut().apply_settings(off).unwrap();
    let asked = send(&to_window, true, "queue.list", json!({}));
    assert!(answered(&mut h, &asked).unwrap_err().contains("turned off"));
    let asked = send(&to_window, false, "queue.list", json!({}));
    assert!(answered(&mut h, &asked).is_ok());
    let mut on = h.state().applied.clone();
    on.allow_mcp = true;
    h.state_mut().apply_settings(on).unwrap();

    // A port opened for assistants answers nobody else.
    h.state_mut().restrict_control_to_assistants(true);
    let asked = send(&to_window, false, "queue.list", json!({}));
    assert!(answered(&mut h, &asked)
        .unwrap_err()
        .contains("AI assistants only"));
    let asked = send(&to_window, true, "nonsense", json!({}));
    assert!(answered(&mut h, &asked)
        .unwrap_err()
        .contains("no method is called nonsense"));
}

#[test]
fn commands_are_listed_and_run_and_what_cannot_run_is_said() {
    let _one = one_window();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    let listed = call(&mut h, "commands.list", json!({})).unwrap();
    let listed = listed.as_array().unwrap();
    let remove = listed.iter().find(|c| c["id"] == "file.remove").unwrap();
    assert_eq!(remove["enabled"], false, "nothing is selected");
    assert!(listed
        .iter()
        .all(|c| c["label"].is_string() && c["enabled"].is_boolean()));

    let unknown = call(&mut h, "command.run", json!({ "id": "file.nonsense" })).unwrap_err();
    assert!(unknown.contains("no command is called file.nonsense"));
    let none = call(&mut h, "command.run", json!({})).unwrap_err();
    assert!(none.contains("no command is called"), "{none}");
    let refused = call(&mut h, "command.run", json!({ "id": "file.preview" }));
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("by a person")),
        "{refused:?}"
    );
    let idle = call(&mut h, "command.run", json!({ "id": "file.remove" })).unwrap_err();
    assert!(idle.contains("has nothing to act on"), "{idle}");

    let ran = call(&mut h, "command.run", json!({ "id": "program.add_url" })).unwrap();
    assert_eq!(ran["ran"], "program.add_url");
    assert!(ran["said"].is_string());
    assert!(h.state().dialogs.add_url.is_some());

    // Exit is not asked about over the port: dm quits.
    assert!(!h.state().quit_asked);
    let quit = call(&mut h, "command.run", json!({ "id": "program.exit" })).unwrap();
    assert_eq!(quit["ran"], "program.exit");
    assert!(h.state().quit_asked);

    assert!(call(&mut h, "nonsense", json!({}))
        .unwrap_err()
        .contains("no method is called nonsense"));
}

#[test]
fn the_selection_and_the_place_shown_are_set_and_read_back() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    later(&mut h, vec![world.url("one.mp4"), world.url("two.zip")]);
    let one = id_of(&h, "one.mp4");
    let picked = call(&mut h, "ui.select", json!({ "ids": [one, 424242, "x"] })).unwrap();
    assert_eq!(picked, json!({ "selected": [one] }));
    assert!(h.state().selected.contains(&one));
    assert_eq!(
        call(&mut h, "ui.select", json!({})).unwrap(),
        json!({ "selected": [] })
    );

    for place in [
        "all",
        "unfinished",
        "finished",
        "kind:videos",
        "queue:Main queue",
    ] {
        let said = call(&mut h, "ui.place", json!({ "place": place })).unwrap();
        assert_eq!(said["place"], place);
    }
    assert!(matches!(h.state().place, Place::Queue(ref q) if q == "Main queue"));
    for (place, why) in [
        ("queue:Nowhere", "no queue is called Nowhere"),
        ("kind:nothing", "no category is called nothing"),
        ("custom:Mine", "no category is called Mine"),
        ("somewhere", "no place is called somewhere"),
        ("", "no place is called"),
    ] {
        let refused = call(&mut h, "ui.place", json!({ "place": place })).unwrap_err();
        assert!(refused.contains(why), "{place}: {refused}");
    }
    let mut wanted = h.state().applied.clone();
    wanted.custom_categories.push(dm_core::Custom {
        name: "Mine".into(),
        extensions: vec!["zip".into()],
        sites: Vec::new(),
    });
    h.state_mut().apply_settings(wanted).unwrap();
    call(&mut h, "ui.place", json!({ "place": "custom:Mine" })).unwrap();
    assert!(matches!(h.state().place, Place::Custom(ref c) if c == "Mine"));

    call(&mut h, "ui.place", json!({ "place": "all" })).unwrap();
    frames(&mut h, 3);
    let seen = call(&mut h, "ui.inspect", json!({})).unwrap();
    assert_eq!(seen["place"], "All");
    assert_eq!(seen["hidden"], false);
    assert!(seen["dialogs"].as_array().unwrap().is_empty(), "{seen}");
    let rows = seen["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{seen}");
    let first = rows.iter().find(|r| r["id"] == one).unwrap();
    assert_eq!(first["name"], "one.mp4");
    assert_eq!(first["queue"], "Main queue");
    assert!(first["url"].as_str().unwrap().ends_with("one.mp4"));
    assert_eq!(first["rect"].as_array().map(Vec::len), Some(4), "{first}");
}

#[test]
fn the_queue_is_listed_without_any_sign_in() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    let url = world.url("signed.bin");
    let cookies = ["session", "=", "abc"].concat();
    h.state_mut()
        .add(
            std::slice::from_ref(&url),
            dm_core::AddOptions {
                later: true,
                queue: "Main queue".into(),
                cookies: cookies.clone(),
                headers: vec![("X-Test".into(), "value".into())],
                ..Default::default()
            },
        )
        .unwrap();
    later(&mut h, vec![world.url("plain.bin")]);
    let listed = call(&mut h, "queue.list", json!({})).unwrap();
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 2);
    for row in listed {
        assert!(row.get("cookies").is_none() && row.get("headers").is_none());
        assert!(!row.to_string().contains(&cookies), "{row}");
        let signed = row["url"].as_str().unwrap().ends_with("signed.bin");
        assert_eq!(row["signed_in"], signed, "{row}");
    }
}

#[test]
fn downloads_are_paused_resumed_and_cancelled_by_id_and_what_cannot_be_is_said() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    for method in ["downloads.pause", "downloads.resume", "downloads.cancel"] {
        for params in [json!({}), json!({ "ids": [] }), json!({ "ids": ["a"] })] {
            assert!(call(&mut h, method, params)
                .unwrap_err()
                .contains("give `ids`"));
        }
    }
    start(&mut h, world.url("file.zip"));
    start(&mut h, world.url("gone.iso"));
    start(&mut h, world.url("stalled.bin"));
    let (done, gone, running) = (
        id_of(&h, "file.zip"),
        id_of(&h, "gone.iso"),
        id_of(&h, "stalled.bin"),
    );
    settle(&mut h, "one done, one failed, one running", |app| {
        let is =
            |id, want: fn(&State) -> bool| app.backend().row(id).is_some_and(|r| want(&r.state));
        is(done, |s| *s == State::Completed)
            && is(gone, |s| matches!(s, State::Failed(_)))
            && is(running, |s| *s == State::Running)
    });
    let ids = json!({ "ids": [done, gone, running, 99999] });
    let why = |said: &Value, id: u64| {
        said["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .map(|s| s["why"].as_str().unwrap().to_string())
    };

    let said = call(&mut h, "downloads.resume", ids.clone()).unwrap();
    assert_eq!(said["done"], json!([gone]), "a failed one is tried again");
    assert_eq!(
        why(&said, running).as_deref(),
        Some("it is not paused or failed")
    );
    assert_eq!(
        why(&said, done).as_deref(),
        Some("it is not paused or failed")
    );
    assert_eq!(why(&said, 99999).as_deref(), Some("no such download"));

    let said = call(&mut h, "downloads.pause", json!({ "ids": [done, running] })).unwrap();
    assert_eq!(said["done"], json!([running]));
    assert_eq!(why(&said, done).as_deref(), Some("it is not running"));
    assert_eq!(state_of(&h, running), Some(State::Paused));

    // Paused by a person: resumed.
    let said = call(&mut h, "downloads.resume", json!({ "ids": [running] })).unwrap();
    assert_eq!(said["done"], json!([running]));
    settle(&mut h, "it to run again", |app| {
        app.backend()
            .row(running)
            .is_some_and(|r| r.state == State::Running)
    });

    let said = call(
        &mut h,
        "downloads.cancel",
        json!({ "ids": [done, running] }),
    )
    .unwrap();
    assert_eq!(said["done"], json!([running]));
    assert_eq!(why(&said, done).as_deref(), Some("it is already settled"));
    settle(&mut h, "the cancel", |app| {
        app.backend()
            .row(running)
            .is_some_and(|r| r.state == State::Cancelled)
    });
    let said = call(&mut h, "downloads.cancel", json!({ "ids": [running] })).unwrap();
    assert_eq!(
        why(&said, running).as_deref(),
        Some("it is already settled")
    );
}

#[test]
fn a_download_the_schedule_holds_is_not_resumed_over_the_port() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    // Closed until a day far off.
    settings.schedule = dm_core::Schedule {
        enabled: true,
        once_at: Some("2099-01-01 00:00".into()),
        to: "01:00".into(),
        ..dm_core::Schedule::default()
    };
    let (mut h, _to_window) = person_window_with(&world, 1100.0, settings);
    start(&mut h, world.url("stalled.bin"));
    let id = id_of(&h, "stalled.bin");
    settle(&mut h, "the schedule to hold it", |app| {
        app.backend()
            .row(id)
            .is_some_and(|r| r.state == State::Paused && r.paused_by_schedule)
    });
    let said = call(&mut h, "downloads.resume", json!({ "ids": [id] })).unwrap();
    assert_eq!(said["done"], json!([]));
    assert_eq!(
        said["skipped"][0]["why"],
        "the schedule or the download limit holds it"
    );
    assert_eq!(state_of(&h, id), Some(State::Paused));
}

#[test]
fn the_speed_limit_is_set_exactly_and_the_one_before_is_kept() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    for params in [json!({}), json!({ "kb": -1 }), json!({ "kb": "fast" })] {
        assert!(call(&mut h, "downloads.speed_limit", params)
            .unwrap_err()
            .contains("give `kb`"));
    }
    assert!(call(
        &mut h,
        "downloads.speed_limit",
        json!({ "kb": 100_000_001 })
    )
    .unwrap_err()
    .contains("at most"));
    let set = call(&mut h, "downloads.speed_limit", json!({ "kb": 500 })).unwrap();
    assert_eq!(set, json!({ "speed_limit_kb": 500 }));
    assert_eq!(h.state().applied.speed_limit, 500);
    let off = call(&mut h, "downloads.speed_limit", json!({ "kb": 0 })).unwrap();
    assert_eq!(off, json!({ "speed_limit_kb": 0 }));
    assert_eq!(h.state().applied.speed_limit, 0);
    assert_eq!(h.state().applied.last_speed_limit, 500, "the one before");
    let saved = h.state().backend().settings();
    assert_eq!(saved.speed_limit, 0);
}

#[test]
fn downloads_are_added_where_they_may_go_with_their_names_and_parts() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    let add = |h: &mut Harness<'_, DmApp>, params: Value| call(h, "downloads.add", params);
    for params in [json!({}), json!({ "urls": 4 }), json!({ "url": null })] {
        assert!(add(&mut h, params).unwrap_err().contains("give `urls`"));
    }
    for params in [
        json!({ "urls": [] }),
        json!({ "urls": [" ", 7] }),
        json!({ "url": "" }),
    ] {
        let refused = add(&mut h, params.clone()).unwrap_err();
        assert!(
            refused.contains("that list is empty"),
            "{params}: {refused}"
        );
    }
    // One link as `url`.
    let said = add(
        &mut h,
        json!({ "url": world.url("single.bin"), "later": true, "description": "the one" }),
    )
    .unwrap();
    assert_eq!(said, json!({ "added": 1, "already_listed": [] }));
    h.state_mut().refresh();
    let row = h.state().row(id_of(&h, "single.bin")).unwrap().clone();
    assert_eq!(row.description, "the one");
    assert_eq!(row.queue, "Main queue");

    // Listed already, and named once however often it is asked for.
    let twice = world.url("single.bin");
    let said = add(
        &mut h,
        json!({ "urls": [twice, twice, twice], "later": true }),
    )
    .unwrap();
    assert_eq!(said, json!({ "added": 0, "already_listed": [twice] }));
    // Unless asked again.
    let said = add(
        &mut h,
        json!({ "urls": [twice], "later": true, "again": true }),
    )
    .unwrap();
    assert_eq!(said, json!({ "added": 1, "already_listed": [] }));
    h.state_mut().refresh();
    let copies = h
        .state()
        .rows()
        .iter()
        .filter(|r| r.url.ends_with("single.bin"))
        .count();
    assert_eq!(copies, 2);

    let inside = world.dir.path().join("out").join("sub");
    let outside = world.dir.path().join("elsewhere");
    let up = world.dir.path().join("out").join("..").join("elsewhere");
    for (folder, why) in [
        ("relative/folder".to_string(), "must be a full path"),
        (outside.display().to_string(), "inside the download folder"),
        (up.display().to_string(), "inside the download folder"),
    ] {
        let refused = add(
            &mut h,
            json!({ "urls": [world.url("f.bin")], "later": true, "folder": folder }),
        )
        .unwrap_err();
        assert!(refused.contains(why), "{folder}: {refused}");
    }
    let said = add(
        &mut h,
        json!({ "urls": [world.url("f.bin")], "later": true,
                "folder": inside.display().to_string(), "name": "renamed.bin" }),
    )
    .unwrap();
    assert_eq!(said["added"], 1, "{said}");
    h.state_mut().refresh();
    let row = h.state().row(id_of(&h, "f.bin")).unwrap().clone();
    assert!(
        row.save_dir.as_deref().is_some_and(|d| d.ends_with("sub")),
        "{:?}",
        row.save_dir
    );

    // A part of a video: both ends, the end after the start.
    for (span, why) in [
        (json!({ "from": "1:00" }), "give both"),
        (json!({ "to": "1:00" }), "give both"),
        (json!({ "from": "2:00", "to": "1:00" }), "times such as"),
        (json!({ "from": "soon", "to": "later" }), "times such as"),
    ] {
        let mut params = span.clone();
        params["urls"] = json!([world.url("clip.mp4")]);
        params["later"] = json!(true);
        assert!(add(&mut h, params).unwrap_err().contains(why), "{span}");
    }
    let said = add(
        &mut h,
        json!({ "urls": [world.url("clip.mp4")], "later": true, "from": "0:10", "to": "75" }),
    )
    .unwrap();
    assert_eq!(said["added"], 1);
    h.state_mut().refresh();
    let row = h.state().row(id_of(&h, "clip.mp4")).unwrap().clone();
    assert!(row.span.is_some(), "the part was not kept");
}

#[test]
fn a_batch_answers_each_step_and_refuses_what_cannot_be_batched() {
    let _one = one_window();
    let world = world();
    let (mut h, _to_window) = person_window(&world);
    for params in [json!({}), json!({ "steps": "ui.inspect" })] {
        assert!(call(&mut h, "batch", params)
            .unwrap_err()
            .contains("give `steps`"));
    }
    let said = call(
        &mut h,
        "batch",
        json!({ "steps": [
            { "method": "batch", "params": { "steps": [] } },
            { "method": "ui.screenshot" },
            { "method": "queue.list" },
            { "method": "ui.select" },
            {},
        ]}),
    )
    .unwrap();
    let said = said.as_array().unwrap();
    assert_eq!(said[0]["error"], "batch cannot be batched");
    assert_eq!(said[1]["error"], "ui.screenshot cannot be batched");
    assert_eq!(said[2], json!({ "ok": true, "result": [] }));
    assert_eq!(said[3]["result"], json!({ "selected": [] }));
    assert_eq!(said[4]["ok"], false);
    let full = json!({ "steps": vec![json!({ "method": "queue.list" }); 256] });
    assert_eq!(
        call(&mut h, "batch", full)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        256
    );
    let over = json!({ "steps": vec![json!({ "method": "queue.list" }); 257] });
    assert!(call(&mut h, "batch", over)
        .unwrap_err()
        .contains("at most 256"));
}

// The AI client's tools (`mcp.rs`).

/// A stand-in for dm's control port: takes connections one after another,
/// answers each line as `reply` says, and records what it was asked.
struct FakeDm {
    port: u16,
    asked: Arc<Mutex<Vec<(String, Value)>>>,
    connections: Arc<std::sync::atomic::AtomicUsize>,
}

/// What the stand-in does with one request.
enum Reply {
    /// Answers with this line.
    Line(String),
    /// Answers with two lines at once.
    Two(String, String),
    /// Answers, and a moment later sends another line unasked.
    Late(String, String),
    /// Reads it and says nothing, the connection kept.
    Silent,
    /// Closes the connection without answering.
    Hang,
    /// Closes it without reading the request, which resets it.
    Reset,
}

fn fake_dm(reply: impl Fn(&str, &Value) -> Reply + Send + Sync + 'static) -> FakeDm {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (log, count) = (Arc::clone(&asked), Arc::clone(&connections));
    let reply = Arc::new(reply);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (log, reply) = (Arc::clone(&log), Arc::clone(&reply));
            std::thread::spawn(move || {
                let mut out = stream.try_clone().unwrap();
                // Unbuffered: a line read is that request and no more.
                let mut reader = stream;
                loop {
                    // The request seen whole before it is read, so a reset
                    // can leave it unread.
                    let mut peeked = vec![0u8; 64 * 1024];
                    let (first, length) = loop {
                        let seen = reader.peek(&mut peeked).unwrap_or(0);
                        if seen == 0 {
                            return;
                        }
                        if let Some(end) = peeked[..seen].iter().position(|b| *b == b'\n') {
                            let first =
                                serde_json::from_slice::<Value>(&peeked[..end]).unwrap_or_default();
                            break (first, end + 1);
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    };
                    let method = first["method"].as_str().unwrap_or("").to_string();
                    let params = first["params"].clone();
                    let what = reply(&method, &params);
                    if matches!(what, Reply::Reset) {
                        return;
                    }
                    let mut line = vec![0u8; length];
                    if std::io::Read::read_exact(&mut reader, &mut line).is_err() {
                        return;
                    }
                    log.lock().unwrap().push((method, params));
                    match what {
                        Reply::Line(one) => {
                            let _ = writeln!(out, "{one}");
                        }
                        Reply::Two(one, two) => {
                            let _ = write!(out, "{one}\n{two}\n");
                        }
                        Reply::Late(one, two) => {
                            let _ = writeln!(out, "{one}");
                            std::thread::sleep(Duration::from_millis(100));
                            let _ = writeln!(out, "{two}");
                        }
                        Reply::Silent => {}
                        Reply::Hang | Reply::Reset => return,
                    }
                }
            });
        }
    });
    FakeDm {
        port,
        asked,
        connections,
    }
}

fn ok(result: Value) -> Reply {
    Reply::Line(json!({ "id": 1, "ok": true, "result": result }).to_string())
}

fn refused(why: &str) -> Reply {
    Reply::Line(json!({ "id": 1, "ok": false, "error": why }).to_string())
}

/// A stand-in that lets everyone in and echoes the method it was asked.
fn echoing_dm() -> FakeDm {
    fake_dm(|method, params| match method {
        "auth" => ok(json!({ "client": params["client"] })),
        "ui.select" if params["ids"] == json!([666]) => refused("no such rows"),
        _ => ok(json!({ "method": method })),
    })
}

fn token() -> String {
    "t".repeat(64)
}

impl FakeDm {
    fn asked(&self) -> Vec<(String, Value)> {
        self.asked.lock().unwrap().clone()
    }

    fn methods(&self) -> Vec<String> {
        self.asked().into_iter().map(|(m, _)| m).collect()
    }

    fn connections(&self) -> usize {
        self.connections.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn clear(&self) {
        self.asked.lock().unwrap().clear();
    }
}

/// The MCP replies to these lines, in order.
fn serve(control: &mut Control, lines: &[Value]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out = Vec::new();
    mcp::serve(std::io::Cursor::new(input), &mut out, control).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn tool(id: u64, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

#[test]
fn the_mcp_server_answers_its_protocol_and_lists_every_tool() {
    let dm = echoing_dm();
    let mut control = Control::connect(dm.port, &token()).unwrap();
    assert_eq!(dm.asked()[0].1["client"], "mcp", "said to be an assistant");
    let input = format!(
        "{}\n\n   \n{}\nnot json\n{}\n{}\n{}\n",
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": "two", "method": "ping" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
    );
    let mut out = Vec::new();
    mcp::serve(std::io::Cursor::new(input), &mut out, &mut control).unwrap();
    let replies: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 5, "{replies:?}");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "dm");
    assert!(replies[0]["result"]["capabilities"]["tools"].is_object());
    assert_eq!(replies[1]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], Value::Null);
    assert_eq!(
        replies[2],
        json!({ "jsonrpc": "2.0", "id": "two", "result": {} })
    );
    let tools = replies[3]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for name in [
        "dm_list_commands",
        "dm_run_command",
        "dm_add_downloads",
        "dm_list_downloads",
        "dm_select",
        "dm_pause",
        "dm_resume",
        "dm_cancel",
        "dm_set_speed_limit",
        "dm_move",
        "dm_queue",
        "dm_screen",
        "dm_click",
        "dm_type",
        "dm_press_key",
        "dm_inspect",
        "dm_screenshot",
    ] {
        assert!(names.contains(&name), "{name} missing: {names:?}");
    }
    assert_eq!(names.len(), 17);
    for tool in tools {
        assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
    assert_eq!(replies[4]["error"]["code"], -32601);
    assert!(replies[4]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("resources/list"));
    // The client's own protocol version is said back.
    let said = serve(
        &mut control,
        &[json!({ "jsonrpc": "2.0", "id": 9, "method": "initialize",
                  "params": { "protocolVersion": "2024-11-05" } })],
    );
    assert_eq!(said[0]["result"]["protocolVersion"], "2024-11-05");
    // Only the auth reached dm.
    assert_eq!(dm.methods(), ["auth"]);
}

#[test]
fn each_tool_is_the_control_calls_it_stands_for() {
    let dm = echoing_dm();
    let mut control = Control::connect(dm.port, &token()).unwrap();
    // A tool, its arguments, and the control calls it is.
    type Case = (&'static str, Value, Vec<(&'static str, Value)>);
    let cases: Vec<Case> = vec![
        (
            "dm_list_commands",
            json!({}),
            vec![("commands.list", json!({}))],
        ),
        (
            "dm_run_command",
            json!({ "id": "downloads.start_queue" }),
            vec![("command.run", json!({ "id": "downloads.start_queue" }))],
        ),
        (
            "dm_add_downloads",
            json!({ "urls": ["http://x.invalid/a"], "later": true }),
            vec![(
                "downloads.add",
                json!({ "urls": ["http://x.invalid/a"], "later": true }),
            )],
        ),
        (
            "dm_list_downloads",
            json!({}),
            vec![("queue.list", json!({}))],
        ),
        (
            "dm_select",
            json!({ "ids": [1] }),
            vec![("ui.select", json!({ "ids": [1] }))],
        ),
        ("dm_inspect", json!({}), vec![("ui.inspect", json!({}))]),
        (
            "dm_screenshot",
            json!({ "name": "n" }),
            vec![("ui.screenshot", json!({ "name": "n" }))],
        ),
        ("dm_screen", json!({}), vec![("ui.texts", json!({}))]),
        (
            "dm_type",
            json!({ "text": "x" }),
            vec![("ui.type", json!({ "text": "x" }))],
        ),
        (
            "dm_press_key",
            json!({ "key": "Escape" }),
            vec![("ui.key", json!({ "key": "Escape" }))],
        ),
        (
            "dm_pause",
            json!({ "ids": null }),
            vec![("command.run", json!({ "id": "downloads.stop_all" }))],
        ),
        (
            "dm_resume",
            json!({ "ids": [2] }),
            vec![("downloads.resume", json!({ "ids": [2] }))],
        ),
        (
            "dm_cancel",
            json!({ "ids": [3] }),
            vec![("downloads.cancel", json!({ "ids": [3] }))],
        ),
        (
            "dm_move",
            json!({ "ids": [4], "direction": "up" }),
            vec![
                ("ui.select", json!({ "ids": [4] })),
                ("command.run", json!({ "id": "file.move_up" })),
            ],
        ),
        (
            "dm_queue",
            json!({ "action": "start" }),
            vec![("command.run", json!({ "id": "downloads.start_queue" }))],
        ),
        (
            "dm_queue",
            json!({ "action": "stop" }),
            vec![("command.run", json!({ "id": "downloads.stop_queue" }))],
        ),
        (
            "dm_move",
            json!({ "ids": [5, 6], "direction": "down" }),
            vec![
                ("ui.select", json!({ "ids": [5, 6] })),
                ("command.run", json!({ "id": "file.move_down" })),
            ],
        ),
        (
            "dm_pause",
            json!({ "ids": [7] }),
            vec![("downloads.pause", json!({ "ids": [7] }))],
        ),
        (
            "dm_resume",
            json!({}),
            vec![("command.run", json!({ "id": "downloads.resume_all" }))],
        ),
        (
            "dm_set_speed_limit",
            json!({ "preset": "unlimited" }),
            vec![("command.run", json!({ "id": "downloads.speed.unlimited" }))],
        ),
        (
            "dm_set_speed_limit",
            json!({ "kb": 0, "preset": "low" }),
            vec![("downloads.speed_limit", json!({ "kb": 0 }))],
        ),
    ];
    for (id, (name, arguments, wanted)) in cases.into_iter().enumerate() {
        dm.clear();
        let said = serve(&mut control, &[tool(id as u64, name, arguments.clone())]);
        let result = &said[0]["result"];
        assert_eq!(said[0]["id"], id as u64);
        assert_eq!(result["isError"], Value::Null, "{name}: {result}");
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(result["content"][0]["type"], "text");
        // The last call's answer is the tool's.
        assert_eq!(text["method"], wanted.last().unwrap().0, "{name}");
        let asked: Vec<(String, Value)> = wanted
            .into_iter()
            .map(|(m, p)| (m.to_string(), p))
            .collect();
        assert_eq!(dm.asked(), asked, "{name} {arguments}");
    }
    // No arguments at all is none given.
    dm.clear();
    let said = serve(
        &mut control,
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                  "params": { "name": "dm_list_downloads" } })],
    );
    assert_eq!(said[0]["result"]["isError"], Value::Null);
    assert_eq!(dm.asked(), [("queue.list".to_string(), json!({}))]);
}

#[test]
fn a_tool_that_fails_is_a_result_the_model_reads_and_stops_where_it_failed() {
    let dm = echoing_dm();
    let mut control = Control::connect(dm.port, &token()).unwrap();
    dm.clear();
    let said = serve(
        &mut control,
        &[
            tool(1, "dm_move", json!({ "ids": [666], "direction": "down" })),
            tool(2, "dm_cancel", json!({})),
            tool(3, "dm_move", json!({ "ids": [1], "direction": "sideways" })),
            tool(4, "dm_queue", json!({})),
            tool(5, "dm_set_speed_limit", json!({})),
            tool(6, "dm_teleport", json!({})),
            json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call" }),
            tool(8, "dm_move", json!({ "ids": [], "direction": "up" })),
            tool(9, "dm_cancel", json!({ "ids": null })),
        ],
    );
    let text = |n: usize| said[n]["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(said[0]["result"]["isError"], true);
    assert_eq!(text(0), "no such rows");
    // The select failed: the move was not run on whatever was selected.
    assert_eq!(dm.methods(), ["ui.select"]);
    assert_eq!(text(1), "give `ids`: the downloads to cancel");
    assert_eq!(text(2), "direction is up or down");
    assert_eq!(text(3), "action is start or stop");
    assert!(text(4).contains("preset is low, medium, high or unlimited"));
    for one in &said[1..5] {
        assert_eq!(one["result"]["isError"], true, "{one}");
    }
    assert_eq!(said[5]["error"]["code"], -32602);
    assert_eq!(said[5]["error"]["message"], "no tool is called dm_teleport");
    assert_eq!(said[6]["error"]["message"], "no tool is called ");
    assert_eq!(text(7), "give `ids`: the downloads to move");
    assert_eq!(text(8), "give `ids`: the downloads to cancel");
    assert_eq!(dm.methods(), ["ui.select"], "nothing more reached dm");
}

#[test]
fn a_connection_is_refused_with_why_when_dm_is_not_there_or_says_no() {
    // A port nothing listens on.
    let closed = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let why = Control::connect(closed, &token()).err().unwrap();
    assert!(why.contains("dm is not running"), "{why}");
    assert!(why.contains(&closed.to_string()), "{why}");

    let wrong = fake_dm(|_, _| refused("wrong token"));
    let why = Control::connect_as(wrong.port, "nope", None).err().unwrap();
    assert_eq!(why, "wrong token");
    let silent = fake_dm(|_, _| Reply::Line(json!({ "ok": false }).to_string()));
    assert_eq!(
        Control::connect(silent.port, "x").err().unwrap(),
        "dm refused"
    );

    let lost = Control::with(Box::new(|| Err("nowhere to look".into())));
    assert_eq!(lost.err().unwrap(), "nowhere to look");

    // `dm mcp` with nothing to talk to ends at once, saying so.
    let home = tempfile::tempdir().unwrap();
    assert_eq!(mcp::main(&[], home.path()), 2);
    let tokens = home.path().join("token");
    std::fs::write(&tokens, token()).unwrap();
    let args: Vec<String> = [
        "mcp",
        "--port",
        &closed.to_string(),
        "--token-file",
        tokens.to_str().unwrap(),
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(mcp::main(&args, home.path()), 2);
}

#[test]
fn the_port_and_token_are_found_in_the_options_or_in_dms_home() {
    let home = tempfile::tempdir().unwrap();
    let nothing = mcp::finder(home.path(), None, None);
    assert!(nothing().unwrap_err().contains("no running dm"));
    std::fs::write(home.path().join("control.port"), " 4321\n").unwrap();
    assert!(nothing().is_err(), "a port without a token");
    std::fs::write(home.path().join("control.token"), "  sesame \n").unwrap();
    assert_eq!(nothing().unwrap(), (4321, "sesame".to_string()));
    // The options before the files, read again each time.
    let other = home.path().join("other.token");
    std::fs::write(&other, "other").unwrap();
    let given = mcp::finder(
        home.path(),
        Some("1234".into()),
        Some(other.display().to_string()),
    );
    assert_eq!(given().unwrap(), (1234, "other".to_string()));
    std::fs::write(&other, "changed").unwrap();
    assert_eq!(given().unwrap().1, "changed");
    let garbled = mcp::finder(home.path(), Some("port".into()), None);
    assert!(garbled().is_err());
    std::fs::write(home.path().join("control.port"), "99999").unwrap();
    assert!(nothing().is_err(), "no such port");

    // Found that way, and connected as an assistant.
    let dm = echoing_dm();
    std::fs::write(home.path().join("control.port"), dm.port.to_string()).unwrap();
    let mut control = Control::as_assistant(mcp::finder(home.path(), None, None)).unwrap();
    assert_eq!(
        dm.asked()[0].1,
        json!({ "token": "sesame", "client": "mcp" })
    );
    assert_eq!(
        control.call("queue.list", json!({})).unwrap()["method"],
        "queue.list"
    );
}

#[test]
fn a_connection_lost_mid_call_is_made_again_for_the_next() {
    let dm = fake_dm(|method, params| match (method, params["n"].as_u64()) {
        ("auth", _) => ok(json!("in")),
        (_, Some(1)) => Reply::Hang,
        (_, Some(2)) => Reply::Reset,
        (_, Some(3)) => Reply::Line("not json".into()),
        (_, Some(4)) => Reply::Two(
            json!({ "ok": true, "result": "four" }).to_string(),
            json!({ "ok": true, "result": "early" }).to_string(),
        ),
        (_, Some(5)) => Reply::Line(json!({ "ok": true }).to_string()),
        (_, Some(6)) => refused("no"),
        (_, Some(7)) => Reply::Late(
            json!({ "ok": true, "result": "seven" }).to_string(),
            json!({ "ok": true, "result": "unasked" }).to_string(),
        ),
        // Answered already, by the second line of the answer before.
        ("w", _) => Reply::Silent,
        _ => ok(json!(method)),
    });
    let mut control = Control::with(Box::new({
        let port = dm.port;
        move || Ok((port, token()))
    }))
    .unwrap();
    assert_eq!(dm.asked()[0].1["client"], Value::Null, "not an assistant");
    assert_eq!(control.call("x", json!({})).unwrap(), json!("x"));

    // Closed without an answer: said, and the next call connects again.
    let lost = control.call("x", json!({ "n": 1 })).unwrap_err();
    assert_eq!(lost, "dm closed the connection");
    assert_eq!(control.call("y", json!({})).unwrap(), json!("y"));
    assert_eq!(dm.connections(), 2);

    // Reset with the request unread.
    assert!(control.call("x", json!({ "n": 2 })).is_err());
    assert_eq!(control.call("z", json!({})).unwrap(), json!("z"));
    assert_eq!(dm.connections(), 3);

    // An answer that is not JSON.
    assert_eq!(
        control.call("x", json!({ "n": 3 })).unwrap_err(),
        "dm closed the connection"
    );

    // Two answers to one request: the second, already read, is taken as
    // the next call's, on the same connection.
    let before = dm.connections();
    assert_eq!(control.call("x", json!({ "n": 4 })).unwrap(), json!("four"));
    assert_eq!(control.call("w", json!({})).unwrap(), json!("early"));
    assert_eq!(dm.connections(), before);

    // An answer with nothing in it, and a refusal that keeps the connection.
    assert_eq!(control.call("x", json!({ "n": 5 })).unwrap(), Value::Null);
    assert_eq!(control.call("x", json!({ "n": 6 })).unwrap_err(), "no");
    assert_eq!(control.call("v", json!({})).unwrap(), json!("v"));
    assert_eq!(dm.connections(), before);

    // A line sent unasked after an answer, still unread: the connection is
    // there, and the line is taken as the next answer.
    assert_eq!(
        control.call("x", json!({ "n": 7 })).unwrap(),
        json!("seven")
    );
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(control.call("w", json!({})).unwrap(), json!("unasked"));
    assert_eq!(dm.connections(), before);

    // A person's step waits for the frames that draw it.
    let at = Instant::now();
    assert!(control.call("ui.click", json!({ "text": "a" })).is_ok());
    assert!(at.elapsed() >= Duration::from_millis(300));
}

#[test]
fn an_assistant_drives_the_window_through_its_tools() {
    let _one = one_window();
    let _file = settings_file();
    let world = world();
    let (mut h, to_window) = person_window(&world);
    let secret = token();
    let port = dm_desktop::control::listen(0, secret.clone(), to_window, || {}).unwrap();
    let url = world.url("tooled.bin");
    let client = std::thread::spawn(move || {
        let mut control = Control::connect(port, &secret).unwrap();
        serve(
            &mut control,
            &[
                tool(
                    1,
                    "dm_add_downloads",
                    json!({ "urls": [url], "later": true }),
                ),
                tool(2, "dm_screen", json!({})),
                tool(3, "dm_click", json!({ "text": "Add URL" })),
                tool(4, "dm_click", json!({ "text": "Open" })),
                tool(5, "dm_inspect", json!({})),
            ],
        )
    });
    while !client.is_finished() {
        frame(&mut h);
        std::thread::sleep(Duration::from_millis(5));
    }
    let said = client.join().unwrap();
    let text = |n: usize| said[n]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text(0).contains("\"added\":1"), "{}", said[0]);
    assert!(text(1).contains("Add URL"), "{}", said[1]);
    assert!(text(2).contains("\"clicked\":\"Add URL\""), "{}", said[2]);
    assert_eq!(said[3]["result"]["isError"], true);
    assert!(text(3).contains("runs a downloaded file"));
    assert!(
        text(4).contains("Add URL"),
        "the dialog is open: {}",
        said[4]
    );
    settle(&mut h, "Add URL", |app| app.dialogs.add_url.is_some());
    assert_eq!(h.state().rows().len(), 1);
}
