// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The control port's window methods (`ui.*`, `commands.list`,
//! `command.run`) as a contract any window can answer: what each is asked,
//! as typed requests read from its parameters, and what each answers, as
//! types whose JSON the tests below fix (from what `scripts/human-qa.py` and
//! the MCP server read). A window implements [`WindowProbe`]; [`input`] and
//! [`view`] read a request, call it, and write its answer.
//!
//! Two doors, as the port has always had them: [`input`] for what acts as a
//! person's hands and eyes (clicks, keys, typing, scrolling, the words on
//! screen, the windows), answered only directly; [`view`] for what reads or
//! sets the window's view and runs its commands, which a `batch` reaches too.

use serde::Serialize;
use serde_json::Value;

/// A rectangle on the screen, in points: left, top, right, bottom.
pub type Rect = [f32; 4];

/// Which window a piece of text is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Window {
    /// dm's main window.
    Main,
    /// One of its dialogs' own windows.
    Dialog,
}

/// One piece of text drawn on screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Text {
    /// The words.
    pub text: String,
    /// Where they are drawn.
    pub rect: Rect,
    /// In which window.
    pub window: Window,
}

/// `ui.texts`: every piece of text on screen, nearest the front first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Texts {
    /// The pieces, nearest the front first.
    pub texts: Vec<Text>,
    /// Says the order.
    pub note: &'static str,
}

impl Texts {
    /// The texts, the note saying their order.
    pub fn new(texts: Vec<Text>) -> Texts {
        Texts {
            texts,
            note: "nearest the front first",
        }
    }
}

/// Which pointer button a click is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The usual one.
    Primary,
    /// The one that opens a menu.
    Secondary,
}

/// `ui.click {text, nth?, button?, double?}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click {
    /// The words on what to click.
    pub text: String,
    /// Which of several that say them, from 0.
    pub nth: usize,
    /// Which button.
    pub button: Button,
    /// A double click.
    pub double: bool,
}

/// What `ui.click` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Clicked {
    /// The words clicked, as drawn.
    pub clicked: String,
    /// Where, in points.
    pub at: [f32; 2],
}

/// `ui.scroll {text, lines?}`: lines turned down (negative: up) over the
/// words, as a wheel is.
#[derive(Debug, Clone, PartialEq)]
pub struct Scroll {
    /// Words on screen to hold the pointer over.
    pub text: String,
    /// Lines, 3 unless given.
    pub lines: f32,
}

/// What `ui.scroll` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scrolled {
    /// The words scrolled over.
    pub scrolled: String,
    /// The lines.
    pub lines: f32,
}

/// The modifier keys held with a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Alt (Option).
    pub alt: bool,
    /// Ctrl.
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// Cmd.
    pub cmd: bool,
}

/// `ui.key {key, modifiers?}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    /// The key's name: Escape, Enter, Tab, Delete, ArrowDown, A…
    pub key: String,
    /// What is held with it.
    pub modifiers: Modifiers,
}

/// Where `ui.window` puts the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowTo {
    /// Into the tray, as the close button's "tray" does.
    Tray,
    /// Minimized.
    Minimized,
    /// Shown and in front.
    Shown,
}

impl WindowTo {
    /// Its word in the protocol.
    pub fn word(self) -> &'static str {
        match self {
            WindowTo::Tray => "tray",
            WindowTo::Minimized => "minimized",
            WindowTo::Shown => "shown",
        }
    }
}

/// A dialog's own window, for `ui.windows`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DialogWindow {
    /// Its title.
    pub title: String,
    /// Its frame on the screen, where known.
    pub outer: Option<Rect>,
    /// Its inside on the screen, where known.
    pub inner: Option<Rect>,
}

/// What `ui.windows` answers: where dm's windows are.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Windows {
    /// Points on the screen are this many pixels each.
    pub pixels_per_point: Option<f32>,
    /// The main window, where known.
    pub main: Option<Rect>,
    /// Each dialog's own window.
    pub dialogs: Vec<DialogWindow>,
}

/// `ui.move_window {title, x, y}`.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveWindow {
    /// The dialog's title, or part of it, lowercased.
    pub title: String,
    /// Where its top-left corner goes, in screen points.
    pub to: [f32; 2],
}

/// What `ui.move_window` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Moved {
    /// The title of the dialog moved.
    pub moved: String,
    /// Where it went.
    pub to: [f32; 2],
}

/// The sort `ui.inspect` reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sort {
    /// The column's English name.
    pub column: String,
    /// Up or down.
    pub ascending: bool,
}

/// One row as `ui.inspect` reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InspectRow {
    /// The row.
    pub id: u64,
    /// The name the window shows it by, the one it will be saved as.
    pub name: String,
    /// Its status, in English.
    pub status: String,
    /// Its queue.
    pub queue: String,
    /// Its description.
    pub description: String,
    /// Its address as shown (no login).
    pub url: String,
    /// Where its line is drawn, where it is.
    pub rect: Option<Rect>,
}

