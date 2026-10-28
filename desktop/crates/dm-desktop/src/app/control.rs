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
                let name = dm_core::probe::sanitize(&name);
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
                    .visible()
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
            "queue.list" => Ok(json!(self
                .rows
                .iter()
                .map(|r| {
                    let mut value = serde_json::to_value(r).unwrap_or_default();
                    value["url"] = json!(format::shown_url(&r.url));
                    // A site's sign-in, handed over by the browser, is the
                    // row's to send to that server and nobody's to read: not
                    // a script's, and not an AI client's through MCP. Said
                    // only as whether there is one.
                    if let Some(row) = value.as_object_mut() {
                        row.remove("cookies");
                        row.insert("signed_in".into(), json!(!r.cookies.is_empty()));
                    }
                    value
                })
                .collect::<Vec<_>>())),
            // By id and not by selection, so a script or the command line
            // leaves the person's selection in the window alone.
            "downloads.pause" | "downloads.resume" | "downloads.cancel" => {
                let ids: Vec<Id> = params
                    .get("ids")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
                    .unwrap_or_default();
                if ids.is_empty() {
                    return Err("give `ids`: the downloads to act on".into());
                }
                let (mut done, mut skipped) = (Vec::new(), Vec::new());
                for id in ids {
                    let Some(state) = self
                        .row(id)
                        .map(|r| (r.state.clone(), r.paused_by_schedule))
                    else {
                        skipped.push(json!({ "id": id, "why": "no such download" }));
                        continue;
                    };
                    let (state, held) = state;
                    let why = match (method, &state) {
                        ("downloads.pause", State::Running | State::Queued) => {
                            self.backend.pause(id);
                            None
                        }
                        ("downloads.pause", _) => Some("it is not running"),
                        ("downloads.resume", State::Paused) if held => {
                            Some("the schedule or the download limit holds it")
                        }
                        ("downloads.resume", State::Paused) => {
                            self.backend.resume(id);
                            None
                        }
                        ("downloads.resume", State::Failed(_)) => {
                            self.backend.retry(id);
                            None
                        }
                        ("downloads.resume", _) => Some("it is not paused or failed"),
                        (_, State::Completed | State::Cancelled) => Some("it is already settled"),
                        _ => {
                            self.backend.cancel(id);
                            None
                        }
                    };
                    match why {
                        None => done.push(id),
                        Some(why) => skipped.push(json!({ "id": id, "why": why })),
                    }
                }
                Ok(json!({ "done": done, "skipped": skipped }))
            }
            // An exact ceiling in KB/s, where the commands only have presets;
            // 0 is no limit.
            "downloads.speed_limit" => {
                let kb = params
                    .get("kb")
                    .and_then(|v| v.as_u64())
                    .ok_or("give `kb`: the limit in KB/s, or 0 for none")?;
                if kb > 100_000_000 {
                    return Err("`kb` is at most 100000000".into());
                }
                let mut wanted = self.applied.clone();
                if wanted.speed_limit > 0 {
                    self.last_limit = wanted.speed_limit;
                    wanted.last_speed_limit = wanted.speed_limit;
                }
                wanted.speed_limit = kb;
                self.apply_settings(wanted)?;
                Ok(json!({ "speed_limit_kb": self.applied.speed_limit }))
            }
            "downloads.add" => {
                let urls: Vec<String> = match params.get("urls").or_else(|| params.get("url")) {
                    Some(serde_json::Value::String(one)) => vec![one.clone()],
                    Some(serde_json::Value::Array(many)) => many
                        .iter()
                        .filter_map(|u| u.as_str().map(str::to_owned))
                        .collect(),
                    _ => return Err("give `urls`: a link or a list of links".into()),
                };
                // Nothing to add is a mistake in the asking, not success.
                if urls.iter().all(|u| u.trim().is_empty()) {
                    return Err("give `urls`: a link or a list of links; that list is empty".into());
                }
                let text = |key: &str| {
                    params
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                        .filter(|s| !s.is_empty())
                };
                let later = params
                    .get("later")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                // Over this connection a download goes into the download
                // folder or below it, not into a startup folder or a
                // shell's profile.
                if let Some(folder) = text("folder") {
                    let full = dm_shell::settings::full_folder(&folder, None)
                        .ok_or("`folder` must be a full path, such as ~/Downloads")?;
                    let inside = full.starts_with(&self.applied.download_dir)
                        && !full
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir));
                    if !inside {
                        return Err(format!(
                            "`folder` must be inside the download folder, {}",
                            self.applied.download_dir.display()
                        ));
                    }
                }
                let with = dm_core::AddOptions {
                    later,
                    queue: if later {
                        MAIN_QUEUE.to_string()
                    } else {
                        String::new()
                    },
                    save_dir: text("folder"),
                    save_name: text("name"),
                    description: text("description").unwrap_or_default(),
                    ..dm_core::AddOptions::default()
                };
                /* An address already listed is not added a second time
                 * unless `again` says so: Add URL asks a person, and a
                 * script that repeats itself (or an AI client that calls
                 * twice) made two rows fetching one file.
                 * Said back, so the caller knows. */
                let again = params
                    .get("again")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                // The rows' addresses once, not a search of every row for each
                // link: 6 ms a link at 12 000 rows, a minute and a half for
                // 20 000.
                let known = if again {
                    Default::default()
                } else {
                    crate::dialogs::listed_addresses(self)
                };
                let (mut listed, fresh): (Vec<String>, Vec<String>) = urls
                    .into_iter()
                    .partition(|u| !again && known.contains(&crate::dialogs::bare(u)));
                // Each address once: a script that repeats a link a thousand
                // times was answered with it a thousand times.
                let mut seen = std::collections::HashSet::new();
                listed.retain(|u| seen.insert(u.clone()));
                let added = if fresh.is_empty() {
                    0
                } else {
                    self.add(&fresh, with)?
                };
                Ok(json!({ "added": added, "already_listed": listed }))
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
