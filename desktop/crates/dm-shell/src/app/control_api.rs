// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The control port's methods that need no window: what a script, the command
//! line or an AI client (MCP) asks of downloads, checked and answered the same
//! whichever presentation runs. The `ui.*` methods, which act like a person on
//! a window, stay with that window; so does carrying out an add or a settings
//! change, which goes through the presentation's own path. Who may ask
//! (`refused`), what is never done over the port (`person_only`) and a
//! `batch` of requests are decided here for every presentation.

use std::collections::HashSet;
use std::sync::Arc;

use dm_core::{Entry, Id, State};
use serde_json::{json, Value};

use super::backend::Backend;
use super::rules::{go_on, GoOn};
use crate::{Error, Settings};

/// `queue.list`: every row as the queue keeps it, its address with any
/// password masked, and a row's sign-in (cookies) and the browser's request
/// headers (a bearer token rides in them) said only as whether there is one:
/// they are the row's to send to its server and nobody's to read, not a
/// script's and not an AI client's.
pub fn queue_list(rows: &[Arc<Entry>]) -> Value {
    json!(rows
        .iter()
        .map(|r| {
            // Redacted by the row itself, so no reader can forget a secret.
            let mut value = serde_json::to_value(r.redacted()).unwrap_or_default();
            if let Some(row) = value.as_object_mut() {
                // Empty after redaction: said only as whether there was one.
                row.remove("cookies");
                row.insert("signed_in".into(), json!(!r.cookies.is_empty()));
            }
            value
        })
        .collect::<Vec<_>>())
}