/// What `ui.inspect` answers: the window's view.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Inspect {
    /// The place shown, as the window names it.
    pub place: String,
    /// The rows chosen.
    pub selected: Vec<u64>,
    /// The sort, if any.
    pub sort: Option<Sort>,
    /// Whether the categories are shown.
    pub categories: bool,
    /// Whether the toolbar is shown.
    pub toolbar: bool,
    /// Whether it is drawn dark.
    pub dark: bool,
    /// Whether it is parked in the tray and not drawing.
    pub hidden: bool,
    /// What the status line says, in English.
    pub said: String,
    /// The problems listed.
    pub problems: Vec<String>,
    /// The dialogs open, by name.
    pub dialogs: Vec<String>,
    /// The rows listed, in order.
    pub rows: Vec<InspectRow>,
}

/// One command as `commands.list` gives it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandState {
    /// Its stable name.
    pub id: &'static str,
    /// Its English label.
    pub label: &'static str,
    /// Whether it applies now.
    pub enabled: bool,
}

/// What `command.run` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ran {
    /// The command run.
    pub ran: String,
    /// What the status line says after, in English.
    pub said: String,
}

/// What a window answers over the control port. Each method gets its
/// request already read; an error is said to the client as it is.
pub trait WindowProbe {
    /// Whether a step (a click, a key, typing, a scroll) may be queued now;
    /// asked before each one's parameters are read. `Err` says why not.
    fn may_step(&mut self) -> Result<(), String>;
    /// Whether there is a window (not headless).
    fn has_window(&self) -> bool;
    /// `ui.texts`.
    fn texts(&mut self) -> Result<Texts, String>;
    /// `ui.click`, after [`is_run_a_file`] refused what runs a file.
    fn click(&mut self, click: Click) -> Result<Clicked, String>;
    /// `ui.scroll`.
    fn scroll(&mut self, scroll: Scroll) -> Result<Scrolled, String>;
    /// `ui.type`: what to type into the field that has focus. How many
    /// characters were typed.
    fn type_text(&mut self, text: &str) -> Result<usize, String>;
    /// `ui.key`.
    fn key(&mut self, key: KeyPress) -> Result<(), String>;
    /// `ui.accessibility`: what a screen reader is given of each window.
    fn accessibility(&mut self) -> Result<Value, String>;
    /// `ui.windows`.
    fn windows(&mut self) -> Result<Windows, String>;
    /// `ui.window`, once [`Self::has_window`].
    fn window_to(&mut self, to: WindowTo) -> Result<(), String>;
    /// `ui.move_window`, once [`Self::has_window`].
    fn move_window(&mut self, to: MoveWindow) -> Result<Moved, String>;
    /// `ui.select {ids}`: the rows chosen of those asked for (unknown ones
    /// left out).
    fn select(&mut self, ids: Vec<u64>) -> Result<Vec<u64>, String>;
    /// `ui.place {place}`: `all`, `unfinished`, `finished`, `queue:<name>`,
    /// `custom:<name>` or `kind:<word>`.
    fn place(&mut self, place: &str) -> Result<(), String>;
    /// `ui.inspect`.
    fn inspect(&mut self) -> Result<Inspect, String>;
    /// `commands.list`.
    fn commands(&mut self) -> Vec<CommandState>;
    /// `command.run {id}`.
    fn run_command(&mut self, id: &str) -> Result<Ran, String>;
}

/// The methods [`input`] answers.
pub const INPUT_METHODS: [&str; 9] = [
    "ui.texts",
    "ui.click",
    "ui.scroll",
    "ui.type",
    "ui.key",
    "ui.accessibility",
    "ui.windows",
    "ui.window",
    "ui.move_window",
];

/// The methods [`view`] answers.
pub const VIEW_METHODS: [&str; 5] = [
    "ui.select",
    "ui.place",
    "ui.inspect",
    "commands.list",
    "command.run",
];

/// What `ui.screenshot` is refused with when there is no window.
pub const NO_WINDOW_TO_PICTURE: &str = "dm is running headless: there is no window to picture";

/// Whether `text` is the label of what runs a downloaded file (Open, Open
/// with…, Preview, in English or the language in force): done in the
/// window by a person, never by a click over the port.
pub fn is_run_a_file(text: &str) -> bool {
    use dm_shell::app::commands::Command;
    let text = text.trim().to_lowercase();
    [Command::OpenFile, Command::OpenWith, Command::Preview]
        .iter()
        .flat_map(|c| [c.label(), crate::i18n::t(c.label())])
        .chain(
            ["Open", "Open with…", "Preview"]
                .into_iter()
                .flat_map(|e| [e, crate::i18n::t(e)]),
        )
        .any(|label| {
            label.trim().trim_end_matches('…').to_lowercase() == text.trim_end_matches('…')
        })
}

fn json<T: Serialize>(answer: Result<T, String>) -> Result<Value, String> {
    answer.and_then(|a| serde_json::to_value(a).map_err(|e| e.to_string()))
}

