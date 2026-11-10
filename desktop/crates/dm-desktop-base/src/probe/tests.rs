// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The JSON each method answers, written out as `scripts/human-qa.py` and
//! the MCP server read it: a window that answers otherwise breaks them.

use super::*;
use serde_json::json;

/// A window that answers with fixed values and keeps what it was asked.
#[derive(Default)]
struct Stand {
    asked: Vec<String>,
    refuse_steps: bool,
    headless: bool,
}

impl WindowProbe for Stand {
    fn may_step(&mut self) -> Result<(), String> {
        if self.refuse_steps {
            Err("dm's window is hidden in the tray".into())
        } else {
            Ok(())
        }
    }
    fn has_window(&self) -> bool {
        !self.headless
    }
    fn texts(&mut self) -> Result<Texts, String> {
        Ok(Texts::new(vec![
            Text {
                text: "OK".into(),
                rect: [10.0, 20.0, 40.0, 36.0],
                window: Window::Dialog,
            },
            Text {
                text: "Add URL".into(),
                rect: [1.0, 2.0, 3.0, 4.0],
                window: Window::Main,
            },
        ]))
    }
    fn click(&mut self, click: Click) -> Result<Clicked, String> {
        self.asked.push(format!("{click:?}"));
        Ok(Clicked {
            clicked: click.text,
            at: [25.0, 28.0],
        })
    }
    fn scroll(&mut self, scroll: Scroll) -> Result<Scrolled, String> {
        Ok(Scrolled {
            scrolled: scroll.text,
            lines: scroll.lines,
        })
    }
    fn type_text(&mut self, text: &str) -> Result<usize, String> {
        Ok(text.chars().count())
    }
    fn key(&mut self, key: KeyPress) -> Result<(), String> {
        self.asked.push(format!("{key:?}"));
        Ok(())
    }
    fn accessibility(&mut self) -> Result<Value, String> {
        Ok(json!({ "windows": [] }))
    }
    fn windows(&mut self) -> Result<Windows, String> {
        Ok(Windows {
            pixels_per_point: Some(2.0),
            main: Some([0.0, 25.0, 900.0, 625.0]),
            dialogs: vec![DialogWindow {
                title: "Options".into(),
                outer: Some([100.0, 100.0, 500.0, 400.0]),
                inner: None,
            }],
        })
    }
    fn window_to(&mut self, to: WindowTo) -> Result<(), String> {
        self.asked.push(to.word().into());
        Ok(())
    }
    fn move_window(&mut self, to: MoveWindow) -> Result<Moved, String> {
        Ok(Moved {
            moved: format!("found {}", to.title),
            to: to.to,
        })
    }
    fn select(&mut self, ids: Vec<u64>) -> Result<Vec<u64>, String> {
        Ok(ids.into_iter().filter(|id| *id < 10).collect())
    }
    fn place(&mut self, place: &str) -> Result<(), String> {
        if place == "all" {
            Ok(())
        } else {
            Err(format!("no place is called {place}"))
        }
    }
    fn inspect(&mut self) -> Result<Inspect, String> {
        Ok(Inspect {
            place: "All".into(),
            selected: vec![3],
            sort: Some(Sort {
                column: "Size".into(),
                ascending: false,
            }),
            categories: true,
            toolbar: true,
            dark: false,
            hidden: false,
            said: String::new(),
            problems: vec!["a problem".into()],
            dialogs: vec!["Options".into()],
            rows: vec![InspectRow {
                id: 3,
                name: "a.zip".into(),
                status: "Complete".into(),
                queue: "Main queue".into(),
                description: String::new(),
                url: "http://e.com/a.zip".into(),
                rect: None,
            }],
        })
    }
    fn commands(&mut self) -> Vec<CommandState> {
        vec![CommandState {
            id: "add_url",
            label: "Add URL",
            enabled: true,
        }]
    }
    fn run_command(&mut self, id: &str) -> Result<Ran, String> {
        Ok(Ran {
            ran: id.into(),
            said: "done".into(),
        })
    }
}