/// `downloads.pause`, `downloads.resume` and `downloads.cancel` on the rows
/// `ids` names: by id and not by selection, so a script leaves the person's
/// selection alone. Each is judged on the queue's row as it is now, not on a
/// window's copy up to a quarter of a second old (a script that paused and
/// then resumed was told "it is not paused"). Answers what was done and what
/// was skipped, and why.
pub fn act_on(backend: &Backend, method: &str, params: &Value) -> Result<Value, Error> {
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
        let Some((state, held)) = backend.row(id).map(|r| (r.state, r.paused_by_schedule)) else {
            skipped.push(json!({ "id": id, "why": "no such download" }));
            continue;
        };
        let why = match (method, &state) {
            ("downloads.pause", State::Running | State::Queued) => {
                backend.pause(id);
                None
            }
            ("downloads.pause", _) => Some("it is not running"),
            ("downloads.resume", _) => match go_on(&state, held) {
                GoOn::Held => Some("the schedule or the download limit holds it"),
                GoOn::Resume => {
                    backend.resume(id);
                    None
                }
                GoOn::Retry => {
                    backend.retry(id);
                    None
                }
                GoOn::Settled | GoOn::NotStopped => Some("it is not paused or failed"),
            },
            (_, State::Completed | State::Cancelled) => Some("it is already settled"),
            _ => {
                backend.cancel(id);
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

/// `downloads.speed_limit`: an exact ceiling in KB/s (0 is none), as the
/// settings to apply, and the limit it replaces where there was one (kept so
/// the menu can turn it back on).
pub fn speed_limit(applied: &Settings, params: &Value) -> Result<(Settings, Option<u64>), Error> {
    let kb = params
        .get("kb")
        .and_then(|v| v.as_u64())
        .ok_or("give `kb`: the limit in KB/s, or 0 for none")?;
    if kb > 100_000_000 {
        return Err("`kb` is at most 100000000".into());
    }
    let mut wanted = applied.clone();
    let mut replaced = None;
    if wanted.advanced.speed_limit > 0 {
        replaced = Some(wanted.advanced.speed_limit);
        wanted.last_speed_limit = wanted.advanced.speed_limit;
    }
    wanted.advanced.speed_limit = kb;
    Ok((wanted, replaced))
}

/// What `downloads.add` asks for, checked: the addresses to add, the ones
/// already listed (answered back, not added again), and how to add them.
#[derive(Debug, Clone)]
pub struct AddAsked {
    /// Addresses to add.
    pub fresh: Vec<String>,
    /// Addresses already in the list, each once.
    pub listed: Vec<String>,
    /// How each is added: later or now, folder, name, description, part.
    pub with: dm_core::AddOptions,
}

/// The rows' addresses, logins aside: one set built once, not a search of
/// every row for each link (6 ms a link at 12 000 rows, a minute and a half
/// for 20 000).
pub fn listed_addresses(rows: &[Arc<Entry>]) -> HashSet<String> {
    rows.iter()
        .filter(|r| r.state != State::Cancelled)
        .map(|r| crate::lists::bare(&r.url))
        .collect()
}

/// Checks a `downloads.add` request against the rows and the settings in
/// force. A download added over this connection goes into the download folder
/// or below it, not into a startup folder or a shell's profile; a part of a
/// video needs both ends; and an address already listed is not added again
/// unless `again` says so (Add URL asks a person; a script that repeats
/// itself, or an AI client that calls twice, made two rows fetching one file).
pub fn add_asked(
    rows: &[Arc<Entry>],
    applied: &Settings,
    params: &Value,
) -> Result<AddAsked, Error> {
    let urls: Vec<String> = match params.get("urls").or_else(|| params.get("url")) {
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Array(many)) => many
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
    if let Some(folder) = text("folder") {
        let full = crate::settings::full_folder(&folder, None)
            .ok_or("`folder` must be a full path, such as ~/Downloads")?;
        let inside = full.starts_with(&applied.folders.download_dir)
            && !full
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir));
        if !inside {
            return Err(format!(
                "`folder` must be inside the download folder, {}",
                applied.folders.download_dir.display()
            )
            .into());
        }
    }
    // A part of a video: both ends, as "4:10" or seconds.
    let span = match (text("from"), text("to")) {
        (None, None) => None,
        (Some(from), Some(to)) => Some(
            dm_core::Span::parse(&from, &to)
                .ok_or("`from` and `to` are times such as 1:30 or 90, the end after the start")?,
        ),
        _ => return Err("give both `from` and `to` for a part of a video".into()),
    };
    let with = dm_core::AddOptions {
        later,
        queue: if later {
            dm_core::MAIN_QUEUE.to_string()
        } else {
            String::new()
        },
        save_dir: text("folder"),
        save_name: text("name"),
        description: text("description").unwrap_or_default(),
        span,
        ..dm_core::AddOptions::default()
    };
    let again = params
        .get("again")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let known = if again {
        HashSet::new()
    } else {
        listed_addresses(rows)
    };
    let (mut listed, fresh): (Vec<String>, Vec<String>) = urls
        .into_iter()
        .partition(|u| !again && known.contains(&crate::lists::bare(u)));
    // Each address once: a script that repeats a link a thousand times was
    // answered with it a thousand times.
    let mut seen = HashSet::new();
    listed.retain(|u| seen.insert(u.clone()));
    Ok(AddAsked {
        fresh,
        listed,
        with,
    })
}

/// Why a request is refused before it is looked at, or `None` to answer it.
/// `mcp_only`: the port was opened for AI assistants alone; `allow_mcp`: the
/// person's Options › General › Allow AI assistants, asked afresh each
/// request so turning it off takes hold at once.
pub fn refused(via_mcp: bool, mcp_only: bool, allow_mcp: bool) -> Option<String> {
    if mcp_only && !via_mcp {
        return Some(
            "this dm's control port is open for AI assistants only: tick Options › General › Allow the command line to control dm, or start dm with --control"
                .into(),
        );
    }
    if via_mcp && !allow_mcp {
        return Some(
            "AI assistants are turned off in dm: tick Options › General › Allow AI assistants to control dm (MCP)"
                .into(),
        );
    }
    None
}

/// Commands done in the window by a person and never over the control port:
/// opening a downloaded file runs it, on Windows, if it is a program, and a
/// web page may have talked a script or an AI client into it.
pub const PERSON_ONLY: [&str; 3] = ["file.open", "file.open_with", "file.preview"];

/// Why `command.run` of command `id` is refused over this connection, if it is.
pub fn person_only(id: &str) -> Option<String> {
    PERSON_ONLY
        .contains(&id)
        .then(|| format!("{id} is done in the window by a person, not over this connection"))
}

/// The most steps a `batch` answers.
pub const BATCH_MOST: usize = 256;

/// `batch`: up to `BATCH_MOST` requests in one, each answered by `call` in
/// order (faster for a script than a round trip each). A batch inside a
/// batch, or a screenshot, is not answered.
pub fn batch(
    params: &Value,
    mut call: impl FnMut(&str, &Value) -> Result<Value, String>,
) -> Result<Value, String> {
    let steps = params
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or("give `steps`: a list of {method, params}")?;
    if steps.len() > BATCH_MOST {
        return Err(format!("a batch is at most {BATCH_MOST} steps"));
    }
    let mut results = Vec::new();
    for step in steps {
        let method = step.get("method").and_then(|m| m.as_str()).unwrap_or("");
        if method == "batch" || method == "ui.screenshot" {
            results.push(json!({ "ok": false, "error": format!("{method} cannot be batched") }));
            continue;
        }
        let params = step.get("params").cloned().unwrap_or(Value::Null);
        results.push(match call(method, &params) {
            Ok(result) => json!({ "ok": true, "result": result }),
            Err(error) => json!({ "ok": false, "error": error }),
        });
    }
    Ok(json!(results))
}

/// The methods answered from the queue alone, whichever presentation runs:
/// `queue.list`, and `downloads.pause`, `.resume` and `.cancel` by id (not
/// by selection, so a script leaves the person's selection alone). `None`
/// for any other method.
pub fn answer(
    backend: &Backend,
    rows: &[Arc<Entry>],
    method: &str,
    params: &Value,
) -> Option<Result<Value, String>> {
    match method {
        "queue.list" => Some(Ok(queue_list(rows))),
        "downloads.pause" | "downloads.resume" | "downloads.cancel" => {
            Some(act_on(backend, method, params).map_err(String::from))
        }
        _ => None,
    }
}

/// Where `ui.screenshot` saves its picture under `home`: the name asked for,
/// made safe as a file name, or "dm".
pub fn screenshot_path(home: &std::path::Path, params: &Value) -> std::path::PathBuf {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("dm");
    let name = dm_core::sanitize_name(name);
    let name = if name.is_empty() { "dm".into() } else { name };
    home.join(dm_core::paths::SCREENSHOTS)
        .join(format!("{name}.png"))
}

/// How long after a control client clicked, typed or pressed a key what is
/// done is taken for the script's, not a person's.
pub const SCRIPT_ACTS_FOR: std::time::Duration = std::time::Duration::from_secs(3);

/// When a control client last acted on the window. What runs a downloaded
/// file, and the commands dm runs when a download finishes, are refused for
/// a moment after: words on a button are no proof of what it does, so the
/// refusal is made where the file would be run, not only where the click is
/// aimed (security review, gaps row 159 f).
#[derive(Debug, Default, Clone, Copy)]
pub struct ScriptGuard(Option<std::time::Instant>);

/// When a control client last acted on any window, for what no window holds itself: a download
/// added or moved into a folder of a script's typing (`folder_refused`, gaps row 199).
static LAST_ACTED: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Why a download may not be put in `folder` now, or `None`: while a script drives the window
/// ([`SCRIPT_ACTS_FOR`] after it last acted), a folder typed into Add URL, Properties, Save to or
/// Move/Rename is held to the download folder or below it, as the port's own `downloads.add` is
/// (`add_asked`) and Options' folders are (`Settings::keep_places_of`): a script that typed
/// `~/Library/LaunchAgents` there would put a file of a page's choosing where it runs at login
/// (security audit, gaps row 199).
pub fn folder_refused(folder: &std::path::Path, download_dir: &std::path::Path) -> Option<String> {
    let acting = dm_core::lock(&LAST_ACTED).is_some_and(|at| at.elapsed() < SCRIPT_ACTS_FOR);
    folder_refused_while(acting, folder, download_dir)
}

/// [`folder_refused`], with whether a script is acting said outright.
fn folder_refused_while(
    acting: bool,
    folder: &std::path::Path,
    download_dir: &std::path::Path,
) -> Option<String> {
    // Each as the disk has it, links followed (on macOS `/tmp` is `/private/tmp`), so one
    // folder written two ways is one folder; `..` refused outright.
    let below = !folder
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
        && (folder.starts_with(download_dir) || real(folder).starts_with(real(download_dir)));
    (acting && !below).then(|| {
        format!(
            "a script was driving the window a moment ago, so a download goes into the download folder ({}) or below it; choose the folder yourself",
            download_dir.display()
        )
    })
}

/// `path` with its nearest folder that is there resolved as the disk has it (links followed),
/// and the rest, not made yet, after it.
fn real(path: &std::path::Path) -> std::path::PathBuf {
    let mut there = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(resolved) = std::fs::canonicalize(there) {
            return rest.iter().rev().fold(resolved, |p, part| p.join(part));
        }
        match (there.parent(), there.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                there = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

impl ScriptGuard {
    /// A control client acted now.
    pub fn acted(&mut self) {
        let now = std::time::Instant::now();
        self.0 = Some(now);
        *dm_core::lock(&LAST_ACTED) = Some(now);
    }

    /// Whether a control client acted a moment ago ([`SCRIPT_ACTS_FOR`]).
    pub fn by_script(&self) -> bool {
        self.0.is_some_and(|at| at.elapsed() < SCRIPT_ACTS_FOR)
    }

    /// Why a downloaded file may not be run now, or `None` when a person's
    /// click may run it.
    pub fn run_refused(&self) -> Option<&'static str> {
        self.by_script()
            .then_some("Opening a file is done by a person, not by a script")
    }
}

#[cfg(test)]
mod tests;
