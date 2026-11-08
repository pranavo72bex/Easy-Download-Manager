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
            if self.mcp_only && !request.via_mcp {
                let _ = request.reply.send(Err(
                    "this dm's control port is open for AI assistants only: tick Options › General › Allow the command line to control dm, or start dm with --control"
                        .to_string(),
                ));
                continue;
            }
            // An AI assistant is answered only while Options allows it, asked
            // afresh each request, so turning it off takes hold at once.
            if request.via_mcp && !self.applied.allow_mcp {
                let _ = request.reply.send(Err(
                    "AI assistants are turned off in dm: tick Options › General › Allow AI assistants to control dm (MCP)"
                        .to_string(),
                ));
                continue;
            }
            if request.method == "ui.screenshot" && self.headless {
                let _ = request.reply.send(Err(
                    "dm is running headless: there is no window to picture".into(),
                ));
                continue;
            }
            if request.method == "ui.screenshot" {
                let name = request
                    .params
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("dm")
                    .to_string();
                let name = dm_core::sanitize_name(&name);
                let name = if name.is_empty() { "dm".into() } else { name };
                let path = dm_shell::home()
                    .join("screenshots")
                    .join(format!("{name}.png"));
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
        match method {
            "commands.list" => Ok(json!(Command::ALL
                .iter()
                .map(|c| json!({ "id": c.id(), "label": c.label(), "enabled": self.enabled(*c) }))
                .collect::<Vec<_>>())),
            "command.run" => {
                let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let command =
                    Command::from_id(id).ok_or_else(|| format!("no command is called {id}"))?;
                // Opening a downloaded file runs it, on Windows, if it is a
                // program: a person's click in the window, never a script's
                // or an AI client's that a web page may have talked into it.
                if matches!(
                    command,
                    Command::OpenFile | Command::OpenWith | Command::Preview
                ) {
                    return Err(format!(
                        "{id} is done in the window by a person, not over this connection"
                    ));
                }
                if !self.enabled(command) {
                    return Err(format!("{id} has nothing to act on"));
                }
                // A script cannot answer "Exit while downloading?": asked
                // over this connection, dm quits.
                if command == Command::Quit {
                    self.quit_asked = true;
                    return Ok(json!({ "ran": id, "said": self.said.english() }));
                }
                self.run(command);
                Ok(json!({ "ran": id, "said": self.said.english() }))
            }
            "ui.select" => {
                let ids: Vec<Id> = params
                    .get("ids")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
                    .unwrap_or_default();
                self.selected = ids
                    .into_iter()
                    .filter(|id| self.row(*id).is_some())
                    .collect();
                Ok(json!({ "selected": self.selected }))
            }
            "ui.place" => {
                let place = params.get("place").and_then(|v| v.as_str()).unwrap_or("");
                self.place = match place {
                    "all" => Place::All,
                    "unfinished" => Place::Unfinished,
                    "finished" => Place::Finished,
                    other => {
                        if let Some(name) = other.strip_prefix("queue:") {
                            // A place nothing is filed under showed an empty
                            // list as if it were a queue.
                            if !self.backend.queues().iter().any(|q| q == name) {
                                return Err(format!("no queue is called {name}"));
                            }
                            Place::Queue(name.to_string())
                        } else if let Some(name) = other.strip_prefix("custom:") {
                            if !self
                                .applied
                                .custom_categories
                                .iter()
                                .any(|c| c.name == name)
                            {
                                return Err(format!("no category is called {name}"));
                            }
                            Place::Custom(name.to_string())
                        } else if let Some(word) = other.strip_prefix("kind:") {
                            let kind = Category::ALL
                                .into_iter()
                                .find(|c| category_word(*c) == word)
                                .ok_or_else(|| format!("no category is called {word}"))?;
                            Place::Kind(kind)
                        } else {
                            return Err(format!("no place is called {other}"));
                        }
                    }
                };
                Ok(json!({ "place": place }))
            }
            "ui.inspect" => {
                // Each row under the name the window shows it by, the one it
                // will be saved as (the control protocol said
                // the name a second copy would not get).
                let mut categories = self.applied.categories();
                let before = self.earlier_of_the_same_name();
                let rows: Vec<_> = self
                    .visible_shared()
                    .iter()
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "name": self.name_it_will_get_after(r, &mut categories, Some(before.get(&r.id).copied().unwrap_or(0))),
                            "status": status_in_english(r),
                            "queue": r.queue,
                            "description": r.description,
                            "url": format::shown_url(&r.url),
                            "rect": self.row_rects.get(&r.id).map(|b| json!([b.min.x, b.min.y, b.max.x, b.max.y])),
                        })
                    })
                    .collect();
                Ok(json!({
                    "place": format!("{:?}", self.place),
                    "selected": self.selected,
                    "sort": self.sort.map(|(c, up)| json!({ "column": c.english(), "ascending": up })),
                    "categories": self.show_categories,
                    "toolbar": self.show_toolbar,
                    "dark": self.dark,
                    // Whether the window is parked in the tray and not
                    // drawing; false for a window the compositor shows.
                    "hidden": self.hidden,
                    "said": self.said.english(),
                    "problems": self.problems,
                    "dialogs": self.dialogs.open_names(),
                    "rows": rows,
                }))
            }
            // UI-free methods answered by the application (`dm_shell::app::control_api`).
            "queue.list" => Ok(dm_shell::app::control_api::queue_list(&self.rows)),
            // By id and not by selection, so a script or the command line
            // leaves the person's selection in the window alone.
            "downloads.pause" | "downloads.resume" | "downloads.cancel" => {
                dm_shell::app::control_api::act_on(&self.backend, method, params)
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
            // Up to 256 requests in one, answered in order: faster for a
            // script than a round trip each.
            "batch" => {
                let steps = params
                    .get("steps")
                    .and_then(|v| v.as_array())
                    .ok_or("give `steps`: a list of {method, params}")?;
                if steps.len() > 256 {
                    return Err("a batch is at most 256 steps".into());
                }
                let mut results = Vec::new();
                for step in steps {
                    let method = step.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    if method == "batch" || method == "ui.screenshot" {
                        results.push(
                            json!({ "ok": false, "error": format!("{method} cannot be batched") }),
                        );
                        continue;
                    }
                    let params = step
                        .get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    results.push(match self.control_call(method, &params) {
                        Ok(result) => json!({ "ok": true, "result": result }),
                        Err(error) => json!({ "ok": false, "error": error }),
                    });
                }
                Ok(json!(results))
            }
            other => Err(format!("no method is called {other}")),
        }
    }
}
