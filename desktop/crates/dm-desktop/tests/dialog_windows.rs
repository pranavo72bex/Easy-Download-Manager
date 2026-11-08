// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every dialog as a window of its own (`dialogs/apart.rs`), as the real
//! app shows them, without a screen: the harness is told it may open
//! windows, and a stand-in for the window system draws each one in a pass of
//! its own, as eframe does, handing it the input a control client sent it.
//! The windows are driven through the control protocol's `ui.*` calls, the
//! words on screen, as a script or the AI client drives them.
//!
//! In a binary of its own: what the dialogs' windows share is the process's,
//! and the window tests beside it draw their dialogs in the main window.

#[path = "window/support.rs"]
#[allow(dead_code, unused_imports)]
mod support;

use egui::{ViewportCommand, ViewportId};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;
use support::*;

/// What the stand-in window system knows of each dialog window.
#[derive(Default)]
struct Window {
    /// Its title, as its builder gave it.
    title: String,
    /// The builder's last word on it.
    visible: Option<bool>,
    size: Option<egui::Vec2>,
    position: Option<egui::Pos2>,
    /// How many passes it was drawn in.
    passes: usize,
    /// The commands it sent the system, oldest first.
    commands: Vec<ViewportCommand>,
    /// Its close button pressed: given as the system gives it, next pass.
    close: bool,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<ViewportId, Window>> = RefCell::new(HashMap::new());
    /// The dialog window the system says has the keyboard.
    static FOCUSED: RefCell<Option<ViewportId>> = const { RefCell::new(None) };
}

/// The dialogs' windows share the process's record of them: one test at a
/// time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Where the main window stands on a 1440 by 900 screen.
fn main_window() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(100.0, 80.0), egui::vec2(1100.0, 720.0))
}

/// Draws each dialog window as eframe does, in a pass of its own inside the
/// main window's, with what a control client sent it.
fn stand_in_window_system() {
    egui::Context::set_immediate_viewport_renderer(|ctx, mut viewport| {
        let id = viewport.ids.this;
        let builder = viewport.builder.clone();
        let size = builder.inner_size.unwrap_or(egui::vec2(400.0, 300.0));
        let close = WINDOWS.with(|w| {
            let mut w = w.borrow_mut();
            let window = w.entry(id).or_default();
            if let Some(title) = &builder.title {
                window.title.clone_from(title);
            }
            window.visible = builder.visible.or(window.visible);
            window.size = Some(size);
            window.position = builder.position.or(window.position);
            window.passes += 1;
            std::mem::take(&mut window.close)
        });
        let at = WINDOWS.with(|w| w.borrow()[&id].position.unwrap_or(egui::Pos2::ZERO));
        let inner = egui::Rect::from_min_size(at, size);
        let mut raw = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        raw.viewports.insert(
            id,
            egui::ViewportInfo {
                parent: Some(viewport.ids.parent),
                title: builder.title.clone(),
                events: if close {
                    vec![egui::ViewportEvent::Close]
                } else {
                    Vec::new()
                },
                native_pixels_per_point: Some(1.0),
                monitor_size: Some(egui::vec2(1440.0, 900.0)),
                inner_rect: Some(inner),
                outer_rect: Some(inner.expand2(egui::vec2(0.0, 14.0))),
                focused: Some(FOCUSED.with(|f| *f.borrow() == Some(id))),
                ..Default::default()
            },
        );
        dm_desktop::dialogs::apart::feed(&mut raw);
        let mut out = ctx.run_ui(raw, |ui| (viewport.viewport_ui_cb)(ui));
        // Nothing here paints: what it would have uploaded is let go.
        out.textures_delta.clear();
    });
}