fn input_ok(probe: &mut Stand, method: &str, params: Value) -> Value {
    input(probe, method, &params).unwrap().unwrap()
}

fn input_err(probe: &mut Stand, method: &str, params: Value) -> String {
    input(probe, method, &params).unwrap().unwrap_err()
}

#[test]
fn the_words_on_screen_are_given_with_their_place_and_window() {
    assert_eq!(
        input_ok(&mut Stand::default(), "ui.texts", json!({})),
        json!({
            "texts": [
                { "text": "OK", "rect": [10.0, 20.0, 40.0, 36.0], "window": "dialog" },
                { "text": "Add URL", "rect": [1.0, 2.0, 3.0, 4.0], "window": "main" },
            ],
            "note": "nearest the front first",
        })
    );
}

#[test]
fn a_click_is_read_from_its_parameters_and_answered_with_where_it_landed() {
    let mut stand = Stand::default();
    assert_eq!(
        input_ok(
            &mut stand,
            "ui.click",
            json!({ "text": "Add URL", "nth": 1, "button": "right", "double": true })
        ),
        json!({ "clicked": "Add URL", "at": [25.0, 28.0] })
    );
    input_ok(&mut stand, "ui.click", json!({ "text": "OK" }));
    assert_eq!(
        stand.asked,
        [
            format!(
                "{:?}",
                Click {
                    text: "Add URL".into(),
                    nth: 1,
                    button: Button::Secondary,
                    double: true
                }
            ),
            format!(
                "{:?}",
                Click {
                    text: "OK".into(),
                    nth: 0,
                    button: Button::Primary,
                    double: false
                }
            ),
        ]
    );
    assert_eq!(
        input_err(&mut stand, "ui.click", json!({ "text": "" })),
        "give `text`: the words on the button, tab or menu to click"
    );
}

#[test]
fn a_click_on_what_runs_a_file_is_refused_before_the_window_is_asked() {
    let mut stand = Stand::default();
    for label in ["Open", "open with…", "Preview"] {
        let why = input_err(&mut stand, "ui.click", json!({ "text": label }));
        assert!(why.contains("runs a downloaded file"), "{why}");
    }
    assert!(stand.asked.is_empty());
    assert!(!is_run_a_file("Open folder"));
}

#[test]
fn scroll_type_and_key_answer_as_they_always_have() {
    let mut stand = Stand::default();
    assert_eq!(
        input_ok(&mut stand, "ui.scroll", json!({ "text": "a.zip" })),
        json!({ "scrolled": "a.zip", "lines": 3.0 })
    );
    assert_eq!(
        input_ok(
            &mut stand,
            "ui.scroll",
            json!({ "text": "a.zip", "lines": -2 })
        ),
        json!({ "scrolled": "a.zip", "lines": -2.0 })
    );
    assert_eq!(
        input_ok(&mut stand, "ui.type", json!({ "text": "héllo" })),
        json!({ "typed": 5 })
    );
    assert_eq!(
        input_ok(
            &mut stand,
            "ui.key",
            json!({ "key": "A", "modifiers": ["Ctrl", "shift"] })
        ),
        json!({ "pressed": "A" })
    );
    assert_eq!(
        stand.asked.last().unwrap(),
        &format!(
            "{:?}",
            KeyPress {
                key: "A".into(),
                modifiers: Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::default()
                }
            }
        )
    );
    assert_eq!(
        input_err(&mut stand, "ui.scroll", json!({})),
        "give `text`: words on screen to hold the pointer over, in what scrolls"
    );
    assert_eq!(
        input_err(&mut stand, "ui.type", json!({})),
        "give `text`: what to type into the field that has focus"
    );
    assert_eq!(
        input_err(&mut stand, "ui.key", json!({})),
        "give `key`: Escape, Enter, Tab, Delete, ArrowDown, A…"
    );
}

#[test]
fn a_step_the_window_cannot_take_is_refused_before_its_parameters_are_read() {
    let mut stand = Stand {
        refuse_steps: true,
        ..Stand::default()
    };
    for method in ["ui.click", "ui.type", "ui.key", "ui.scroll"] {
        assert_eq!(
            input_err(&mut stand, method, json!({})),
            "dm's window is hidden in the tray",
            "{method}"
        );
    }
    // What only reads is not a step.
    input_ok(&mut stand, "ui.texts", json!({}));
}

