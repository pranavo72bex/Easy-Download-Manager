// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the tray says, whichever window owns it: its menu's words and its
//! tooltip, which says how many downloads run and how fast while some do.
//! The icon and its menu are each window's.

use std::sync::Arc;

use dm_core::{Entry, State};

use crate::i18n::{t, tn};

/// The tray's words in the language in force: its four menu items, then its
/// tooltip when nothing is downloading.
pub fn words() -> [&'static str; 5] {
    [
        t("Show dm"),
        t("Speed limiter"),
        t("Scheduler…"),
        t("Exit dm"),
        t("dm — downloads keep running here"),
    ]
}

/// What the tray's tooltip says while downloads run: how many and how fast,
/// as the status bar says it; `None` when nothing is downloading. A row with
/// every byte is only putting its file together, and is not counted.
pub fn activity(rows: &[Arc<Entry>]) -> Option<String> {
    let running: Vec<&Entry> = rows
        .iter()
        .map(|r| &**r)
        .filter(|r| r.state == State::Running && !dm_shell::app::rules::fetched_all(r))
        .collect();
    if running.is_empty() {
        return None;
    }
    let rate: f64 = running.iter().map(|r| r.progress.speed).sum();
    Some(tn(
        "{} downloading at {}||{} downloading at {}",
        running.len() as u64,
        &[&running.len(), &crate::format::rate(rate)],
    ))
}

/// What is said when the system will make no tray icon (a Linux desktop
/// with no StatusNotifier host, say), for every window alike: why, in its
/// first line, and what follows from it.
pub fn unavailable(why: &str) -> String {
    format!(
        "the tray icon could not be shown ({}); until it can, closing this window \
         quits dm rather than keeping it downloading in the background.",
        why.lines().next().unwrap_or("").trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_running_says_nothing_and_a_download_is_counted() {
        assert_eq!(activity(&[]), None);
        let row: Entry = serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "http://e.com/f",
            "state": serde_json::to_value(State::Running).unwrap(),
            "progress": {"downloaded": 10, "total": 100, "speed": 2048.0, "eta": null},
            "filename": "f",
        }))
        .unwrap();
        let said = activity(&[Arc::new(row)]).unwrap();
        assert!(said.starts_with("1 downloading at"), "{said}");
        assert_eq!(words().len(), 5);
        assert!(unavailable("no host\nmore").contains("(no host);"));
    }
}