/// A window whose dialogs are windows of their own, with a control client
/// attached (its requests are made here, by `human_call`).
fn windowed(world: &World) -> Harness<'static, DmApp> {
    let mut app = world.app();
    let (_requests, inbox) = std::sync::mpsc::channel();
    app.attach_control(inbox);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(
            |ui, app: &mut DmApp| {
                app.ui(ui);
                let ctx = ui.ctx().clone();
                app.scan_screen(&ctx);
                app.painted();
            },
            app,
        );
    harness.ctx.set_embed_viewports(false);
    stand_in_window_system();
    WINDOWS.with(|w| w.borrow_mut().clear());
    FOCUSED.with(|f| *f.borrow_mut() = None);
    let root = harness
        .input_mut()
        .viewports
        .entry(ViewportId::ROOT)
        .or_default();
    root.inner_rect = Some(main_window());
    root.outer_rect = Some(main_window().expand2(egui::vec2(0.0, 28.0)));
    root.monitor_size = Some(egui::vec2(1440.0, 900.0));
    root.focused = Some(true);
    harness
}

/// One frame, with the step of what a control client asked that it is due.
fn frame(harness: &mut Harness<'_, DmApp>) {
    let ctx = harness.ctx.clone();
    let events = harness.state_mut().synthetic_events(&ctx);
    harness.input_mut().events.extend(events);
    harness.step();
    took_commands(harness);
}

/// The commands the dialogs' windows sent the system this frame: egui hands
/// them over with the main window's, once every window is drawn.
fn took_commands(harness: &Harness<'_, DmApp>) {
    WINDOWS.with(|w| {
        let mut w = w.borrow_mut();
        for (id, said) in &harness.output().viewport_output {
            if let Some(window) = w.get_mut(id) {
                window.commands.extend(said.commands.iter().cloned());
            }
        }
    });
}

fn frames(harness: &mut Harness<'_, DmApp>, n: usize) {
    for _ in 0..n {
        frame(harness);
    }
}

/// A `ui.*` call of the control protocol, as a client makes it.
fn call(
    harness: &mut Harness<'_, DmApp>,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .human_call(method, &params, Some(&ctx))
        .unwrap_or_else(|| panic!("{method} is not a ui call"))
        .unwrap_or_else(|e| panic!("{method}: {e}"))
}

/// Clicks the words on screen, as a client does, and lets the steps through.
fn click_words(harness: &mut Harness<'_, DmApp>, text: &str) {
    call(harness, "ui.click", json!({ "text": text }));
    frames(harness, 10);
}

fn press_key(harness: &mut Harness<'_, DmApp>, key: &str) {
    call(harness, "ui.key", json!({ "key": key }));
    frames(harness, 6);
}

/// The dialog window with this title, as the system knows it.
fn window_titled(title: &str) -> Option<ViewportId> {
    WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .find(|(_, window)| window.title == title)
            .map(|(id, _)| *id)
    })
}

fn with_window<R>(id: ViewportId, f: impl FnOnce(&mut Window) -> R) -> R {
    WINDOWS.with(|w| f(w.borrow_mut().get_mut(&id).expect("a dialog window")))
}

