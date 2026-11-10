// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Driving the window as a person does: see what is drawn, click a word,
//! type, press a key. The egui window's answer to the control port's
//! contract (`dm_desktop_base::probe::WindowProbe`): `ui.texts`, `ui.click`,
//! `ui.type`, `ui.key` and the rest, and so the AI client's tools.
//!
//! What is on screen is read from the frame's own text shapes, back to
//! front, so a click lands on the word a person would see on top. Clicks,
//! typing and keys are fed to egui as the pointer and keyboard would, one
//! step a frame, so a button needs the hover, press and release a real one
//! does.

use super::*;
use serde_json::Value;

/// How many steps may wait for frames before more are refused.
pub const MAX_WAITING_STEPS: usize = 64;

/// How long a step may wait for a frame before it is dropped.
const STEP_EXPIRES: Duration = Duration::from_secs(5);

/// One piece of text drawn this frame.
#[derive(Clone, Debug)]
pub struct ScreenText {
    /// The text drawn.
    pub text: String,
    /// Where it is drawn, in points.
    pub rect: egui::Rect,
    /// The window it is drawn in: the main window, or a dialog's own.
    pub viewport: egui::ViewportId,
}

use dm_desktop_base::probe::{self, is_run_a_file, WindowProbe};

impl DmApp {
    /// Whether a control client acted a moment ago. What runs a downloaded
    /// file, and the command dm runs when a download finishes, are refused
    /// then: words on a button are no proof of what it does (a double-click
    /// on a row opens it, once Options says so), so the refusal is made
    /// where the file would be run, not only where the click is aimed.
    pub(super) fn by_script(&self) -> bool {
        self.scripted_at.by_script()
    }

    /// Why a downloaded file may not be run now: a control client acted a moment ago
    /// ([`Self::by_script`]). `None` when a person's click may run it. Asked where each file is
    /// run, File › Open and the Open of Download complete and of Properties alike: a key pressed
    /// on a focused button (Tab, then Enter) reaches it where a scripted click on its word is
    /// refused (security review of 2026-11-07, gaps row 159 f).
    pub(crate) fn run_refused(&self) -> Option<&'static str> {
        self.scripted_at.run_refused()
    }

    /// Reads the words drawn this frame. Run at the end of the frame, and
    /// only when a control client may ask.
    pub fn scan_screen(&mut self, ctx: &egui::Context) {
        if self.control.is_none() {
            return;
        }
        let mut seen = scan_viewport(ctx, egui::ViewportId::ROOT);
        // The dialogs' windows are in front of the main window, and read in
        // the order they were drawn.
        for (viewport, _) in crate::dialogs::apart::windows() {
            seen.extend(crate::dialogs::apart::screen_of(viewport));
        }
        self.screen = seen;
    }

    /// Counts a painted pass: what a step of a script waits for.
    pub fn painted(&mut self) {
        self.passes = self.passes.wrapping_add(1);
    }

    /// Events for the pass about to run: one step of what was asked, and
    /// only once the window has been painted since the step before.
    ///
    /// eframe asks for these before every pass, the ones that only run the
    /// app's logic included, and carries the input of such a pass over to
    /// the next painted one. A step given to each call piled up there: the
    /// move, the press and the release reached egui in one painted pass, or
    /// many times over, and a button in a dialog took the press and not the
    /// release (the QA lost OK and Start now, four runs of five, whenever
    /// eframe ran its logic passes between paintings).
    pub fn synthetic_events(&mut self, ctx: &egui::Context) -> Vec<egui::Event> {
        if !self.synthetic.is_empty() && self.delivered_at_pass == Some(self.passes) {
            ctx.request_repaint();
            // A window the system does not draw (covered whole by another,
            // on macOS) gets no painted pass however often one is asked
            // for, and a step waits for ever: brought to the front, as a
            // person driving it would have it in sight.
            if self
                .delivered_at
                .is_some_and(|at| at.elapsed() > Duration::from_millis(400))
            {
                self.delivered_at = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            return Vec::new();
        }
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
        if !self.synthetic.is_empty() {
            self.delivered_at_pass = Some(self.passes);
            self.delivered_at = Some(Instant::now());
        }
        let (viewport, events) = match self.synthetic.pop_front().map(|(_, step)| step) {
            None => (egui::ViewportId::ROOT, Vec::new()),
            // Typing and keys go where the keyboard is: the dialog that has
            // it, else the main window.
            Some(Step::Events(events)) => (crate::dialogs::apart::typing_target(), events),
            Some(Step::Pointer {
                text,
                nth,
                button,
                pressed,
            }) => match self
                .find_text(&text, nth)
                .map(|found| (found.viewport, found.rect.center()))
            {
                Some((viewport, pos)) => (
                    viewport,
                    vec![match pressed {
                        None => egui::Event::PointerMoved(pos),
                        Some(pressed) => egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    }],
                ),
                // Gone from the screen between the asking and this frame:
                // the step is dropped, as a pointer over nothing does nothing.
                None => (egui::ViewportId::ROOT, Vec::new()),
            },
        };
        // The moment an event is given to the window is the script's, not
        // the moment it was asked for.
        if !events.is_empty() {
            self.scripted_at.acted();
        }
        // A dialog's own window takes its input when it is drawn, a moment
        // later in this frame (`dialogs::apart::feed`).
        let events = if viewport == egui::ViewportId::ROOT {
            events
        } else {
            crate::dialogs::apart::queue_input(viewport, events);
            Vec::new()
        };
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
        probe::input(&mut Probe { app: self, ctx }, method, params)
    }

    /// Answers the requests that read or set the view and run commands
    /// (`probe::view`), which a `batch` reaches too.
    pub(super) fn view_call(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value, String>> {
        probe::view(
            &mut Probe {
                app: self,
                ctx: None,
            },
            method,
            params,
        )
    }
}

/// The egui window as the control port's [`WindowProbe`]: the app, and the
/// frame's context where there is one (none headless, and in some tests).
struct Probe<'a> {
    app: &'a mut DmApp,
    ctx: Option<&'a egui::Context>,
}

