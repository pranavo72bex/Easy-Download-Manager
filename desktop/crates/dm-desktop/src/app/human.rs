//! Driving the window as a person does: see what is drawn, click a word,
//! type, press a key. Behind `ui.texts`, `ui.click`, `ui.type` and `ui.key`
//! of the control protocol, and so behind the AI client's tools.
//!
//! What is on screen is read from the frame's own text shapes, back to
//! front, so a click lands on the word a person would see on top. Clicks,
//! typing and keys are fed to egui as the pointer and keyboard would, one
//! step a frame, so a button needs the hover, press and release a real one
//! does.

use super::*;
use serde_json::{json, Value};

/// How many steps may wait for frames before more are refused.
pub const MAX_WAITING_STEPS: usize = 64;

/// How long a step may wait for a frame before it is dropped.
const STEP_EXPIRES: Duration = Duration::from_secs(5);

/// One piece of text drawn this frame.
#[derive(Clone, Debug)]
pub struct ScreenText {
    pub text: String,
    pub rect: egui::Rect,
}

/// Words a script or an AI client may not click: they run a downloaded file,
/// which on Windows may be a program, and that stays a person's click.
fn is_run_a_file(text: &str) -> bool {
    use crate::commands::Command;
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

impl DmApp {
    /// Whether a control client acted a moment ago. What runs a downloaded
    /// file, and the command dm runs when a download finishes, are refused
    /// then: words on a button are no proof of what it does (a double-click
    /// on a row opens it, once Options says so), so the refusal is made
    /// where the file would be run, not only where the click is aimed.
    pub(super) fn by_script(&self) -> bool {
        self.scripted_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
    }

    /// Reads the words drawn this frame. Run at the end of the frame, and
    /// only when a control client may ask.
    pub fn scan_screen(&mut self, ctx: &egui::Context) {
        if self.control.is_none() {
            return;
        }
        let mut layers = vec![egui::LayerId::background()];
        layers.extend(ctx.memory(|m| m.layer_ids().collect::<Vec<_>>()));
        let mut seen = Vec::new();
        ctx.graphics(|graphics| {
            for layer in &layers {
                let Some(list) = graphics.get(*layer) else {
                    continue;
                };
                for clipped in list.all_entries() {
                    collect(&clipped.shape, clipped.clip_rect, &mut seen);
                }
            }
        });
        self.screen = seen;
    }

    /// Events for the frame about to run: one step of what was asked.
    pub fn synthetic_events(&mut self, ctx: &egui::Context) -> Vec<egui::Event> {
        // Steps that waited for frames that never came (a minimized window
        // draws none) are dropped, not replayed onto whatever is on screen
        // when it comes back.
        if self
            .synthetic
            .front()
            .is_some_and(|(at, _)| at.elapsed() > STEP_EXPIRES)
        {
            self.synthetic.clear();
        }
        let events = self
            .synthetic
            .pop_front()
            .map(|(_, e)| e)
            .unwrap_or_default();
        // The moment an event is given to the window is the script's, not
        // the moment it was asked for.
        if !events.is_empty() {
            self.scripted_at = Some(Instant::now());
        }
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
        events
    }

    /// Answers the `ui.*` requests that act like a person.
    pub fn human_call(
        &mut self,
        method: &str,
        params: &Value,
        ctx: Option<&egui::Context>,
    ) -> Option<Result<Value, String>> {
        // Steps wait for frames: a window that draws none (parked in the
        // tray) would let a client queue them without end.
        if matches!(method, "ui.click" | "ui.type" | "ui.key") {
            if self.hidden {
                return Some(Err(
                    "dm's window is hidden in the tray: launch dm again, or choose Show in the tray, first"
                        .into(),
                ));
            }
            if self.synthetic.len() > MAX_WAITING_STEPS {
                return Some(Err(
                    "too many steps are waiting: let the window catch up".into()
                ));
            }
            // Only what was taken counts as a script's moment.
            self.scripted_at = Some(Instant::now());
        }
        let wake = || {
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        };
        Some(match method {
            "ui.texts" => {
                let texts: Vec<Value> = self
                    .screen
                    .iter()
                    .rev()
                    .map(|t| {
                        json!({ "text": t.text, "rect": [t.rect.min.x, t.rect.min.y, t.rect.max.x, t.rect.max.y] })
                    })
                    .collect();
                Ok(json!({ "texts": texts, "note": "nearest the front first" }))
            }
            "ui.click" => (|| {
                let wanted = params
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                    .ok_or("give `text`: the words on the button, tab or menu to click")?;
                if is_run_a_file(wanted) {
                    return Err(format!(
                        "{wanted} runs a downloaded file: it is done in the window by a person, not over this connection"
                    ));
                }
                let nth = params.get("nth").and_then(Value::as_u64).unwrap_or(0) as usize;
                let lower = wanted.to_lowercase();
                let matching = |exact: bool| -> Vec<&ScreenText> {
                    self.screen
                        .iter()
                        .rev()
                        .filter(|t| {
                            let have = t.text.trim().to_lowercase();
                            if exact {
                                have == lower
                            } else {
                                have.contains(&lower)
                            }
                        })
                        .collect()
                };
                let mut found = matching(true);
                if found.is_empty() {
                    found = matching(false);
                }
                let target = found.get(nth).copied().ok_or_else(|| {
                    let near: Vec<&str> = self
                        .screen
                        .iter()
                        .rev()
                        .map(|t| t.text.as_str())
                        .take(40)
                        .collect();
                    format!("nothing on screen says {wanted:?}; some of what does: {near:?}")
                })?;
                if is_run_a_file(&target.text) {
                    return Err(format!("{} is done by a person", target.text));
                }
                let pos = target.rect.center();
                let button = match params.get("button").and_then(Value::as_str) {
                    Some("right") | Some("secondary") => egui::PointerButton::Secondary,
                    _ => egui::PointerButton::Primary,
                };
                let double = params
                    .get("double")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let m = egui::Modifiers::NONE;
                step(&mut self.synthetic, vec![egui::Event::PointerMoved(pos)]);
                step(&mut self.synthetic, Vec::new());
                // The press and the release in one frame: egui takes a
                // release more than 0.8 s after its press for a hold, not a
                // click, and a frame can take longer than that on a busy
                // computer (the QA's first click after a full test run
                // highlighted Add URL and opened nothing). A double-click
                // is both pairs in the frame, for the same reason.
                let mut presses = Vec::new();
                for _ in 0..if double { 2 } else { 1 } {
                    for pressed in [true, false] {
                        presses.push(egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            modifiers: m,
                        });
                    }
                }
                step(&mut self.synthetic, presses);
                step(&mut self.synthetic, Vec::new());
                wake();
                Ok(json!({ "clicked": target.text, "at": [pos.x, pos.y] }))
            })(),
            "ui.type" => (|| {
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("give `text`: what to type into the field that has focus")?;
                step(
                    &mut self.synthetic,
                    vec![egui::Event::Text(text.to_string())],
                );
                wake();
                Ok(json!({ "typed": text.chars().count() }))
            })(),
            "ui.key" => (|| {
                let name = params
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or("give `key`: Escape, Enter, Tab, Delete, ArrowDown, A…")?;
                let key =
                    egui::Key::from_name(name).ok_or_else(|| format!("no key is called {name}"))?;
                let has = |m: &str| {
                    params
                        .get("modifiers")
                        .and_then(Value::as_array)
                        .is_some_and(|a| {
                            a.iter()
                                .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(m)))
                        })
                };
                let modifiers = egui::Modifiers {
                    alt: has("alt"),
                    ctrl: has("ctrl"),
                    shift: has("shift"),
                    mac_cmd: has("cmd"),
                    command: has("ctrl") && !cfg!(target_os = "macos") || has("cmd"),
                };
                let event = |pressed| egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers,
                };
                step(&mut self.synthetic, vec![event(true)]);
                step(&mut self.synthetic, vec![event(false)]);
                wake();
                Ok(json!({ "pressed": name }))
            })(),
            _ => return None,
        })
    }
}

/// One step for a frame, stamped with when it was asked for.
fn step(
    queue: &mut std::collections::VecDeque<(Instant, Vec<egui::Event>)>,
    events: Vec<egui::Event>,
) {
    queue.push_back((Instant::now(), events));
}

fn collect(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<ScreenText>) {
    match shape {
        egui::Shape::Vec(inner) => inner.iter().for_each(|s| collect(s, clip, out)),
        egui::Shape::Text(text) => {
            for row in &text.galley.rows {
                let words = row.text();
                let rect = row.rect().translate(text.pos.to_vec2() + row.pos.to_vec2());
                // Only what a person could see, and that is not blank.
                if words.trim().is_empty() || !clip.intersects(rect) {
                    continue;
                }
                let rect = rect.intersect(clip);
                if rect.width() > 0.0 && rect.height() > 0.0 {
                    out.push(ScreenText {
                        text: words.trim().to_string(),
                        rect,
                    });
                }
            }
        }
        _ => {}
    }
}