/// The words on screen, as `ui.texts` gives them, and the window of each.
fn texts(harness: &mut Harness<'_, DmApp>) -> Vec<(String, String)> {
    call(harness, "ui.texts", json!({}))["texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["text"].as_str().unwrap().to_string(),
                t["window"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// A dialog opens as a window of its own over the middle of the main window,
/// shown only once its size has settled, with its title in the system's
/// title bar; a control client sees its words as a dialog's and where it
/// stands; its close button closes it (B42).
#[test]
fn about_opens_in_a_window_of_its_own_over_the_main_window_and_closes_with_its_button() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = windowed(&world);
    harness.state_mut().dialogs.about_open = true;
    frame(&mut harness);
    let about = window_titled("About dm").expect("About opened no window");
    assert_eq!(
        with_window(about, |w| w.visible),
        Some(false),
        "shown before its size settled"
    );
    frames(&mut harness, 6);
    let (visible, size, position, passes) = with_window(about, |w| {
        (w.visible, w.size.unwrap(), w.position.unwrap(), w.passes)
    });
    assert_eq!(visible, Some(true));
    assert!(passes >= 6);
    // Over the middle of the main window, and wholly on the screen.
    let middle = position + size / 2.0;
    assert!(
        (middle.x - main_window().center().x).abs() < 2.0,
        "{position:?} {size:?}"
    );
    assert!(position.y >= main_window().top() && position.y + size.y < 900.0);
    // Its theme told to the system as it opens, not on every frame.
    let themes = with_window(about, |w| {
        w.commands
            .iter()
            .filter(|c| matches!(c, ViewportCommand::SetTheme(_)))
            .count()
    });
    assert!(
        themes >= 1 && themes < passes,
        "{themes} of {passes} passes"
    );

    // What a control client sees: its words, as a dialog's, and its window.
    let seen = texts(&mut harness);
    assert!(
        seen.iter().any(
            |(text, window)| text == "A free download manager, written in Rust."
                && window == "dialog"
        ),
        "{seen:?}"
    );
    assert!(seen.iter().any(|(_, window)| window == "main"));
    let windows = call(&mut harness, "ui.windows", json!({}));
    assert_eq!(windows["dialogs"][0]["title"], "About dm");
    assert_eq!(windows["main"][0], 100.0);
    assert!(windows["dialogs"][0]["inner"].is_array());
    // Moved by a client, as a person drags it: the system is asked to.
    let moved = call(
        &mut harness,
        "ui.move_window",
        json!({ "title": "about", "x": 40.0, "y": 60.0 }),
    );
    assert_eq!(moved["moved"], "About dm");
    harness.step();
    assert!(harness
        .output()
        .viewport_output
        .get(&about)
        .is_some_and(|o| o
            .commands
            .contains(&ViewportCommand::OuterPosition(egui::pos2(40.0, 60.0)))));
    let ctx = harness.ctx.clone();
    let nowhere = harness.state_mut().human_call(
        "ui.move_window",
        &json!({ "title": "nothing open", "x": 1.0, "y": 1.0 }),
        Some(&ctx),
    );
    assert!(matches!(nowhere, Some(Err(_))));

    // About holds the main window: a click on it brings About forward.
    harness.hover_at(egui::pos2(500.0, 400.0));
    harness.event(egui::Event::PointerButton {
        pos: egui::pos2(500.0, 400.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    assert!(harness
        .output()
        .viewport_output
        .get(&about)
        .is_some_and(|o| o.commands.contains(&ViewportCommand::Focus)));
    harness.event(egui::Event::PointerButton {
        pos: egui::pos2(500.0, 400.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    frame(&mut harness);

    // Its close button.
    with_window(about, |w| w.close = true);
    frames(&mut harness, 2);
    assert!(
        !harness.state().dialogs.about_open,
        "the close button left About open"
    );
    frames(&mut harness, 2);
    assert!(
        call(&mut harness, "ui.windows", json!({}))["dialogs"]
            .as_array()
            .unwrap()
            .is_empty(),
        "its window stayed"
    );
}

/// A client types into the dialog that has the keyboard and clicks its
/// buttons in its own window: Add URL, typed into and Cancelled; Escape in
/// a dialog's own window closes it.
#[test]
fn keys_and_clicks_reach_the_dialog_window_they_are_for() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = windowed(&world);
    frames(&mut harness, 2);
    harness.state_mut().dialogs.open_add_url(None);
    frames(&mut harness, 6);
    let add = window_titled("Add URL").expect("Add URL opened no window");
    // Its first field has the keyboard: what a client types goes there,
    // into the window the system says has the keyboard.
    FOCUSED.with(|f| *f.borrow_mut() = Some(add));
    call(
        &mut harness,
        "ui.type",
        json!({ "text": "http://example.invalid/a.zip" }),
    );
    frames(&mut harness, 4);
    assert_eq!(
        harness.state().dialogs.add_url.as_ref().unwrap().address,
        "http://example.invalid/a.zip"
    );
    // Asked for again, it is brought forward with what was typed in it.
    harness.state_mut().dialogs.open_add_url(None);
    frames(&mut harness, 2);
    assert!(with_window(add, |w| w
        .commands
        .contains(&ViewportCommand::Focus)));
    assert_eq!(
        harness.state().dialogs.add_url.as_ref().unwrap().address,
        "http://example.invalid/a.zip"
    );
    click_words(&mut harness, "Cancel");
    assert!(
        harness.state().dialogs.add_url.is_none(),
        "Cancel in its window did nothing"
    );

    // Escape in a question's own window answers it as Cancel.
    harness.state_mut().dialogs.confirm_exit = true;
    frames(&mut harness, 4);
    let exit = window_titled("Exit dm?").expect("the question opened no window");
    FOCUSED.with(|f| *f.borrow_mut() = Some(exit));
    press_key(&mut harness, "Escape");
    assert!(
        !harness.state().dialogs.confirm_exit,
        "Escape left the question open"
    );
    assert!(!harness.state().quit_asked);

    // With no dialog, keys go to the main window.
    frames(&mut harness, 3);
    press_key(&mut harness, "Escape");
    assert!(harness.state().dialogs.open_names().is_empty());
}

/// Options in its own window keeps its fixed size, closes with Escape,
/// follows the window's theme, and is hidden with the main window.
#[test]
fn options_in_its_own_window_keeps_its_size_and_theme() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = windowed(&world);
    frames(&mut harness, 2);
    let settings = harness.state().applied.clone();
    harness.state_mut().dialogs.open_options(&settings);
    frames(&mut harness, 8);
    let options = window_titled("Options").expect("Options opened no window");
    FOCUSED.with(|f| *f.borrow_mut() = Some(options));
    let first = with_window(options, |w| w.size.unwrap());
    frames(&mut harness, 3);
    assert_eq!(
        with_window(options, |w| w.size.unwrap()),
        first,
        "its size moved"
    );
    press_key(&mut harness, "Escape");
    assert!(
        harness.state().dialogs.options.is_none(),
        "Escape left Options open"
    );

    // The dark look picked in it: its window's title bar is told so, and
    // told again for the light one.
    harness.state_mut().dialogs.open_options(&settings);
    frames(&mut harness, 4);
    let options = window_titled("Options").unwrap();
    let pick = |harness: &mut Harness<'_, DmApp>, theme: &str| {
        harness
            .state_mut()
            .dialogs
            .options
            .as_mut()
            .unwrap()
            .draft
            .theme = theme.into();
        frames(harness, 4);
    };
    pick(&mut harness, "dark");
    pick(&mut harness, "light");
    let told: Vec<egui::SystemTheme> = with_window(options, |w| {
        w.commands
            .iter()
            .filter_map(|c| match c {
                ViewportCommand::SetTheme(theme) => Some(*theme),
                _ => None,
            })
            .collect()
    });
    let dark = told
        .iter()
        .position(|t| *t == egui::SystemTheme::Dark)
        .expect("the dark look was not told to the dialog's window");
    assert!(
        told[dark..].contains(&egui::SystemTheme::Light),
        "the light look was not told after it: {told:?}"
    );

    // Minimised with the main window, it is hidden with it.
    harness
        .input_mut()
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .minimized = Some(true);
    frames(&mut harness, 2);
    assert_eq!(with_window(options, |w| w.visible), Some(false));
    harness
        .input_mut()
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .minimized = Some(false);
    frames(&mut harness, 2);
    assert_eq!(with_window(options, |w| w.visible), Some(true));
    harness.state_mut().dialogs.options = None;
    frames(&mut harness, 2);
}

/// On a small screen a dialog taller than it is held to the screen and its
/// body scrolls; a window that opens at a corner (Download complete) or
/// with an anchor (a question) stands there, on the screen.
#[test]
fn a_dialog_stays_on_a_small_screen_and_opens_where_it_says() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = windowed(&world);
    let root = harness
        .input_mut()
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap();
    root.monitor_size = Some(egui::vec2(900.0, 420.0));
    root.inner_rect = Some(egui::Rect::from_min_size(
        egui::pos2(0.0, 30.0),
        egui::vec2(900.0, 380.0),
    ));
    frames(&mut harness, 2);
    let settings = harness.state().applied.clone();
    harness.state_mut().dialogs.open_options(&settings);
    frames(&mut harness, 10);
    let options = window_titled("Options").unwrap();
    let (size, position) = with_window(options, |w| (w.size.unwrap(), w.position.unwrap()));
    // No taller than the screen less room for its bars, nor wider.
    assert!(size.y <= 420.0 - 120.0 + 0.5, "{size:?}");
    assert!(size.x <= 900.0 - 40.0 + 0.5, "{size:?}");
    assert!(position.y >= 0.0);
    harness.state_mut().dialogs.options = None;
    frames(&mut harness, 2);

    // A question with an anchor, in the middle of the main window, on the
    // screen.
    add_named(&mut harness, &world, "kept.bin");
    harness.state_mut().refresh();
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.confirm_remove = Some(vec![id]);
    frames(&mut harness, 6);
    let remove = WINDOWS
        .with(|w| {
            w.borrow()
                .iter()
                .find(|(_, w)| w.title.starts_with("Remove"))
                .map(|(id, _)| *id)
        })
        .expect("the question opened no window");
    let (size, position) = with_window(remove, |w| (w.size.unwrap(), w.position.unwrap()));
    assert!(
        position.y >= 30.0 && position.y + size.y <= 420.0,
        "{position:?} {size:?}"
    );
    assert!(
        (position.x + size.x / 2.0 - 450.0).abs() < 2.0,
        "{position:?} {size:?}"
    );
    // Its close button answers it as Cancel: the row stays.
    with_window(remove, |w| w.close = true);
    frames(&mut harness, 3);
    assert!(harness.state().dialogs.confirm_remove.is_none());
    assert_eq!(harness.state().rows().len(), 1);
}

/// Properties of a download, and its progress window, open in windows of
/// their own placed where they say: Properties over the middle, one a step
/// on from the other; the main window takes no click while one is open.
#[test]
fn properties_open_in_windows_of_their_own() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = windowed(&world);
    add_named(&mut harness, &world, "first.bin");
    add_named(&mut harness, &world, "second.bin");
    harness.state_mut().refresh();
    frames(&mut harness, 2);
    let rows: Vec<dm_core::Entry> = harness
        .state()
        .rows()
        .iter()
        .map(|r| dm_core::Entry::clone(r))
        .collect();
    for row in &rows {
        harness.state_mut().dialogs.open_properties(row);
    }
    harness.state_mut().dialogs.progress.insert(rows[0].id);
    frames(&mut harness, 8);
    let titled: Vec<String> =
        WINDOWS.with(|w| w.borrow().values().map(|w| w.title.clone()).collect());
    assert_eq!(
        titled.iter().filter(|t| *t == "File properties").count(),
        2,
        "{titled:?}"
    );
    let places: Vec<egui::Pos2> = WINDOWS.with(|w| {
        w.borrow()
            .values()
            .filter(|w| w.title == "File properties")
            .filter_map(|w| w.position)
            .collect()
    });
    assert_eq!(places.len(), 2);
    assert_ne!(places[0], places[1], "both Properties at one place");
    // Each Properties' Cancel is its own.
    let windows = call(&mut harness, "ui.windows", json!({}));
    assert!(
        windows["dialogs"].as_array().unwrap().len() >= 3,
        "{windows}"
    );
    click_words(&mut harness, "Cancel");
    assert_eq!(harness.state().dialogs.properties.len(), 1);
    harness.state_mut().dialogs.properties.clear();
    harness.state_mut().dialogs.progress.clear();
    frames(&mut harness, 3);
}

/// Escape that closes a drop-down in a dialog's own window is the
/// drop-down's: the dialog stays open, with what was changed in it, and the
/// next Escape closes it (as in the main window, `dialogs::show`).
#[test]
fn escape_on_a_drop_down_in_a_dialogs_window_closes_only_the_drop_down() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut harness = windowed(&world);
    frames(&mut harness, 2);
    let settings = harness.state().applied.clone();
    harness.state_mut().dialogs.open_options(&settings);
    frames(&mut harness, 8);
    let options = window_titled("Options").expect("Options opened no window");
    FOCUSED.with(|f| *f.borrow_mut() = Some(options));
    // The language's drop-down: its languages are listed in the dialog's window.
    click_words(&mut harness, "English");
    assert!(
        texts(&mut harness)
            .iter()
            .any(|(text, window)| text == "Deutsch" && window == "dialog"),
        "the drop-down did not open"
    );
    press_key(&mut harness, "Escape");
    assert!(
        harness.state().dialogs.options.is_some(),
        "the Escape that closed the drop-down closed Options"
    );
    press_key(&mut harness, "Escape");
    assert!(harness.state().dialogs.options.is_none());
}
