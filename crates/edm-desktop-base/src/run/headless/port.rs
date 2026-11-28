// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What EDM with no window answers over the control port: its requests, the
//! commands they run, and the view a window probe would see.

use std::sync::Arc;

use edm_core::Entry;
use edm_shell::app::commands::{self, Command, Ran};
use edm_shell::app::control_api;
use edm_shell::app::names::name_of;
use edm_shell::app::places::Place;
use serde_json::{json, Value};

use super::{english, Headless, NO_WINDOW};
use crate::control::Request;
use crate::probe::{self, WindowProbe};

impl Headless {
    /// Answers what the control port asked, in order: who may ask as the
    /// application says, then [`Self::answer`].
    pub(super) fn answer_requests(&mut self) {
        let asked: Vec<Request> = match &self.requests {
            Some(requests) => requests.try_iter().collect(),
            None => return,
        };
        let allow_mcp = self.backend.settings().general.allow_mcp;
        for request in asked {
            let answer =
                match control_api::refused(request.via_mcp, self.assistants_only, allow_mcp) {
                    Some(why) => Err(why),
                    None => self.answer(&request.method, &request.params),
                };
            let _ = request.reply.send(answer);
        }
    }

    /// One control request: a screenshot is refused (there is no window to
    /// picture), a step says there is no window, and the view, the
    /// commands and the application answer as under a window.
    pub fn answer(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        if method == "ui.screenshot" {
            return Err(probe::NO_WINDOW_TO_PICTURE.into());
        }
        if let Some(answer) = probe::input(&mut Probe(self), method, params) {
            return answer;
        }
        self.answer_inside(method, params)
    }

    /// What a `batch` reaches as well: the view and the application.
    fn answer_inside(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        if let Some(answer) = probe::view(&mut Probe(self), method, params) {
            return answer;
        }
        if let Some(answer) = control_api::answer(&self.backend, &self.rows, method, params) {
            self.read();
            return answer;
        }
        match method {
            "downloads.add" => {
                let settings = self.backend.settings();
                let asked = control_api::add_asked(&self.rows, &settings, params)?;
                let added = if asked.fresh.is_empty() {
                    0
                } else {
                    let items = asked
                        .fresh
                        .iter()
                        .map(|url| (url.clone(), asked.with.clone()))
                        .collect();
                    self.add_items(items)?
                };
                Ok(json!({ "added": added, "already_listed": asked.listed }))
            }
            // An exact ceiling in KB/s; 0 is no limit. A script's change
            // keeps what makes EDM run something.
            "downloads.speed_limit" => {
                let (wanted, replaced) =
                    control_api::speed_limit(&self.backend.settings(), params)?;
                if let Some(limit) = replaced {
                    self.last_limit = limit;
                }
                let (now, _) = self.backend.set_settings_from(wanted, true);
                self.session.forget_limit();
                Ok(json!({ "speed_limit_kb": now?.advanced.speed_limit }))
            }
            "batch" => {
                control_api::batch(params, |method, params| self.answer_inside(method, params))
            }
            other => Err(format!("no method is called {other}")),
        }
    }