#[test]
fn the_windows_are_said_where_they_are_and_moved_and_put_away() {
    let mut stand = Stand::default();
    assert_eq!(
        input_ok(&mut stand, "ui.windows", json!({})),
        json!({
            "pixels_per_point": 2.0,
            "main": [0.0, 25.0, 900.0, 625.0],
            "dialogs": [{ "title": "Options", "outer": [100.0, 100.0, 500.0, 400.0], "inner": null }],
        })
    );
    assert_eq!(
        input_ok(&mut stand, "ui.window", json!({ "to": "tray" })),
        json!({ "to": "tray" })
    );
    assert_eq!(
        input_err(&mut stand, "ui.window", json!({ "to": "away" })),
        "give `to`: tray, minimized or shown"
    );
    assert_eq!(
        input_ok(
            &mut stand,
            "ui.move_window",
            json!({ "title": "OPTIONS", "x": 10, "y": 20.5 })
        ),
        json!({ "moved": "found options", "to": [10.0, 20.5] })
    );
    assert_eq!(
        input_err(&mut stand, "ui.move_window", json!({ "title": "Options" })),
        "give `x` and `y`: where its top-left corner goes, in screen points"
    );
    let mut headless = Stand {
        headless: true,
        ..Stand::default()
    };
    assert_eq!(
        input_err(&mut headless, "ui.window", json!({ "to": "away" })),
        "there is no window",
        "no window is said before a bad `to`"
    );
    assert_eq!(
        input_err(&mut headless, "ui.move_window", json!({})),
        "there is no window to move"
    );
}

#[test]
fn the_view_is_inspected_chosen_and_placed() {
    let mut stand = Stand::default();
    let inspect = view(&mut stand, "ui.inspect", &json!({})).unwrap().unwrap();
    assert_eq!(
        inspect,
        json!({
            "place": "All",
            "selected": [3],
            "sort": { "column": "Size", "ascending": false },
            "categories": true,
            "toolbar": true,
            "dark": false,
            "hidden": false,
            "said": "",
            "problems": ["a problem"],
            "dialogs": ["Options"],
            "rows": [{
                "id": 3,
                "name": "a.zip",
                "status": "Complete",
                "queue": "Main queue",
                "description": "",
                "url": "http://e.com/a.zip",
                "rect": null,
            }],
        })
    );
    assert_eq!(
        view(&mut stand, "ui.select", &json!({ "ids": [3, 12, "x"] }))
            .unwrap()
            .unwrap(),
        json!({ "selected": [3] })
    );
    assert_eq!(
        view(&mut stand, "ui.place", &json!({ "place": "all" }))
            .unwrap()
            .unwrap(),
        json!({ "place": "all" })
    );
    assert_eq!(
        view(&mut stand, "ui.place", &json!({}))
            .unwrap()
            .unwrap_err(),
        "no place is called "
    );
}

#[test]
fn the_commands_are_listed_and_run() {
    let mut stand = Stand::default();
    assert_eq!(
        view(&mut stand, "commands.list", &json!({}))
            .unwrap()
            .unwrap(),
        json!([{ "id": "add_url", "label": "Add URL", "enabled": true }])
    );
    assert_eq!(
        view(&mut stand, "command.run", &json!({ "id": "add_url" }))
            .unwrap()
            .unwrap(),
        json!({ "ran": "add_url", "said": "done" })
    );
}

#[test]
fn each_door_answers_its_own_methods_only() {
    let mut stand = Stand::default();
    for method in INPUT_METHODS {
        assert!(view(&mut stand, method, &json!({})).is_none(), "{method}");
    }
    for method in VIEW_METHODS {
        assert!(input(&mut stand, method, &json!({})).is_none(), "{method}");
    }
    assert!(input(&mut stand, "queue.list", &json!({})).is_none());
    assert!(view(&mut stand, "queue.list", &json!({})).is_none());
}
