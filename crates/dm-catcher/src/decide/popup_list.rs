// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The toolbar popup's list (gaps rows U18 and C12): what the app is fetching, each with Stop;
//! its last failure; and its recent downloads, each with what can be done to it (Open and Open
//! folder where finished, Resume or Retry where stopped or failed), and Clear finished. The
//! buttons are there only where the app says it acts on downloads
//! ([`dm_protocol::feature::ACTS_ON_DOWNLOADS`]); an older app's popup lists what runs, as before.

use dm_protocol::{act_does, recent_state, Going, LastFailure, Recent, Reply};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::words;

/// The toolbar popup asking the background to have the app do something to a download
/// ([`dm_protocol::Act`]): its `does` and `id` beside the type.
pub const ACT_MESSAGE: &str = "dm.act";

/// What the app said it is fetching and has fetched (`downloads.list`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Listed {
    /// The downloads running, the fastest first.
    pub downloads: Vec<Going>,
    /// How many run in all.
    pub running: usize,
    /// The last download that failed.
    pub failed: Option<LastFailure>,
    /// The downloads stopped or finished last, the newest first.
    pub recent: Vec<Recent>,
}

/// What the app said it is fetching (`downloads.list`), where `frame` is that answer.
pub fn downloads_listed(frame: &Value) -> Option<Listed> {
    match Reply::from_frame(frame).ok()? {
        Reply::Downloads {
            downloads,
            running,
            failed,
            recent,
        } => Some(Listed {
            downloads,
            running,
            failed,
            recent,
        }),
        _ => None,
    }
}

/// The app's answer to an ask of the popup's (`downloads.acted`): `Ok` where it was done, its
/// reason where not; `None` for any other frame.
pub fn acted(frame: &Value) -> Option<Result<(), String>> {
    match Reply::from_frame(frame).ok()? {
        Reply::Acted { reason: None } => Some(Ok(())),
        Reply::Acted { reason: Some(why) } => Some(Err(why)),
        _ => None,
    }
}

/// One button beside a line of the popup's list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PopupButton {
    /// What it asks the app to do: one of [`act_does`].
    pub does: String,
    /// Its words, in the browser's language.
    pub label: String,
}

/// One line of the popup's list, with its buttons.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct PopupRow {
    /// What it says.
    pub line: String,
    /// The download's row in dm's list, which its buttons name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// What can be done to it, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<PopupButton>,
}

/// The popup's list, as the background hands it to the popup.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct PopupList {
    /// What runs, and how many more past those listed.
    pub going: Vec<PopupRow>,
    /// The last failure, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    /// The recent downloads, the newest first.
    pub recent: Vec<PopupRow>,
    /// Clear finished's words, where a finished download is listed for it to clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear: Option<String>,
}

fn button(does: &str, word: words::Word) -> PopupButton {
    PopupButton {
        does: does.to_string(),
        label: word.say(),
    }
}

/// What can be done to a recent download in `state` (one of [`recent_state`]): nothing for one
/// this extension does not know.
fn recent_buttons(state: &str) -> Vec<PopupButton> {
    match state {
        recent_state::FINISHED => vec![
            button(act_does::OPEN, words::POPUP_OPEN_FILE),
            button(act_does::FOLDER, words::POPUP_OPEN_FOLDER),
        ],
        recent_state::STOPPED => vec![button(act_does::RESUME, words::POPUP_RESUME)],
        recent_state::FAILED => vec![button(act_does::RESUME, words::POPUP_RETRY)],
        _ => Vec::new(),
    }
}

/// The popup's list of what the app said (`listed`), each line in the browser's language, with
/// buttons where the app acts on downloads (`acts`).
pub fn popup_list(listed: &Listed, acts: bool) -> PopupList {
    let mut going: Vec<PopupRow> = listed
        .downloads
        .iter()
        .map(|g| PopupRow {
            line: super::going_line(g),
            id: g.id.filter(|_| acts),
            buttons: match g.id {
                Some(_) if acts => vec![button(act_does::STOP, words::POPUP_STOP)],
                _ => Vec::new(),
            },
        })
        .collect();
    if listed.running > listed.downloads.len() {
        going.push(PopupRow {
            line: words::POPUP_MORE.with(&[&(listed.running - listed.downloads.len()).to_string()]),
            ..PopupRow::default()
        });
    }
    let failed = listed
        .failed
        .as_ref()
        .map(|f| words::POPUP_LAST_FAILED.with(&[&f.name, &f.why]));
    let recent: Vec<PopupRow> = if acts {
        listed
            .recent
            .iter()
            .take(dm_protocol::MOST_RECENT)
            .map(|r| PopupRow {
                line: r.name.clone(),
                id: Some(r.id),
                buttons: recent_buttons(&r.state),
            })
            .collect()
    } else {
        Vec::new()
    };
    let finished = acts
        && listed
            .recent
            .iter()
            .any(|r| r.state == recent_state::FINISHED);
    PopupList {
        going,
        failed,
        recent,
        clear: finished.then(|| words::POPUP_CLEAR_FINISHED.say()),
    }
}