impl Probe<'_> {
    fn wake(&self) {
        if let Some(ctx) = self.ctx {
            ctx.request_repaint();
        }
    }
}

fn rect_of(r: egui::Rect) -> probe::Rect {
    [r.min.x, r.min.y, r.max.x, r.max.y]
}

impl WindowProbe for Probe<'_> {
    fn may_step(&mut self) -> Result<(), String> {
        // Steps wait for frames: a window that draws none (parked in the
        // tray) would let a client queue them without end.
        if self.app.hidden {
            return Err(
                "dm's window is hidden in the tray: launch dm again, or choose Show in the tray, first"
                    .into(),
            );
        }
        if self.app.synthetic.len() > MAX_WAITING_STEPS {
            return Err("too many steps are waiting: let the window catch up".into());
        }
        // Only what was taken counts as a script's moment.
        self.app.scripted_at.acted();
        Ok(())
    }

    fn has_window(&self) -> bool {
        self.ctx.is_some()
    }

    fn texts(&mut self) -> Result<probe::Texts, String> {
        Ok(probe::Texts::new(
            self.app
                .screen
                .iter()
                .rev()
                .map(|t| probe::Text {
                    text: t.text.clone(),
                    rect: rect_of(t.rect),
                    window: if t.viewport == egui::ViewportId::ROOT {
                        probe::Window::Main
                    } else {
                        probe::Window::Dialog
                    },
                })
                .collect(),
        ))
    }

    fn click(&mut self, click: probe::Click) -> Result<probe::Clicked, String> {
        let probe::Click {
            text: wanted,
            nth,
            button,
            double,
        } = click;
        let target = self.app.find_text(&wanted, nth).cloned().ok_or_else(|| {
            let near: Vec<&str> = self
                .app
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
        let button = match button {
            probe::Button::Secondary => egui::PointerButton::Secondary,
            probe::Button::Primary => egui::PointerButton::Primary,
        };
        // Each step finds the word where it is drawn when the step
        // reaches a frame, not where it was when asked: a dialog
        // that had just opened was still settling, and one closing
        // still showed a title that read the same, so a click aimed
        // at the frame before landed beside its button.
        let at = |pressed: Option<bool>| Step::Pointer {
            text: wanted.clone(),
            nth,
            button,
            pressed,
        };
        let synthetic = &mut self.app.synthetic;
        synthetic.push_back((Instant::now(), at(None)));
        step(synthetic, Vec::new());
        // The press and the release a frame apart, as a pointer's
        // are: in one frame, a button in a dialog took the press and
        // not the release (the QA lost Start now and OK, three runs
        // of four). A release that comes late is still a click: the
        // window sets egui's click-duration limit high (`theme`),
        // since a slow frame on a busy computer once came past it.
        for _ in 0..if double { 2 } else { 1 } {
            synthetic.push_back((Instant::now(), at(Some(true))));
            synthetic.push_back((Instant::now(), at(Some(false))));
        }
        step(synthetic, Vec::new());
        self.wake();
        Ok(probe::Clicked {
            clicked: target.text,
            at: [pos.x, pos.y],
        })
    }

    fn scroll(&mut self, scroll: probe::Scroll) -> Result<probe::Scrolled, String> {
        let probe::Scroll {
            text: wanted,
            lines,
        } = scroll;
        self.app
            .find_text(&wanted, 0)
            .ok_or_else(|| format!("nothing on screen says {wanted:?}"))?;
        self.app.synthetic.push_back((
            Instant::now(),
            Step::Pointer {
                text: wanted.clone(),
                nth: 0,
                button: egui::PointerButton::Primary,
                pressed: None,
            },
        ));
        // Lines turned down (negative: up), as a wheel is, a line being a
        // few rows of text.
        step(
            &mut self.app.synthetic,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -lines * 40.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        self.wake();
        Ok(probe::Scrolled {
            scrolled: wanted,
            lines,
        })
    }

    fn type_text(&mut self, text: &str) -> Result<usize, String> {
        step(
            &mut self.app.synthetic,
            vec![egui::Event::Text(text.to_string())],
        );
        self.wake();
        Ok(text.chars().count())
    }

    fn key(&mut self, press: probe::KeyPress) -> Result<(), String> {
        let name = press.key;
        let key = egui::Key::from_name(&name).ok_or_else(|| format!("no key is called {name}"))?;
        let m = press.modifiers;
        let modifiers = egui::Modifiers {
            alt: m.alt,
            ctrl: m.ctrl,
            shift: m.shift,
            mac_cmd: m.cmd,
            command: m.ctrl && !cfg!(target_os = "macos") || m.cmd,
        };
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        };
        step(&mut self.app.synthetic, vec![event(true)]);
        step(&mut self.app.synthetic, vec![event(false)]);
        self.wake();
        Ok(())
    }

    // What a screen reader is given of each window (macOS; gaps row 160).
    fn accessibility(&mut self) -> Result<Value, String> {
        crate::a11y_tree::windows()
    }

    fn windows(&mut self) -> Result<probe::Windows, String> {
        let (main, dialogs) = crate::dialogs::apart::placement();
        Ok(probe::Windows {
            // Points on the screen are this many pixels each.
            pixels_per_point: self.ctx.map(|c| c.pixels_per_point()),
            main: main.map(rect_of),
            dialogs: dialogs
                .iter()
                .map(|(_, w)| probe::DialogWindow {
                    title: w.title.clone(),
                    outer: w.outer.map(rect_of),
                    inner: w.inner.map(rect_of),
                })
                .collect(),
        })
    }

    // Where the main window is, as a person puts it: in the tray (the
    // close button's "tray"), minimized, or shown and in front. For
    // checks of the window's slower wakes while it draws nothing.
    fn window_to(&mut self, to: probe::WindowTo) -> Result<(), String> {
        let ctx = self.ctx.ok_or("there is no window")?;
        match to {
            probe::WindowTo::Tray => self.app.hide_asked = true,
            probe::WindowTo::Minimized => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true))
            }
            probe::WindowTo::Shown => self.app.show_asked = true,
        }
        ctx.request_repaint();
        Ok(())
    }

    fn move_window(&mut self, to: probe::MoveWindow) -> Result<probe::Moved, String> {
        let ctx = self.ctx.ok_or("there is no window to move")?;
        let at = egui::pos2(to.to[0], to.to[1]);
        let (_, dialogs) = crate::dialogs::apart::placement();
        let (window, found) = dialogs
            .into_iter()
            .find(|(_, w)| w.title.to_lowercase().contains(&to.title))
            .ok_or("no dialog window with that title is open")?;
        ctx.send_viewport_cmd_to(window, egui::ViewportCommand::OuterPosition(at));
        Ok(probe::Moved {
            moved: found.title,
            to: to.to,
        })
    }

    fn select(&mut self, ids: Vec<u64>) -> Result<Vec<u64>, String> {
        self.app.select_ids(ids)
    }

    fn place(&mut self, place: &str) -> Result<(), String> {
        self.app.place_named(place)
    }

    fn inspect(&mut self) -> Result<probe::Inspect, String> {
        Ok(self.app.inspected())
    }

    fn commands(&mut self) -> Vec<probe::CommandState> {
        self.app.command_states()
    }

    fn run_command(&mut self, id: &str) -> Result<probe::Ran, String> {
        self.app.run_named(id)
    }
}

