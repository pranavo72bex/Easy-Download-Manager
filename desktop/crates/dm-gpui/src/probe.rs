// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The shell's answer to the control port: the window's methods through
//! `dm_desktop_base::probe::WindowProbe`, the rest through the application
//! (`dm_shell::app::control_api`), as the egui window answers them. What the
//! shell does not draw yet it says so, by name, rather than answering as if
//! it did.

use dm_desktop_base::probe::{self, WindowProbe};
use gpui::{point, px, Context};
use serde_json::{json, Value};

use crate::list::{line, Downloads, ROW_HEIGHT};

/// Answers one request on the window's thread.
pub fn answer(
    view: &mut Downloads,
    method: &str,
    params: &Value,
    cx: &mut Context<Downloads>,
) -> Result<Value, String> {
    if method == "ui.screenshot" {
        return Err("ui.screenshot is not in the GPUI shell yet".into());
    }
    if let Some(answer) = probe::input(&mut Probe { view, cx }, method, params) {
        return answer;
    }
    answer_inside(view, method, params, cx)
}

/// What a `batch` reaches as well: the view and the application, never the
/// window's input, as with the egui window.
fn answer_inside(
    view: &mut Downloads,
    method: &str,
    params: &Value,
    cx: &mut Context<Downloads>,
) -> Result<Value, String> {
    if let Some(answer) = probe::view(&mut Probe { view, cx }, method, params) {
        return answer;
    }
    if let Some(answer) =
        dm_shell::app::control_api::answer(&view.backend, &view.rows, method, params)
    {
        return answer;
    }
    match method {
        "downloads.add" => {
            let settings = view.backend.settings();
            let asked = dm_shell::app::control_api::add_asked(&view.rows, &settings, params)?;
            let added = if asked.fresh.is_empty() {
                0
            } else {
                let items = asked
                    .fresh
                    .iter()
                    .map(|url| (url.clone(), asked.with.clone()))
                    .collect();
                view.session.add(&view.backend, items).outcome()?
            };
            Ok(json!({ "added": added, "already_listed": asked.listed }))
        }
        // An exact ceiling in KB/s; 0 is no limit. A script's change keeps
        // what makes dm run something (`Backend::set_settings_from`).
        "downloads.speed_limit" => {
            let (wanted, _) =
                dm_shell::app::control_api::speed_limit(&view.backend.settings(), params)?;
            let (now, _) = view.backend.set_settings_from(wanted, true);
            Ok(json!({ "speed_limit_kb": now?.speed_limit }))
        }
        "batch" => dm_shell::app::control_api::batch(params, |method, params| {
            answer_inside(view, method, params, cx)
        }),
        other => Err(format!("no method is called {other}")),
    }
}

/// The shell as a `WindowProbe`.
struct Probe<'a, 'b> {
    view: &'a mut Downloads,
    cx: &'a mut Context<'b, Downloads>,
}

/// The words of a method the shell does not draw yet.
fn not_yet<T>(method: &str) -> Result<T, String> {
    Err(format!("{method} is not in the GPUI shell yet"))
}

impl Probe<'_, '_> {
    /// The rows on screen, with where each line is: (index, top, left, width).
    fn on_screen(&self) -> Vec<(usize, f32, f32, f32)> {
        let state = self.view.scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        let offset = state.base_handle.offset();
        let (top, left) = (f32::from(bounds.origin.y), f32::from(bounds.origin.x));
        let (height, width) = (f32::from(bounds.size.height), f32::from(bounds.size.width));
        let scrolled = -f32::from(offset.y);
        let first = (scrolled / ROW_HEIGHT).floor().max(0.0) as usize;
        let count = (height / ROW_HEIGHT).ceil() as usize + 1;
        (first..(first + count).min(self.view.rows.len()))
            .map(|i| (i, top + i as f32 * ROW_HEIGHT - scrolled, left, width))
            .filter(|(_, y, _, _)| *y + ROW_HEIGHT > top && *y < top + height)
            .collect()
    }
}

