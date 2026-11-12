// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl DmApp {
    /// Takes requests from the control protocol from now on.
    pub fn attach_control(&mut self, requests: std::sync::mpsc::Receiver<crate::control::Request>) {
        self.control = Some(requests);
    }

    /// From now on the control port answers only AI assistants.
    pub fn restrict_control_to_assistants(&mut self, only: bool) {
        self.mcp_only = only;
    }

    pub(super) fn answer_control(&mut self, ctx: &egui::Context) {
        let mut asked = Vec::new();
        if let Some(requests) = &self.control {
            while let Ok(request) = requests.try_recv() {
                asked.push(request);
            }
        }
        for request in asked {
            // Who may ask is the application's (`control_api::refused`).
            if let Some(why) = dm_shell::app::control_api::refused(
                request.via_mcp,
                self.mcp_only,
                self.applied.allow_mcp,
            ) {
                let _ = request.reply.send(Err(why));
                continue;
            }
            if request.method == "ui.screenshot" && self.headless {
                let _ = request.reply.send(Err(
                    "dm is running headless: there is no window to picture".into(),
                ));
                continue;
            }
            if request.method == "ui.screenshot" {
                let path =
                    dm_shell::app::control_api::screenshot_path(&dm_shell::home(), &request.params);
                // Answered when the integration has read the frame's pixels.
                let reply = request.reply;
                ctx.request_screenshot(move |image| {
                    let _ = reply.send(
                        save_png(&image, &path)
                            .map(|()| serde_json::json!({ "path": path.display().to_string() })),
                    );
                });
                ctx.request_repaint();
                continue;
            }
            let answer = match self.human_call(&request.method, &request.params, Some(ctx)) {
                Some(answer) => answer,
                None => self.control_call(&request.method, &request.params),
            };
            let _ = request.reply.send(answer);
        }
    }

    /// Answers one control request on the window's own thread.
    pub fn control_call(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use serde_json::json;
        // The window's view and its commands, as the contract says them
        // (`dm_desktop_base::probe::view`).
        if let Some(answer) = self.view_call(method, params) {
            return answer;
        }
        match method {
            // UI-free methods answered by the application
            // (`dm_shell::app::control_api::answer`).
            "queue.list" | "downloads.pause" | "downloads.resume" | "downloads.cancel" => {
                dm_shell::app::control_api::answer(&self.backend, &self.rows, method, params)
                    .unwrap_or_else(|| Err(format!("no method is called {method}")))
            }
            // An exact ceiling in KB/s, where the commands only have presets;
            // 0 is no limit.
            "downloads.speed_limit" => {
                let (wanted, replaced) =
                    dm_shell::app::control_api::speed_limit(&self.applied, params)?;
                if let Some(limit) = replaced {
                    self.last_limit = limit;
                }
                self.apply_settings(wanted)?;
                Ok(json!({ "speed_limit_kb": self.applied.speed_limit }))
            }
            "downloads.add" => {
                let asked =
                    dm_shell::app::control_api::add_asked(&self.rows, &self.applied, params)?;
                let added = if asked.fresh.is_empty() {
                    0
                } else {
                    self.add(&asked.fresh, asked.with)?
                };
                Ok(json!({ "added": added, "already_listed": asked.listed }))
            }
            // Up to 256 requests in one, answered in order
            // (`control_api::batch`).
            "batch" => dm_shell::app::control_api::batch(params, |method, params| {
                self.control_call(method, params)
            }),
            other => Err(format!("no method is called {other}")),
        }
    }
}

impl DmApp {
    /// `ui.select`: the rows chosen, of those asked for that are listed.
    pub(super) fn select_ids(&mut self, ids: Vec<Id>) -> Result<Vec<Id>, String> {
        self.selected = ids
            .into_iter()
            .filter(|id| self.row(*id).is_some())
            .collect();
        Ok(self.selected.iter().copied().collect())
    }

    /// `ui.place`: the place a word names.
    pub(super) fn place_named(&mut self, place: &str) -> Result<(), String> {
        let customs: Vec<String> = self
            .applied
            .custom_categories
            .iter()
            .map(|c| c.name.clone())
            .collect();
        self.place = Place::from_word(place, &self.backend.queues(), &customs)?;
        Ok(())
    }

    /// `ui.inspect`: the view, and each row under the name the window shows
    /// it by, the one it will be saved as (the control protocol said the
    /// name a second copy would not get).
    pub(super) fn inspected(&self) -> dm_desktop_base::probe::Inspect {
        use dm_desktop_base::probe;
        let mut categories = self.applied.categories();
        let before = self.earlier_of_the_same_name();
        let rows = self
            .visible_shared()
            .iter()
            .map(|r| probe::InspectRow {
                id: r.id,
                name: self.name_it_will_get_after(
                    r,
                    &mut categories,
                    Some(before.get(&r.id).copied().unwrap_or(0)),
                ),
                status: status_in_english(r),
                queue: r.queue.clone(),
                description: r.description.clone(),
                url: format::shown_url(&r.url),
                rect: self
                    .row_rects
                    .get(&r.id)
                    .map(|b| [b.min.x, b.min.y, b.max.x, b.max.y]),
            })
            .collect();
        probe::Inspect {
            place: format!("{:?}", self.place),
            selected: self.selected.iter().copied().collect(),
            sort: self.sort.map(|(c, up)| probe::Sort {
                column: c.english().to_string(),
                ascending: up,
            }),
            categories: self.show_categories,
            toolbar: self.show_toolbar,
            dark: self.dark,
            // Whether the window is parked in the tray and not
            // drawing; false for a window the compositor shows.
            hidden: self.hidden,
            said: self.said.english(),
            problems: self.problems.clone(),
            dialogs: self.dialogs.open_names(),
            rows,
        }
    }

    /// `commands.list`: every command, and whether it applies now.
    pub(super) fn command_states(&self) -> Vec<dm_desktop_base::probe::CommandState> {
        Command::ALL
            .iter()
            .map(|c| dm_desktop_base::probe::CommandState {
                id: c.id(),
                label: c.label(),
                enabled: self.enabled(*c),
            })
            .collect()
    }

    /// `command.run`: a command by its stable name.
    pub(super) fn run_named(&mut self, id: &str) -> Result<dm_desktop_base::probe::Ran, String> {
        let command = Command::from_id(id).ok_or_else(|| format!("no command is called {id}"))?;
        // Opening a downloaded file is a person's click in the
        // window (`control_api::person_only`).
        if let Some(why) = dm_shell::app::control_api::person_only(command.id()) {
            return Err(why);
        }
        if !self.enabled(command) {
            return Err(format!("{id} has nothing to act on"));
        }
        // A script cannot answer "Exit while downloading?": asked
        // over this connection, dm quits.
        if command == Command::Quit {
            self.quit_asked = true;
        } else {
            self.run(command);
        }
        Ok(dm_desktop_base::probe::Ran {
            ran: id.to_string(),
            said: self.said.english(),
        })
    }
}