fn text_param<'a>(params: &'a Value, missing: &str) -> Result<&'a str, String> {
    params
        .get("text")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| missing.to_string())
}

/// Answers one of [`INPUT_METHODS`]; `None` for any other method.
pub fn input(
    probe: &mut dyn WindowProbe,
    method: &str,
    params: &Value,
) -> Option<Result<Value, String>> {
    if !INPUT_METHODS.contains(&method) {
        return None;
    }
    // Steps wait for frames: whether one may wait is the window's to say,
    // before anything is read.
    if matches!(method, "ui.click" | "ui.type" | "ui.key" | "ui.scroll") {
        if let Err(why) = probe.may_step() {
            return Some(Err(why));
        }
    }
    Some(match method {
        "ui.texts" => json(probe.texts()),
        "ui.click" => (|| {
            let text = text_param(
                params,
                "give `text`: the words on the button, tab or menu to click",
            )?;
            if is_run_a_file(text) {
                return Err(format!(
                    "{text} runs a downloaded file: it is done in the window by a person, not over this connection"
                ));
            }
            let click = Click {
                text: text.to_string(),
                nth: params.get("nth").and_then(Value::as_u64).unwrap_or(0) as usize,
                button: match params.get("button").and_then(Value::as_str) {
                    Some("right") | Some("secondary") => Button::Secondary,
                    _ => Button::Primary,
                },
                double: params
                    .get("double")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            json(probe.click(click))
        })(),
        "ui.scroll" => (|| {
            let text = text_param(
                params,
                "give `text`: words on screen to hold the pointer over, in what scrolls",
            )?;
            let lines = params.get("lines").and_then(Value::as_f64).unwrap_or(3.0) as f32;
            json(probe.scroll(Scroll {
                text: text.to_string(),
                lines,
            }))
        })(),
        "ui.type" => (|| {
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .ok_or("give `text`: what to type into the field that has focus")?;
            let typed = probe.type_text(text)?;
            Ok(serde_json::json!({ "typed": typed }))
        })(),
        "ui.key" => (|| {
            let name = params
                .get("key")
                .and_then(Value::as_str)
                .ok_or("give `key`: Escape, Enter, Tab, Delete, ArrowDown, A…")?;
            let has = |m: &str| {
                params
                    .get("modifiers")
                    .and_then(Value::as_array)
                    .is_some_and(|a| {
                        a.iter()
                            .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(m)))
                    })
            };
            probe.key(KeyPress {
                key: name.to_string(),
                modifiers: Modifiers {
                    alt: has("alt"),
                    ctrl: has("ctrl"),
                    shift: has("shift"),
                    cmd: has("cmd"),
                },
            })?;
            Ok(serde_json::json!({ "pressed": name }))
        })(),
        "ui.accessibility" => probe.accessibility(),
        "ui.windows" => json(probe.windows()),
        "ui.window" => (|| {
            if !probe.has_window() {
                return Err("there is no window".to_string());
            }
            let to = match params.get("to").and_then(Value::as_str) {
                Some("tray") => WindowTo::Tray,
                Some("minimized") => WindowTo::Minimized,
                Some("shown") => WindowTo::Shown,
                _ => return Err("give `to`: tray, minimized or shown".into()),
            };
            probe.window_to(to)?;
            Ok(serde_json::json!({ "to": to.word() }))
        })(),
        "ui.move_window" => (|| {
            if !probe.has_window() {
                return Err("there is no window to move".to_string());
            }
            let title = params
                .get("title")
                .and_then(Value::as_str)
                .ok_or("give `title`: the dialog's title, or part of it")?
                .to_lowercase();
            let at = |key: &str| {
                params
                    .get(key)
                    .and_then(Value::as_f64)
                    .ok_or("give `x` and `y`: where its top-left corner goes, in screen points")
            };
            let to = [at("x")? as f32, at("y")? as f32];
            json(probe.move_window(MoveWindow { title, to }))
        })(),
        _ => return None,
    })
}

/// Answers one of [`VIEW_METHODS`]; `None` for any other method.
pub fn view(
    probe: &mut dyn WindowProbe,
    method: &str,
    params: &Value,
) -> Option<Result<Value, String>> {
    Some(match method {
        "commands.list" => json::<Vec<CommandState>>(Ok(probe.commands())),
        "command.run" => {
            let id = params.get("id").and_then(Value::as_str).unwrap_or("");
            json(probe.run_command(id))
        }
        "ui.select" => {
            let ids: Vec<u64> = params
                .get("ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default();
            probe
                .select(ids)
                .map(|selected| serde_json::json!({ "selected": selected }))
        }
        "ui.place" => {
            let place = params.get("place").and_then(Value::as_str).unwrap_or("");
            probe
                .place(place)
                .map(|()| serde_json::json!({ "place": place }))
        }
        "ui.inspect" => json(probe.inspect()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