/// One step for a frame, stamped with when it was asked for.
/// One step of what a script asked, for one frame.
pub(super) enum Step {
    /// Events as egui takes them.
    Events(Vec<egui::Event>),
    /// The pointer moved onto (`pressed` none), pressed on or released on
    /// the `nth` word reading `text`, found where it is drawn when this
    /// step reaches a frame.
    Pointer {
        text: String,
        nth: usize,
        button: egui::PointerButton,
        pressed: Option<bool>,
    },
}

fn step(queue: &mut std::collections::VecDeque<(Instant, Step)>, events: Vec<egui::Event>) {
    queue.push_back((Instant::now(), Step::Events(events)));
}

impl DmApp {
    /// The `nth` piece of text on screen reading `wanted` (whole, else as a
    /// part), nearest the front first.
    fn find_text(&self, wanted: &str, nth: usize) -> Option<&ScreenText> {
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
        // Last, words drawn shortened ("Delete Co…", a toolbar button in a
        // narrow window), as a person reads and clicks them: what is shown
        // begins the words asked for.
        if found.is_empty() {
            found = self
                .screen
                .iter()
                .rev()
                .filter(|t| {
                    let have = t.text.trim().to_lowercase();
                    have.strip_suffix('…')
                        .map(str::trim_end)
                        .is_some_and(|shown| shown.chars().count() >= 4 && lower.starts_with(shown))
                })
                .collect();
        }
        found.get(nth).copied()
    }
}

/// The words drawn so far in the window being drawn, back to front.
pub(crate) fn scan_viewport(ctx: &egui::Context, viewport: egui::ViewportId) -> Vec<ScreenText> {
    let mut layers = vec![egui::LayerId::background()];
    // Each layer once: a window's panel is the background layer, which the
    // memory lists too.
    for layer in ctx.memory(|m| m.layer_ids().collect::<Vec<_>>()) {
        if !layers.contains(&layer) {
            layers.push(layer);
        }
    }
    let mut seen = Vec::new();
    ctx.graphics(|graphics| {
        for layer in &layers {
            let Some(list) = graphics.get(*layer) else {
                continue;
            };
            for clipped in list.all_entries() {
                collect(&clipped.shape, clipped.clip_rect, viewport, &mut seen);
            }
        }
    });
    seen
}

fn collect(
    shape: &egui::Shape,
    clip: egui::Rect,
    viewport: egui::ViewportId,
    out: &mut Vec<ScreenText>,
) {
    match shape {
        egui::Shape::Vec(inner) => inner.iter().for_each(|s| collect(s, clip, viewport, out)),
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
                        viewport,
                    });
                }
            }
        }
        _ => {}
    }
}
