// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The control port's methods that need no window: what a script, the command
//! line or an AI client (MCP) asks of downloads, checked and answered the same
//! whichever presentation runs. The `ui.*` methods, which act like a person on
//! a window, stay with that window; so does carrying out an add or a settings
//! change, which goes through the window's own path until the application
//! holds a session of its own.

use std::collections::HashSet;
use std::sync::Arc;

use dm_core::{Entry, Id, State};
use serde_json::{json, Value};

use super::backend::Backend;
use super::rules::{go_on, GoOn};
use crate::Settings;

/// `queue.list`: every row as the queue keeps it, its address with any
/// password masked, and a row's sign-in (cookies) and the browser's request
/// headers (a bearer token rides in them) said only as whether there is one:
/// they are the row's to send to its server and nobody's to read, not a
/// script's and not an AI client's.
pub fn queue_list(rows: &[Arc<Entry>]) -> Value {
    json!(rows
        .iter()
        .map(|r| {
            let mut value = serde_json::to_value(r).unwrap_or_default();
            value["url"] = json!(crate::lists::shown_url(&r.url));
            if let Some(row) = value.as_object_mut() {
                row.remove("cookies");
                row.remove("headers");
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
pub fn act_on(backend: &Backend, method: &str, params: &Value) -> Result<Value, String> {
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
pub fn speed_limit(applied: &Settings, params: &Value) -> Result<(Settings, Option<u64>), String> {
    let kb = params
        .get("kb")
        .and_then(|v| v.as_u64())
        .ok_or("give `kb`: the limit in KB/s, or 0 for none")?;
    if kb > 100_000_000 {
        return Err("`kb` is at most 100000000".into());
    }
    let mut wanted = applied.clone();
    let mut replaced = None;
    if wanted.speed_limit > 0 {
        replaced = Some(wanted.speed_limit);
        wanted.last_speed_limit = wanted.speed_limit;
    }
    wanted.speed_limit = kb;
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
) -> Result<AddAsked, String> {
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
        let inside = full.starts_with(&applied.download_dir)
            && !full
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir));
        if !inside {
            return Err(format!(
                "`folder` must be inside the download folder, {}",
                applied.download_dir.display()
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `url` with a login in it, set through its parts rather than written
    /// out: a login spelled in the source reads to a secret scanner as a leak.
    fn with_login(url: &str, user: &str, word: &str) -> String {
        let mut parsed = url::Url::parse(url).unwrap();
        parsed.set_username(user).unwrap();
        parsed.set_password(Some(word)).unwrap();
        parsed.to_string()
    }

    fn row(id: Id, url: &str, state: State) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(json!({
                "id": id,
                "url": url,
                "state": serde_json::to_value(&state).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "f",
                "cookies": "sid=1",
            }))
            .unwrap(),
        )
    }

    /// A row's sign-in and request headers never leave over the control port,
    /// and a password in its address is masked.
    #[test]
    fn the_list_says_whether_a_row_is_signed_in_and_never_what_with() {
        let word = format!("{}{}", "secret", "word");
        let listed = queue_list(&[row(
            1,
            &with_login("https://h.example/f", "u", &word),
            State::Paused,
        )]);
        let first = &listed[0];
        assert!(first.get("cookies").is_none() && first.get("headers").is_none());
        assert_eq!(first["signed_in"], json!(true));
        assert!(!first["url"].as_str().unwrap().contains(&word));
    }

    /// An add is checked before anything is added: a list, a folder inside the
    /// download folder, both ends of a part; a listed address is answered back
    /// once and not added again unless asked.
    #[test]
    fn an_add_is_checked_and_a_listed_address_is_not_added_twice() {
        let applied = Settings {
            download_dir: std::path::PathBuf::from("/tmp/dm-down"),
            ..Settings::default()
        };
        let rows = [row(1, "https://h.example/a", State::Paused)];
        assert!(add_asked(&rows, &applied, &json!({})).is_err());
        assert!(add_asked(&rows, &applied, &json!({"urls": ["  "]})).is_err());
        assert!(add_asked(
            &rows,
            &applied,
            &json!({"url": "https://h.example/b", "folder": "/etc"})
        )
        .is_err());
        assert!(add_asked(
            &rows,
            &applied,
            &json!({"url": "https://h.example/b", "from": "1:00"})
        )
        .is_err());
        let asked = add_asked(
            &rows,
            &applied,
            &json!({"urls": ["https://h.example/a", "https://h.example/a", "https://h.example/b"], "later": true}),
        )
        .unwrap();
        assert_eq!(asked.fresh, vec!["https://h.example/b".to_string()]);
        assert_eq!(asked.listed, vec!["https://h.example/a".to_string()]);
        assert!(asked.with.later && asked.with.queue == dm_core::MAIN_QUEUE);
        let again = add_asked(
            &rows,
            &applied,
            &json!({"url": "https://h.example/a", "again": true}),
        )
        .unwrap();
        assert_eq!(again.fresh.len(), 1);
    }

    /// A new limit keeps the one it replaces for the menu to turn back on.
    #[test]
    fn a_speed_limit_is_checked_and_keeps_the_one_it_replaces() {
        let applied = Settings {
            speed_limit: 300,
            ..Settings::default()
        };
        assert!(speed_limit(&applied, &json!({})).is_err());
        assert!(speed_limit(&applied, &json!({"kb": 100_000_001u64})).is_err());
        let (wanted, replaced) = speed_limit(&applied, &json!({"kb": 50})).unwrap();
        assert_eq!(
            (wanted.speed_limit, wanted.last_speed_limit, replaced),
            (50, 300, Some(300))
        );
    }
}