    /// Carries out `command` where there is no window: the application's
    /// own (a row's verbs, the queues, the speed limiter, When done, Exit);
    /// what needs a window is refused, said why.
    fn run(&mut self, command: Command) -> Result<(), String> {
        let chosen: Vec<Entry> = self.chosen().into_iter().cloned().collect();
        if let Some(wanted) = commands::when_done(command) {
            self.session.when_done = wanted;
            return Ok(());
        }
        match command {
            // Nobody to ask "Exit while downloading?": over the port, EDM
            // ends.
            Command::Quit => self.quit_asked = true,
            Command::SelectAll => {
                self.selected = self.shown().iter().map(|r| r.id).collect();
            }
            Command::SpeedLimiterToggle
            | Command::SpeedLow
            | Command::SpeedMedium
            | Command::SpeedHigh
            | Command::SpeedUnlimited => {
                let now = self.backend.settings();
                let wanted = commands::limited(command, &now, &mut self.last_limit).unwrap_or(now);
                let kb = self.backend.set_settings(wanted)?.advanced.speed_limit;
                self.said = if kb > 0 {
                    english("Speed limiter on: {} KB/s.", 1, &[&kb])
                } else {
                    "Speed limiter off.".into()
                };
                self.session.forget_limit();
            }
            Command::StartQueue | Command::StopQueue => self.run_on_queue(command),
            _ => {
                let queue = self.current_queue();
                let ran =
                    commands::verb(command, &self.backend, &chosen, &queue).ok_or_else(|| {
                        format!(
                            "{} needs EDM's window: EDM is running headless",
                            command.id()
                        )
                    })?;
                self.read();
                match ran {
                    Ran::Cleared(n) => {
                        self.said = english(
                            "Deleted {} completed or cancelled download from the list.||Deleted {} completed or cancelled downloads from the list.",
                            n as u64,
                            &[&n],
                        );
                    }
                    // Resume on rows the schedule or the download limit
                    // holds: each of the others went on; these wait, with
                    // nobody to ask whether to start them outside it.
                    Ran::WentOn { held } if !held.is_empty() => {
                        self.said = "Waiting for the schedule or speed limit.".into();
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Start Queue or Stop Queue on the place's queue, and said.
    fn run_on_queue(&mut self, command: Command) {
        let queue = self.current_queue();
        let ran = commands::verb(command, &self.backend, &[], &queue);
        self.read();
        match (command, ran) {
            (Command::StartQueue, ran) => {
                let n = match ran {
                    Some(Ran::QueueStarted(n)) => n,
                    _ => 0,
                };
                if n > 0 {
                    if let Some(path) = self.sound("queue_started") {
                        self.backend.platform.play_file(&path);
                    }
                }
                let settings = self.backend.settings();
                let schedule = settings.schedule_for(&queue);
                let lets = !schedule.enabled || schedule.open();
                self.said = if n == 0 {
                    english("Nothing in {} was waiting to start.", 1, &[&queue])
                } else if !lets {
                    english(
                        "{} is waiting for the schedule; it starts by itself when the schedule opens.",
                        1,
                        &[&queue],
                    )
                } else {
                    english(
                        "Started {} download in {}.||Started {} downloads in {}.",
                        n as u64,
                        &[&n, &queue],
                    )
                };
                if queue == edm_core::RECHECK_QUEUE {
                    commands::recheck(&self.backend, Arc::clone(&self.rechecked));
                }
            }
            (_, ran) => {
                let n = match ran {
                    Some(Ran::QueueStopped(n)) => n,
                    _ => 0,
                };
                self.said = english(
                    "Stopped {} download in {}.||Stopped {} downloads in {}.",
                    n as u64,
                    &[&n, &queue],
                );
            }
        }
    }
}

/// EDM with no window as the control port's [`WindowProbe`]: the view and
/// its commands answer; there is nothing to click, picture or move.
struct Probe<'a>(&'a mut Headless);

impl WindowProbe for Probe<'_> {
    fn may_step(&mut self) -> Result<(), String> {
        Err(NO_WINDOW.into())
    }

    fn has_window(&self) -> bool {
        false
    }

    /// Nothing is on a screen.
    fn texts(&mut self) -> Result<probe::Texts, String> {
        Ok(probe::Texts::new(Vec::new()))
    }

    fn click(&mut self, _: probe::Click) -> Result<probe::Clicked, String> {
        Err(NO_WINDOW.into())
    }

    fn scroll(&mut self, _: probe::Scroll) -> Result<probe::Scrolled, String> {
        Err(NO_WINDOW.into())
    }

    fn type_text(&mut self, _: &str) -> Result<usize, String> {
        Err(NO_WINDOW.into())
    }

    fn key(&mut self, _: probe::KeyPress) -> Result<(), String> {
        Err(NO_WINDOW.into())
    }

    fn accessibility(&mut self) -> Result<Value, String> {
        Ok(json!([]))
    }

    fn windows(&mut self) -> Result<probe::Windows, String> {
        Ok(probe::Windows {
            pixels_per_point: None,
            main: None,
            main_title: None,
            dialogs: Vec::new(),
        })
    }

    fn window_to(&mut self, _: probe::WindowTo) -> Result<(), String> {
        Err("there is no window".into())
    }

    fn move_window(&mut self, _: probe::MoveWindow) -> Result<probe::Moved, String> {
        Err("there is no window to move".into())
    }

    fn select(&mut self, ids: Vec<u64>) -> Result<Vec<u64>, String> {
        let listed: std::collections::HashSet<u64> = self.0.rows.iter().map(|r| r.id).collect();
        self.0.selected = ids.into_iter().filter(|id| listed.contains(id)).collect();
        Ok(self.0.selected.iter().copied().collect())
    }

    fn place(&mut self, place: &str) -> Result<(), String> {
        let customs: Vec<String> = self
            .0
            .backend
            .settings()
            .custom_categories
            .iter()
            .map(|c| c.name.clone())
            .collect();
        self.0.place = Place::from_word(place, &self.0.backend.queues(), &customs)?;
        Ok(())
    }

    fn inspect(&mut self) -> Result<probe::Inspect, String> {
        let edm = &*self.0;
        let rows = edm
            .shown()
            .into_iter()
            .map(|row| probe::InspectRow {
                id: row.id,
                name: edm
                    .names
                    .get(&row.id)
                    .cloned()
                    .unwrap_or_else(|| name_of(row)),
                status: crate::rows::status_in_english(row),
                queue: row.queue.clone(),
                description: row.description.clone(),
                url: crate::format::shown_url(&row.url),
                rect: None,
            })
            .collect();
        Ok(probe::Inspect {
            place: format!("{:?}", edm.place),
            selected: edm.selected.iter().copied().collect(),
            sort: None,
            categories: false,
            toolbar: false,
            dark: false,
            hidden: true,
            said: edm.said.clone(),
            problems: edm.problems.clone(),
            dialogs: Vec::new(),
            rows,
        })
    }

    fn commands(&mut self) -> Vec<probe::CommandState> {
        Command::ALL
            .iter()
            .map(|c| probe::CommandState {
                id: c.id(),
                label: c.label(),
                enabled: self.0.enabled(*c),
            })
            .collect()
    }

    fn run_command(&mut self, id: &str) -> Result<probe::Ran, String> {
        let command = Command::from_id(id).ok_or_else(|| format!("no command is called {id}"))?;
        // Opening a downloaded file is a person's click in the window.
        if let Some(why) = control_api::person_only(command.id()) {
            return Err(why);
        }
        if !self.0.enabled(command) {
            return Err(format!("{id} has nothing to act on"));
        }
        self.0.run(command)?;
        Ok(probe::Ran {
            ran: id.to_string(),
            said: self.0.said.clone(),
        })
    }
}