impl WindowProbe for Probe<'_, '_> {
    fn may_step(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn has_window(&self) -> bool {
        true
    }

    fn texts(&mut self) -> Result<probe::Texts, String> {
        let mut texts = Vec::new();
        for (i, y, left, width) in self.on_screen() {
            let (name, state) = line(&self.view.rows[i]);
            texts.push(probe::Text {
                text: name.to_string(),
                rect: [left + 12.0, y, left + width / 2.0, y + ROW_HEIGHT],
                window: probe::Window::Main,
            });
            texts.push(probe::Text {
                text: state.to_string(),
                rect: [left + width / 2.0, y, left + width - 12.0, y + ROW_HEIGHT],
                window: probe::Window::Main,
            });
        }
        let top = {
            let state = self.view.scroll.0.borrow();
            f32::from(state.base_handle.bounds().origin.y)
        };
        texts.push(probe::Text {
            text: format!("{} downloads", self.view.rows.len()),
            rect: [12.0, 0.0, 300.0, top],
            window: probe::Window::Main,
        });
        // Nearest the front first: the header is drawn over nothing.
        texts.reverse();
        Ok(probe::Texts::new(texts))
    }

    fn click(&mut self, _: probe::Click) -> Result<probe::Clicked, String> {
        not_yet("ui.click")
    }

    fn scroll(&mut self, scroll: probe::Scroll) -> Result<probe::Scrolled, String> {
        let shown = self.texts()?;
        if !shown.texts.iter().any(|t| t.text == scroll.text) {
            return Err(format!("nothing on screen says {:?}", scroll.text));
        }
        {
            let state = self.view.scroll.0.borrow();
            let handle = &state.base_handle;
            let height = f32::from(handle.bounds().size.height);
            let most = (self.view.rows.len() as f32 * ROW_HEIGHT - height).max(0.0);
            // A line is 40 points, as the egui window turns a wheel's.
            let to = (-f32::from(handle.offset().y) + scroll.lines * 40.0).clamp(0.0, most);
            handle.set_offset(point(px(0.0), px(-to)));
        }
        self.cx.notify();
        Ok(probe::Scrolled {
            scrolled: scroll.text,
            lines: scroll.lines,
        })
    }

    fn type_text(&mut self, _: &str) -> Result<usize, String> {
        not_yet("ui.type")
    }

    fn key(&mut self, _: probe::KeyPress) -> Result<(), String> {
        not_yet("ui.key")
    }

    fn accessibility(&mut self) -> Result<Value, String> {
        dm_desktop_base::a11y_tree::windows()
    }

    fn windows(&mut self) -> Result<probe::Windows, String> {
        not_yet("ui.windows")
    }

    fn window_to(&mut self, _: probe::WindowTo) -> Result<(), String> {
        not_yet("ui.window")
    }

    fn move_window(&mut self, _: probe::MoveWindow) -> Result<probe::Moved, String> {
        not_yet("ui.move_window")
    }

    fn select(&mut self, _: Vec<u64>) -> Result<Vec<u64>, String> {
        not_yet("ui.select")
    }

    fn place(&mut self, _: &str) -> Result<(), String> {
        not_yet("ui.place")
    }

    fn inspect(&mut self) -> Result<probe::Inspect, String> {
        let shown: std::collections::HashMap<usize, (f32, f32, f32)> = self
            .on_screen()
            .into_iter()
            .map(|(i, y, left, width)| (i, (y, left, width)))
            .collect();
        let rows = self
            .view
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let (name, state) = line(row);
                probe::InspectRow {
                    id: row.id,
                    name: name.to_string(),
                    status: state.to_string(),
                    queue: row.queue.clone(),
                    description: row.description.clone(),
                    url: dm_desktop_base::format::shown_url(&row.url),
                    rect: shown
                        .get(&i)
                        .map(|(y, left, width)| [*left, *y, left + width, y + ROW_HEIGHT]),
                }
            })
            .collect();
        Ok(probe::Inspect {
            place: "All".into(),
            selected: Vec::new(),
            sort: None,
            categories: false,
            toolbar: false,
            dark: false,
            hidden: false,
            said: String::new(),
            problems: Vec::new(),
            dialogs: Vec::new(),
            rows,
        })
    }

    fn commands(&mut self) -> Vec<probe::CommandState> {
        Vec::new()
    }

    fn run_command(&mut self, id: &str) -> Result<probe::Ran, String> {
        not_yet(&format!("command.run {id}"))
    }
}
